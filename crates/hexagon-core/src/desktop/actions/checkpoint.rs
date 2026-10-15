//! Project-leave checkpoint for the shared computer lane (ui-redesign gap C1,
//! owner ruling 2026-10-14: switching projects must really pause queued and
//! running computer actions and leave a checkpoint the owner can continue).
//!
//! Before this, ticket 13 only made the UI *wait* for the lane to drain and
//! "留在此项目" withdrew the switch; nothing paused the work and nothing told the
//! owner on return that a computer operation had been interrupted.
//!
//! The checkpoint is a durable fact in the leaving project's trace
//! (`System{kind:"computer_checkpoint"}`), never an instruction to replay:
//! continuing only lifts the owner pause. Cost model: a missed checkpoint costs
//! the owner one unexplained pause; replaying the interrupted action could spend
//! an unreviewed GUI side effect against a screen that changed while away. So
//! the interrupted operation is cancelled, the snapshot is dropped, and every
//! later mutation needs a fresh observation (`State::claim_lane`).
use super::*;

pub(crate) const KIND: &str = "computer_checkpoint";

/// Open checkpoint in the current project, shown by the computer-control chip.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, ts_rs::TS)]
#[ts(export, export_to = "../../../ui/src/gen/")]
pub struct DesktopCheckpoint {
    /// Role which held the lane when the owner left.
    pub agent: Option<String>,
    /// An operation was in flight and was cancelled; its effect is unconfirmed.
    pub interrupted: bool,
    pub outcome_unknown: bool,
    /// `project_switch` (owner switched) or `project_close` (host leave path).
    pub reason: String,
    #[ts(type = "number")]
    pub event_id: i64,
}

pub(super) struct Pending {
    agent: String,
    interrupted: bool,
    outcome_unknown: bool,
}

impl State {
    /// Facts to persist if `project` holds the lane; None for idle/foreign lanes.
    pub(super) fn checkpoint_facts(&self, project: &Path) -> Option<Pending> {
        let owner = self.owner.as_ref().filter(|o| o.project == project)?;
        Some(Pending {
            agent: owner.agent.clone(),
            interrupted: self.busy,
            outcome_unknown: self.outcome_unknown || self.busy,
        })
    }

    /// Pause this project's lane and cancel its operation. Foreign lanes are
    /// untouched (same boundary as `close_project`); cancel is a request only.
    fn leave(&mut self, project: &Path) -> Result<Option<Pending>, String> {
        let Some(pending) = self.checkpoint_facts(project) else {
            return Ok(None);
        };
        self.paused = true;
        self.snapshot = None;
        if let Some(id) = self.active {
            self.cancel_active(id)?;
        }
        Ok(Some(pending))
    }
}

/// Latest checkpoint if it has not been resumed or released.
pub fn open(db: &Db) -> Result<Option<DesktopCheckpoint>, String> {
    let row = db
        .conn()
        .query_row(
            "SELECT id, payload FROM events WHERE project_id=?1 AND kind='system' \
             AND json_extract(payload,'$.kind')=?2 ORDER BY id DESC LIMIT 1",
            rusqlite::params![crate::PROJECT_ID, KIND],
            |row| Ok((row.get::<_, i64>(0)?, row.get::<_, String>(1)?)),
        )
        .optional()
        .map_err(|e| e.to_string())?;
    let Some((event_id, text)) = row else {
        return Ok(None);
    };
    let payload: Value = serde_json::from_str(&text).map_err(|e| e.to_string())?;
    if payload["action"] != "saved" {
        return Ok(None);
    }
    Ok(Some(DesktopCheckpoint {
        agent: payload["agent"].as_str().map(str::to_owned),
        interrupted: payload["interrupted"] == true,
        outcome_unknown: payload["outcome_unknown"] == true,
        reason: payload["reason"]
            .as_str()
            .unwrap_or("project_switch")
            .into(),
        event_id,
    }))
}

/// Idempotent: the first save keeps `interrupted` (a later close sees a drained lane).
pub(super) fn save(db: &Db, pending: &Pending, reason: &str) -> Result<(), String> {
    if open(db)?.is_some() {
        return Ok(());
    }
    db.append_event(
        crate::PROJECT_ID,
        EventKind::System,
        json!({"kind":KIND,"action":"saved","agent":pending.agent,
            "interrupted":pending.interrupted,"outcome_unknown":pending.outcome_unknown,
            "reason":reason}),
        // Agent lives in the payload: the trace FK must not make a leave fail
        // because a role row was renamed or removed meanwhile.
        None,
        None,
    )
    .map(|_| ())
    .map_err(|e| e.to_string())
}

/// Owner Resume / Release settles the checkpoint. Never replays anything.
pub(super) fn settle(db: &Db, action: &str) -> Result<(), String> {
    if open(db)?.is_none() {
        return Ok(());
    }
    db.append_event(
        crate::PROJECT_ID,
        EventKind::System,
        json!({"kind":KIND,"action":action,"actor":"owner"}),
        None,
        None,
    )
    .map(|_| ())
    .map_err(|e| e.to_string())
}

fn leave_with(controller: &Controller, db: &Db, project: &Path) -> Result<(), String> {
    let started = Instant::now();
    let mut agent = None;
    let result = (|| {
        let pending = controller
            .state
            .lock()
            .map_err(|_| "desktop state unavailable".to_string())?
            .leave(project)?;
        let Some(pending) = pending else {
            return Ok("no_owned_lane");
        };
        agent = Some(pending.agent.clone());
        save(db, &pending, "project_switch")?;
        Ok(if pending.interrupted {
            "paused_interrupted"
        } else {
            "paused_idle"
        })
    })();
    // AGENTS diagnostics: new lifecycle branch, ids + reason code only.
    crate::diag::note(
        if result.is_err() {
            crate::diag::CLASS_REJECT
        } else {
            crate::diag::CLASS_HOST
        },
        result.is_err(),
        Some(crate::PROJECT_ID),
        agent.as_deref(),
        None,
        None,
        "desktop_leave_checkpoint",
        result.as_ref().map_or("leave_failed", |code| code),
        started,
    );
    result.map(|_| ())
}

/// Owner is switching away: pause this project's lane now (so the drain is a
/// cancellation, not waiting for natural completion) and record the checkpoint.
pub fn leave_project(db: &Db, root: &Path) -> Result<DesktopStatus, String> {
    leave_with(controller(), db, &project_key(root)?)?;
    status(db, root)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};

    struct Slow {
        complete: AtomicBool,
        cancelled: AtomicBool,
        starts: AtomicUsize,
    }
    impl Backend for Slow {
        fn start(&self, _: &str) -> Result<u64, String> {
            self.starts.fetch_add(1, Ordering::SeqCst);
            Ok(7)
        }
        fn cancel(&self, _: u64) -> Result<(), String> {
            self.cancelled.store(true, Ordering::SeqCst);
            self.complete.store(true, Ordering::SeqCst);
            Ok(())
        }
        fn poll(&self, _: u64) -> Result<Option<NativeReply>, String> {
            Ok(self.complete.load(Ordering::SeqCst).then(|| NativeReply {
                protocol_version: 1,
                ok: false,
                result: None,
                applications: None,
                error: Some("cancelled".into()),
                outcome_unknown: true,
                cancelled: true,
                outcome: None,
            }))
        }
    }

    fn seed(db: &Db) {
        db.conn()
            .execute(
                "INSERT INTO projects (id, dir, name, mode) VALUES ('p1','/tmp/x','x','pack')",
                [],
            )
            .unwrap();
    }
    fn req(op: &str, snapshot: Option<&str>) -> NativeRequest {
        serde_json::from_value(json!({"op":op,"snapshot_id":snapshot})).unwrap()
    }
    fn owner(project: &Path) -> Owner {
        Owner {
            project: project.to_path_buf(),
            agent: "qa".into(),
        }
    }

    // Gap C1: switching away pauses + cancels the running action, persists a
    // checkpoint that survives reopen, resume settles it, and the interrupted
    // action is never replayed (a stale snapshot cannot dispatch).
    #[test]
    fn leaving_pauses_checkpoints_and_resumes_without_replay() {
        let dir = tempfile::tempdir().unwrap();
        let project = dir.path().to_path_buf();
        let db_path = dir.path().join("state.db");
        let db = Db::open(&db_path).unwrap();
        seed(&db);
        let backend = Arc::new(Slow {
            complete: AtomicBool::new(false),
            cancelled: AtomicBool::new(false),
            starts: AtomicUsize::new(0),
        });
        let controller = Arc::new(Controller {
            backend: backend.clone(),
            state: Mutex::new(State {
                owner: Some(owner(&project)),
                snapshot: Some("s1".into()),
                ..State::default()
            }),
        });
        let running = controller.clone();
        let who = owner(&project);
        let worker = std::thread::spawn(move || {
            running.run(who, &req("click", Some("s1")), Duration::from_secs(5))
        });
        let deadline = Instant::now() + Duration::from_secs(2);
        while backend.starts.load(Ordering::SeqCst) == 0 && Instant::now() < deadline {
            std::thread::sleep(Duration::from_millis(1));
        }
        leave_with(&controller, &db, &project).unwrap();
        assert!(backend.cancelled.load(Ordering::SeqCst));
        assert!(controller.state.lock().unwrap().paused);
        worker.join().unwrap().unwrap();
        // Second leave (host close path) must not duplicate or erase `interrupted`.
        leave_with(&controller, &db, &project).unwrap();
        controller
            .state
            .lock()
            .unwrap()
            .close_project(&project)
            .unwrap();
        drop(db);

        let reopened = Db::open(&db_path).unwrap();
        let checkpoint = open(&reopened).unwrap().expect("checkpoint persisted");
        assert!(checkpoint.interrupted && checkpoint.outcome_unknown);
        assert_eq!(checkpoint.agent.as_deref(), Some("qa"));
        let saved: i64 = reopened
            .conn()
            .query_row(
                "SELECT count(*) FROM events WHERE json_extract(payload,'$.kind')=?1",
                [KIND],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(saved, 1);

        let mut state = controller.state.lock().unwrap();
        state.control(&project, DesktopControl::Resume).unwrap();
        settle(&reopened, "resumed").unwrap();
        assert!(open(&reopened).unwrap().is_none());
        // No double execution: the pre-leave snapshot cannot authorize a mutation.
        assert!(state
            .claim(&owner(&project), &req("click", Some("s1")))
            .is_err());
        assert_eq!(backend.starts.load(Ordering::SeqCst), 1);
    }

    #[test]
    fn leaving_never_touches_a_foreign_or_idle_lane() {
        let db = Db::open_in_memory().unwrap();
        seed(&db);
        let controller = Controller {
            backend: Arc::new(NativeBackend),
            state: Mutex::new(State {
                owner: Some(owner(Path::new("/other"))),
                ..State::default()
            }),
        };
        leave_with(&controller, &db, Path::new("/mine")).unwrap();
        assert!(!controller.state.lock().unwrap().paused);
        assert!(open(&db).unwrap().is_none());
        settle(&db, "resumed").unwrap();
        assert!(open(&db).unwrap().is_none());
    }
}

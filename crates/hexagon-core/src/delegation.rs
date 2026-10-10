//! Test3/17 (spec 托管委托的附加合同, Q13–Q21, ADR 0086): project delegation.
//!
//! A separate project switch, never a new meaning of Broad. Only the owner's
//! shell entry turns it on or off; no model or tool has a write path. Every
//! managed decision carries `managed:<delegation_id>:<generation>` and is
//! re-verified against the current grant in the committing transaction, so a
//! decision prepared before off/on, a restart or a project switch cannot land.
//! Cost model: an unreadable grant reads as OFF — a false "off" costs one
//! owner decision; a false "on" spends an unreviewed effect.
use crate::db::Db;
use rusqlite::OptionalExtension;

/// The correction rounds one business decision may spend (spec: 三轮).
pub const MAX_ROUNDS: i64 = 3;

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, ts_rs::TS)]
#[ts(export, export_to = "../../../ui/src/gen/")]
pub struct DelegationStatus {
    pub enabled: bool,
    #[ts(type = "number")]
    pub generation: i64,
    pub delegation_id: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Grant {
    pub delegation_id: String,
    pub generation: i64,
}

impl Grant {
    /// The actor recorded on every decision this grant makes.
    pub fn actor(&self) -> String {
        format!("managed:{}:{}", self.delegation_id, self.generation)
    }
}

pub fn status(db: &Db, project_id: &str) -> DelegationStatus {
    let row: Option<(String, i64, i64)> = db
        .conn()
        .query_row(
            "SELECT delegation_id, generation, enabled FROM project_delegations WHERE project_id=?1",
            [project_id],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
        )
        .optional()
        .ok()
        .flatten();
    match row {
        Some((id, generation, 1)) => DelegationStatus {
            enabled: true,
            generation,
            delegation_id: Some(id),
        },
        Some((_, generation, _)) => DelegationStatus {
            enabled: false,
            generation,
            delegation_id: None,
        },
        None => DelegationStatus {
            enabled: false,
            generation: 0,
            delegation_id: None,
        },
    }
}

/// The active grant, if delegation is on.
pub fn current(db: &Db, project_id: &str) -> Option<Grant> {
    let s = status(db, project_id);
    s.enabled.then_some(())?;
    Some(Grant {
        delegation_id: s.delegation_id?,
        generation: s.generation,
    })
}

/// Whether `actor` names the grant that is active right now.
pub fn actor_is_current(db: &Db, project_id: &str, actor: &str) -> bool {
    current(db, project_id).is_some_and(|grant| grant.actor() == actor)
}

fn set(db: &Db, project_id: &str, enabled: bool) -> Result<DelegationStatus, rusqlite::Error> {
    let started = std::time::Instant::now();
    let previous = status(db, project_id).generation;
    let id = format!("d{:x}{:x}", previous + 1, nanos());
    db.conn().execute(
        "INSERT INTO project_delegations(project_id, delegation_id, generation, enabled)
         VALUES (?1, ?2, ?3, ?4)
         ON CONFLICT(project_id) DO UPDATE SET delegation_id=excluded.delegation_id,
           generation=excluded.generation, enabled=excluded.enabled, changed_at=datetime('now')",
        rusqlite::params![project_id, id, previous + 1, enabled as i64],
    )?;
    let after = status(db, project_id);
    let _ = db.append_event(
        project_id,
        crate::trace::EventKind::System,
        serde_json::json!({"kind":"delegation_changed","enabled":enabled,
            "generation":after.generation,"delegation_id":after.delegation_id,"actor":"owner"}),
        None,
        None,
    );
    crate::diag::note(
        crate::diag::CLASS_JUDGE,
        false,
        Some(project_id),
        None,
        None,
        None,
        "delegation",
        if enabled { "enabled" } else { "disabled" },
        started,
    );
    Ok(after)
}

/// Owner turns delegation on: a fresh grant and generation.
pub fn enable(db: &Db, project_id: &str) -> Result<DelegationStatus, rusqlite::Error> {
    set(db, project_id, true)
}

/// Owner turns delegation off: the generation moves on, so any decision still
/// in flight under the old grant fails its commit check.
pub fn disable(db: &Db, project_id: &str) -> Result<DelegationStatus, rusqlite::Error> {
    set(db, project_id, false)
}

fn nanos() -> u128 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0)
}

/// Rounds already spent on one business decision.
pub fn rounds(db: &Db, project_id: &str, key: &str) -> i64 {
    db.conn()
        .query_row(
            "SELECT rounds FROM managed_budgets WHERE project_id=?1 AND decision_key=?2",
            rusqlite::params![project_id, key],
            |r| r.get(0),
        )
        .unwrap_or(0)
}

/// Spend one round; returns the count after spending. Never decreases.
pub fn spend_round(db: &Db, project_id: &str, key: &str) -> Result<i64, rusqlite::Error> {
    db.conn().execute(
        "INSERT INTO managed_budgets(project_id, decision_key, rounds) VALUES (?1, ?2, 1)
         ON CONFLICT(project_id, decision_key) DO UPDATE SET rounds=rounds+1, updated_at=datetime('now')",
        rusqlite::params![project_id, key],
    )?;
    Ok(rounds(db, project_id, key))
}

#[cfg(test)]
mod tests {
    use super::*;
    use proptest::prelude::*;

    fn db() -> Db {
        let db = Db::open_in_memory().unwrap();
        db.conn()
            .execute(
                "INSERT INTO projects (id, dir, name, mode) VALUES ('p1','/tmp/x','x','pack')",
                [],
            )
            .unwrap();
        db
    }

    #[test]
    fn off_by_default_and_every_change_moves_the_generation() {
        let db = db();
        assert!(current(&db, "p1").is_none());
        let on = enable(&db, "p1").unwrap();
        let grant = current(&db, "p1").unwrap();
        assert_eq!(grant.generation, on.generation);
        assert!(actor_is_current(&db, "p1", &grant.actor()));
        disable(&db, "p1").unwrap();
        assert!(
            !actor_is_current(&db, "p1", &grant.actor()),
            "a stale grant never commits"
        );
        let again = enable(&db, "p1").unwrap();
        assert!(again.generation > on.generation);
        assert!(
            !actor_is_current(&db, "p1", &grant.actor()),
            "off/on issues a new grant"
        );
    }

    #[test]
    fn corrupt_rows_read_as_off() {
        let db = db();
        enable(&db, "p1").unwrap();
        db.conn()
            .execute("PRAGMA ignore_check_constraints=ON", [])
            .unwrap();
        db.conn()
            .execute("UPDATE project_delegations SET enabled=7", [])
            .unwrap();
        assert!(current(&db, "p1").is_none());
    }

    proptest! {
        // Budgets only grow, whatever order of spends and off/on toggles.
        #[test]
        fn budgets_are_monotonic(ops in proptest::collection::vec(0u8..3, 1..20)) {
            let db = db();
            let mut last = 0;
            for op in ops {
                match op {
                    0 => { let now = spend_round(&db, "p1", "permission:a1").unwrap(); prop_assert!(now > last); last = now; }
                    1 => { enable(&db, "p1").unwrap(); }
                    _ => { disable(&db, "p1").unwrap(); }
                }
                prop_assert_eq!(rounds(&db, "p1", "permission:a1"), last);
            }
        }
    }
}

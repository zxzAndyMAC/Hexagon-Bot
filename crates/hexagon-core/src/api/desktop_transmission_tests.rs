use super::*;
use crate::desktop::DesktopControl;
use crate::provider::{ChatRequest, ContentBlock, ImageData, Message, Role};
use proptest::prelude::*;

fn request(flags: &[bool]) -> ChatRequest {
    ChatRequest {
        model_slot: "chat".into(),
        tools: vec![],
        messages: vec![Message {
            role: Role::Tool,
            content: vec![ContentBlock::ToolResult {
                tool_use_id: "observed".into(),
                content: "trimmed context".into(),
                is_error: false,
                images: flags
                    .iter()
                    .map(|desktop| ImageData {
                        computer_screenshot: *desktop,
                        media_type: "image/png".into(),
                        data: "pixels".into(),
                    })
                    .collect(),
            }],
        }],
    }
}

#[test]
fn desktop_revocation_blocks_existing_pixels_but_preserves_user_attachments() {
    let dir = tempfile::tempdir().unwrap();
    let wb = Workbench::for_test(dir.path(), &[], None).unwrap();
    assert!(!desktop_request_allowed_for_test(&wb.db, &request(&[true])));
    desktop_control(&wb.db, dir.path(), DesktopControl::Enable).unwrap();
    assert!(desktop_request_allowed_for_test(&wb.db, &request(&[true])));
    desktop_control(&wb.db, dir.path(), DesktopControl::Disable).unwrap();
    // 2026-10-01: disabling the tool previously left screenshots in the running
    // turn's request. Each retry must honor revocation even after tool compaction.
    assert!(!desktop_request_allowed_for_test(
        &wb.db,
        &request(&[false, true])
    ));
    assert!(desktop_request_allowed_for_test(&wb.db, &request(&[false])));
    assert!(desktop_request_allowed_for_test(&wb.db, &request(&[])));
}

proptest! {
    #[test]
    fn revoked_desktop_pixels_never_cross_transport(flags in prop::collection::vec(any::<bool>(), 0..12)) {
        let dir = tempfile::tempdir().unwrap();
        let wb = Workbench::for_test(dir.path(), &[], None).unwrap();
        prop_assert_eq!(desktop_request_allowed_for_test(&wb.db, &request(&flags)), !flags.iter().any(|v| *v));
    }
}

struct WriterQueue {
    queued: std::sync::mpsc::SyncSender<()>,
    release: std::sync::mpsc::Receiver<()>,
}
thread_local! {
    static WRITER_QUEUE: std::cell::RefCell<Option<WriterQueue>> = const { std::cell::RefCell::new(None) };
}

fn queued_writer(_: i32) -> bool {
    WRITER_QUEUE.with(|slot| {
        let Some(WriterQueue { queued, release }) = slot.borrow_mut().take() else {
            return true;
        };
        queued.send(()).is_ok()
            && release
                .recv_timeout(std::time::Duration::from_secs(30))
                .is_ok()
    })
}

struct CountTransport(std::sync::Arc<std::sync::atomic::AtomicUsize>);
impl crate::provider::ModelProvider for CountTransport {
    fn complete(
        &self,
        _: &ChatRequest,
    ) -> Result<crate::provider::ChatResponse, crate::provider::ProviderError> {
        self.0.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        Ok(crate::provider::ChatResponse {
            content: vec![],
            stop: crate::provider::StopReason::EndTurn,
            usage: crate::provider::Usage::default(),
        })
    }
}

#[test]
fn queued_transmission_rechecks_revocation_before_transport_and_accounts_not_sent() {
    for computer_screenshot in [true, false] {
        let dir = tempfile::tempdir().unwrap();
        let mut wb = Workbench::for_test(dir.path(), &["QA"], None).unwrap();
        let path = dir.path().join("transmission.db");
        wb.db = Db::open(&path).unwrap();
        wb.db.conn().execute("INSERT INTO projects(id,dir,name,mode,autonomy) VALUES(?1,?2,'queued transmission','pack','L0')", rusqlite::params![crate::PROJECT_ID,dir.path().display().to_string()]).unwrap();
        wb.db
            .conn()
            .execute(
                "INSERT INTO agents(id,project_id,role,status) VALUES('a0',?1,'QA','active')",
                [crate::PROJECT_ID],
            )
            .unwrap();
        desktop_control(&wb.db, dir.path(), DesktopControl::Enable).unwrap();
        let writer = Db::open(&path).unwrap();
        writer.conn().execute_batch("BEGIN IMMEDIATE").unwrap();
        let calls = std::sync::Arc::new(std::sync::atomic::AtomicUsize::new(0));
        let transport = CountTransport(calls.clone());
        let (queued_tx, queued_rx) = std::sync::mpsc::sync_channel(1);
        let (release_tx, release_rx) = std::sync::mpsc::sync_channel(1);
        let worker = std::thread::spawn(move || {
            WRITER_QUEUE.with(|slot| {
                *slot.borrow_mut() = Some(WriterQueue {
                    queued: queued_tx,
                    release: release_rx,
                })
            });
            wb.db.conn().busy_handler(Some(queued_writer)).unwrap();
            let ctx = wb.ctx_for("a0", None);
            // Same streaming/retry/admission path as a real Workbench turn.
            let result = crate::turn::stream_request_for_test(
                &wb.db,
                &ctx,
                &transport,
                &request(&[computer_screenshot]),
            );
            eprintln!("queued transmission result: {result:?}");
            let ledger: (String,i64,bool) = wb.db.conn().query_row("SELECT request_state,reserved_mc,cost_known FROM usage WHERE record_kind='request' ORDER BY rowid DESC LIMIT 1",[],|r|Ok((r.get(0)?,r.get(1)?,r.get(2)?))).unwrap();
            (result.is_ok(), ledger)
        });
        // No sleeps: the busy handler proves the initial consent check already
        // passed and the actual request is now waiting for the SQLite writer.
        queued_rx
            .recv_timeout(std::time::Duration::from_secs(30))
            .unwrap();
        desktop_control(&writer, dir.path(), DesktopControl::Disable).unwrap();
        writer.conn().execute_batch("COMMIT").unwrap();
        release_tx.send(()).unwrap();
        let (success, (state, reserved, known)) = worker.join().unwrap();
        assert_eq!(
            calls.load(std::sync::atomic::Ordering::SeqCst),
            usize::from(!computer_screenshot)
        );
        assert_eq!(success, !computer_screenshot);
        assert_eq!(reserved, 0);
        if computer_screenshot {
            assert_eq!(state, "not_sent");
            assert!(known, "a denied transmission has known zero model cost");
        } else {
            assert_eq!(
                state, "succeeded",
                "owner-supplied attachments retain their independent authority"
            );
        }
    }
}

// Issue13: owner-selected browser pixels use the same last-dispatch consent gate
// and add clear-generation freshness. Replaying serialized history carries no pixels.
#[test]
fn selected_browser_pixels_recheck_consent_and_clear_generation() {
    let dir = tempfile::tempdir().unwrap();
    let wb = Workbench::for_test(dir.path(), &[], None).unwrap();
    let generation = crate::desktop::actions::capture_generation();
    let mut req = request(&[]);
    req.messages[0].content = vec![ContentBlock::ComputerImage {
        media_type: "image/png".into(),
        data: "pixels".into(),
        generation,
    }];
    assert!(!desktop_request_allowed_for_test(&wb.db, &req));
    desktop_control(&wb.db, dir.path(), DesktopControl::Enable).unwrap();
    // An intentionally stale receipt always fails, independent of other tests clearing evidence.
    if let ContentBlock::ComputerImage { generation, .. } = &mut req.messages[0].content[0] {
        *generation = u64::MAX;
    }
    assert!(!desktop_request_allowed_for_test(&wb.db, &req));
    desktop_control(&wb.db, dir.path(), DesktopControl::Disable).unwrap();
    assert!(!desktop_request_allowed_for_test(&wb.db, &req));
}

proptest! {
    #[test]
    fn selected_image_pixels_never_escape_without_current_consent(data in ".{0,32}", generation in any::<u64>()) {
        let dir = tempfile::tempdir().unwrap();
        let wb = Workbench::for_test(dir.path(), &[], None).unwrap();
        let mut req = request(&[]);
        req.messages[0].content = vec![ContentBlock::ComputerImage { media_type: "image/png".into(), data: data.clone(), generation }];
        prop_assert_eq!(desktop_request_allowed_for_test(&wb.db, &req), data.is_empty());
    }
}

//! Quality behavior through the existing Workbench evidence/acceptance seam.
use super::*;

#[cfg(target_os = "macos")]
#[test]
#[ignore = "requires locally installed Playwright Firefox runtime; run explicitly during browser QA"]
fn quality_browser_starts_real_firefox_inside_terminal_sandbox() {
    let runtime = std::path::PathBuf::from(
        std::env::var_os("HEXAGON_BROWSER_QA_RUNTIME")
            .expect("HEXAGON_BROWSER_QA_RUNTIME must name the owned QA runtime fixture"),
    );
    let dir = tempfile::tempdir().unwrap();
    let pack: PackDef = serde_json::from_value(json!({"name":"browser QA","version":1,"stages":[{"name":"delivery","roles":["dev"],"due":[],"checks":[],"quality_checks":{"accessibility":"node probe.mjs"},"stamp_point":true}]})).unwrap();
    let wb = Workbench::for_test(dir.path(), &["dev"], Some(pack)).unwrap();
    wb.open_stage(0).unwrap();
    let copied = std::process::Command::new("/bin/cp")
        .args(["-R", runtime.to_str().unwrap()])
        .arg(dir.path().join("runtime"))
        .status()
        .unwrap();
    assert!(copied.success());
    std::fs::create_dir(dir.path().join("tmp")).unwrap();
    std::fs::write(
        dir.path().join("browser-page.html"),
        "<h1>Browser QA</h1><label>Value<input id=\"value\"></label>",
    )
    .unwrap();
    std::fs::write(dir.path().join("probe.mjs"), r#"
import assert from 'node:assert/strict';
process.env.TMPDIR = process.cwd() + '/tmp';
process.env.PLAYWRIGHT_BROWSERS_PATH = process.cwd() + '/runtime/browsers';
process.env.DEBUG = 'pw:browser';
const {firefox} = await import('./runtime/node_modules/playwright/index.mjs');
const watchdog = setTimeout(() => { console.error('browser QA deadline'); process.exit(3); }, 15000);
const browser = await firefox.launch({headless:true,timeout:10000});
try {
  const page = await browser.newPage();
  await page.goto(new URL('file://' + process.cwd() + '/browser-page.html').href);
  await page.locator('#value').focus();
  await page.keyboard.type('real-browser');
  assert.equal(await page.locator('#value').inputValue(), 'real-browser');
  assert.equal(await page.locator('h1').textContent(), 'Browser QA');
  console.log('SANDBOX_BROWSER_OK');
} finally { await browser.close(); clearTimeout(watchdog); }
"#).unwrap();
    std::fs::write(
        dir.path().join("page.tsx"),
        "export const Page = () => <input />",
    )
    .unwrap();
    wb.run_checks().unwrap();
    let events = wb
        .db
        .events(&wb.project_id, Some(&[EventKind::TestRan]))
        .unwrap();
    let check = events
        .iter()
        .find(|event| event.payload["cmd"] == "quality:accessibility")
        .unwrap();
    assert_eq!(check.payload["exit_code"], 0, "{}", check.payload);
    assert!(check.payload["stdout"]
        .as_str()
        .unwrap()
        .contains("SANDBOX_BROWSER_OK"));
}

#[cfg(target_os = "macos")]
#[test]
#[ignore = "requires locally installed Playwright Firefox runtime; run explicitly during browser QA"]
fn sandbox_browser_timeout_and_cancellation_leave_no_running_browser_or_late_writes() {
    use std::time::{Duration, Instant};
    let runtime = std::path::PathBuf::from(
        std::env::var_os("HEXAGON_BROWSER_QA_RUNTIME").expect("owned Firefox QA runtime required"),
    );
    for timeout in [false, true] {
        let (dir, wb) = fixture(&[]);
        assert!(std::process::Command::new("/bin/cp")
            .arg("-R")
            .arg(&runtime)
            .arg(dir.path().join("runtime"))
            .status()
            .unwrap()
            .success());
        std::fs::create_dir(dir.path().join("tmp")).unwrap();
        std::fs::write(dir.path().join("probe.mjs"), r#"
import fs from 'node:fs';
process.env.TMPDIR = process.cwd() + '/tmp';
process.env.PLAYWRIGHT_BROWSERS_PATH = process.cwd() + '/runtime/browsers';
const {firefox} = await import('./runtime/node_modules/playwright/index.mjs');
const browser = await firefox.launch({headless:true,timeout:10000});
const page = await browser.newPage();
await page.setContent('<h1>Owned lifecycle probe</h1>');
if (await page.locator('h1').textContent() !== 'Owned lifecycle probe') throw new Error('DOM not ready');
let count = 0;
fs.writeFileSync('heartbeat', String(count));
setInterval(() => fs.writeFileSync('heartbeat', String(++count)), 100);
fs.writeFileSync('ready.pid', String(process.pid));
await new Promise(() => {});
"#).unwrap();
        let root = dir.path().to_path_buf();
        let observer = std::thread::spawn(move || {
            let deadline = Instant::now() + Duration::from_secs(18);
            let pid: i32 = loop {
                if let Some(pid) = std::fs::read_to_string(root.join("ready.pid"))
                    .ok()
                    .and_then(|value| value.parse().ok())
                {
                    break pid;
                }
                assert!(
                    Instant::now() < deadline,
                    "real browser did not reach DOM readiness"
                );
                std::thread::sleep(Duration::from_millis(20));
            };
            // Metadata only: never print command arguments or inherited env.
            let group = unsafe { libc::getpgid(pid) };
            assert!(group > 0);
            let output = std::process::Command::new("/bin/ps")
                .args(["-axo", "pid,pgid,stat,comm"])
                .output()
                .unwrap();
            let text = String::from_utf8(output.stdout).unwrap();
            assert!(
                text.lines().any(|line| {
                    let words: Vec<_> = line.split_whitespace().collect();
                    words.len() >= 4
                        && words[1].parse::<i32>() == Ok(group)
                        && line.to_lowercase().contains("firefox")
                }),
                "no real Firefox in the owned process group"
            );
            group
        });
        let context = crate::tools::ToolContext {
            project_id: wb.project_id.clone(),
            agent_id: "owner".into(),
            stage_run_id: Some(wb.db.active_stage_run(&wb.project_id).unwrap().unwrap().id),
            repo_root: dir.path().into(),
            ..Default::default()
        };
        let sessions = crate::sessions::SessionTable::default();
        let group = if timeout {
            let result = sessions
                .run_oneshot(
                    &wb.db,
                    &context,
                    "node probe.mjs",
                    Duration::from_secs(20),
                    false,
                )
                .unwrap();
            assert_eq!(result["timed_out"], true, "{result}");
            observer.join().unwrap()
        } else {
            let task = sessions
                .spawn_task(&wb.db, &context, "node probe.mjs", false)
                .unwrap();
            let group = observer.join().unwrap();
            sessions
                .kill(&wb.db, &context, task["task_id"].as_str(), None)
                .unwrap();
            group
        };
        let heartbeat = std::fs::read_to_string(dir.path().join("heartbeat")).unwrap();
        std::thread::sleep(Duration::from_secs(2));
        assert_eq!(
            std::fs::read_to_string(dir.path().join("heartbeat")).unwrap(),
            heartbeat,
            "write after lifecycle cleanup returned"
        );
        let output = std::process::Command::new("/bin/ps")
            .args(["-axo", "pid,pgid,stat,comm"])
            .output()
            .unwrap();
        assert!(
            !String::from_utf8(output.stdout)
                .unwrap()
                .lines()
                .any(|line| {
                    let words: Vec<_> = line.split_whitespace().collect();
                    words.len() >= 4
                        && words[1].parse::<i32>() == Ok(group)
                        && !words[2].contains('Z')
                }),
            "running descendant survived timeout={timeout}"
        );
    }
}

fn fixture(contracts: &[(&str, &str)]) -> (tempfile::TempDir, Workbench) {
    let checks: Vec<_> = contracts.iter().map(|(_, cmd)| *cmd).collect();
    let quality_checks: serde_json::Map<String, serde_json::Value> = contracts
        .iter()
        .map(|(kind, cmd)| (kind.to_string(), json!(cmd)))
        .collect();
    let dir = tempfile::tempdir().unwrap();
    let pack: PackDef = serde_json::from_value(json!({"name":"quality fixture","version":1,"stages":[{"name":"delivery","roles":["dev"],"due":[],"checks":checks,"quality_checks":quality_checks,"stamp_point":true}]})).unwrap();
    let wb = Workbench::for_test(dir.path(), &["dev"], Some(pack)).unwrap();
    wb.open_stage(0).unwrap();
    (dir, wb)
}

#[test]
fn reading_missing_check_evidence_does_not_append_diagnostics() {
    let _lock = crate::diag::TEST_LEVEL_LOCK
        .lock()
        .unwrap_or_else(|e| e.into_inner());
    struct RestoreLevel(log::LevelFilter);
    impl Drop for RestoreLevel {
        fn drop(&mut self) {
            log::set_max_level(self.0);
        }
    }
    let _restore = RestoreLevel(log::max_level());
    log::set_max_level(log::LevelFilter::Debug);
    let (dir, wb) = fixture(&[]);
    let mut pack = wb.pack().unwrap().clone();
    pack.stages[0].due = vec!["source".into()];
    std::fs::write(
        dir.path().join("page.tsx"),
        "export const Page = () => <button />",
    )
    .unwrap();
    let before = crate::diag::records(Some(&wb.project_id), None).len();
    for _ in 0..5 {
        assert!(!orchestra::stage_evidence(&wb.db, &wb.project_id, &pack)
            .unwrap()
            .unwrap()
            .missing
            .is_empty());
    }
    assert_eq!(
        crate::diag::records(Some(&wb.project_id), None).len(),
        before
    );
}

#[test]
fn completed_flow_can_recheck_confirmed_commands_without_reopening_history() {
    let (dir, wb) = fixture(&[("tests", "sh pass.sh")]);
    std::fs::write(dir.path().join("pass.sh"), "exit 0\n").unwrap();
    std::fs::write(dir.path().join("other.sh"), "exit 0\n").unwrap();
    std::fs::write(dir.path().join("changed.rs"), "fn changed() {}\n").unwrap();
    wb.run_checks().unwrap();
    assert!(matches!(
        wb.advance().unwrap(),
        orchestra::StageAction::AwaitingStamp { .. }
    ));
    assert!(matches!(
        wb.stamp().unwrap(),
        orchestra::StageAction::PackFinished
    ));
    let count: i64 = wb
        .db
        .conn()
        .query_row("SELECT COUNT(*) FROM stage_runs", [], |r| r.get(0))
        .unwrap();
    let config = wb.quality_configuration().unwrap();
    let mut commands = config.stages[0].commands.clone();
    commands.insert(orchestra::QualityCategory::Tests, "sh other.sh".into());
    wb.update_quality_commands(0, &config.version, &commands)
        .unwrap();
    let before = wb.stage_evidence().unwrap().unwrap();
    let card = wb
        .db
        .queued_questions(&wb.project_id)
        .unwrap()
        .into_iter()
        .find(|q| q.payload["sub"] == "quality_revalidation")
        .unwrap();
    assert!(wb.confirm_quality_revalidation(&card.id, "stale").is_err());
    assert!(before.missing.iter().any(|m| m == "check:quality:tests"));
    assert!(matches!(
        wb.stamp().unwrap(),
        orchestra::StageAction::Incomplete { .. }
    ));
    wb.cancel_quality_revalidation(&card.id).unwrap();
    assert!(wb.db.queued_questions(&wb.project_id).unwrap().is_empty());
    wb.run_checks().unwrap();
    assert!(wb.stage_evidence().unwrap().unwrap().missing.is_empty());
    assert!(
        wb.stamp().is_err(),
        "ordinary stamp cannot accept a completed run without its displayed version"
    );
    let orchestra::StageAction::AwaitingStamp { question_id, .. } = wb.advance().unwrap() else {
        panic!("owner revalidation request missing")
    };
    assert_ne!(question_id, card.id);
    assert!(matches!(
        wb.confirm_quality_revalidation(
            &question_id,
            wb.stage_evidence()
                .unwrap()
                .unwrap()
                .fingerprint
                .as_deref()
                .unwrap()
        )
        .unwrap(),
        orchestra::StageAction::PackFinished
    ));
    assert_eq!(
        wb.db
            .conn()
            .query_row("SELECT COUNT(*) FROM stage_runs", [], |r| r
                .get::<_, i64>(0))
            .unwrap(),
        count
    );
    assert_eq!(
        wb.db
            .conn()
            .query_row("SELECT state FROM stage_runs", [], |r| r
                .get::<_, String>(0))
            .unwrap(),
        "done"
    );
    assert_eq!(
        wb.db
            .events(&wb.project_id, Some(&[EventKind::Stamped]))
            .unwrap()
            .len(),
        2
    );
    assert!(wb.db.queued_questions(&wb.project_id).unwrap().is_empty());
    assert!(matches!(
        wb.advance().unwrap(),
        orchestra::StageAction::PackFinished
    ));
    assert!(
        wb.stamp().is_err(),
        "an unchanged completed flow cannot be accepted twice"
    );
}

#[test]
fn completed_quality_revalidation_checks_all_stages_with_or_without_a_stamp_or_skipped_tail() {
    for shape in 0..3 {
        let dir = tempfile::tempdir().unwrap();
        let later_roles = if shape == 2 {
            vec!["absent"]
        } else {
            vec!["dev"]
        };
        let pack: PackDef = serde_json::from_value(json!({"name":"completion shapes","version":1,"stages":[
            // QA Q4 / 2026-10-05: an ordinary check used to mask the missing
            // quality alias on historical stages. Exercise the quality contract alone.
            {"name":"build","roles":["dev"],"due":[],"checks":[],"quality_checks":{"tests":"sh old.sh"},"stamp_point":shape == 1},
            {"name":"checks","roles":later_roles,"due":[],"quality_checks":{"tests":"sh later.sh"},"stamp_point":false},
            {"name":"tail","roles":later_roles,"due":[],"quality_checks":{"tests":"sh later.sh"},"stamp_point":false}
        ]})).unwrap();
        let wb = Workbench::for_test(dir.path(), &["dev"], Some(pack)).unwrap();
        for file in ["old.sh", "new.sh", "later.sh"] {
            std::fs::write(dir.path().join(file), "exit 0\n").unwrap();
        }
        std::fs::write(dir.path().join("source.rs"), "fn initial() {}\n").unwrap();
        let early_run = wb.open_stage(0).unwrap().run_id;
        wb.run_checks().unwrap();
        let first = wb.advance().unwrap();
        let mut action = if shape == 1 {
            assert!(matches!(
                first,
                orchestra::StageAction::AwaitingStamp { .. }
            ));
            wb.stamp().unwrap()
        } else {
            first
        };
        while matches!(action, orchestra::StageAction::StageOpened { .. }) {
            wb.run_checks().unwrap();
            action = wb.advance().unwrap();
        }
        assert!(matches!(action, orchestra::StageAction::PackFinished));
        let config = wb.quality_configuration().unwrap();
        wb.update_quality_commands(
            0,
            &config.version,
            &std::collections::BTreeMap::from([(
                orchestra::QualityCategory::Tests,
                "sh new.sh".into(),
            )]),
        )
        .unwrap();
        let card = wb
            .db
            .queued_questions(&wb.project_id)
            .unwrap()
            .into_iter()
            .find(|q| q.payload["sub"] == "quality_revalidation")
            .unwrap();
        assert!(
            !wb.stage_evidence().unwrap().unwrap().missing.is_empty(),
            "early-stage changes require new checks even with a later local runner"
        );
        orchestra::run_quality_checks(&wb.db, &wb.project_id, dir.path(), wb.pack().unwrap())
            .unwrap();
        assert!(
            wb.db
                .events(&wb.project_id, Some(&[EventKind::TestRan]))
                .unwrap()
                .iter()
                .any(
                    |event| event.stage_run_id.as_deref() == Some(early_run.as_str())
                        && event.payload["cmd"] == "quality:tests"
                        && event.payload["quality"]["command"] == "sh new.sh"
                ),
            "the historical quality runner must really execute, including at automatic checkpoints"
        );
        let evidence = wb.stage_evidence().unwrap().unwrap();
        assert!(
            evidence.missing.is_empty(),
            "shape {shape}: {:?}",
            evidence.missing
        );
        if shape == 2 {
            // D08/Q4 regression: a skipped tail must not hide a rejected middle.
            wb.db
                .conn()
                .execute(
                    "UPDATE stage_runs SET state='rejected' WHERE project_id=?1 AND seq=1",
                    [&wb.project_id],
                )
                .unwrap();
            let invalid = wb.stage_evidence().unwrap().unwrap();
            assert!(invalid.missing.iter().any(|m| m == "stage:checks"));
            assert!(matches!(
                wb.confirm_quality_revalidation(&card.id, invalid.fingerprint.as_deref().unwrap())
                    .unwrap(),
                orchestra::StageAction::Incomplete { .. }
            ));
            wb.db
                .conn()
                .execute(
                    "UPDATE stage_runs SET state='skipped' WHERE project_id=?1 AND seq=1",
                    [&wb.project_id],
                )
                .unwrap();
        }
        let evidence = wb.stage_evidence().unwrap().unwrap();
        assert!(matches!(
            wb.confirm_quality_revalidation(&card.id, evidence.fingerprint.as_deref().unwrap())
                .unwrap(),
            orchestra::StageAction::PackFinished
        ));
        assert!(matches!(
            wb.advance().unwrap(),
            orchestra::StageAction::PackFinished
        ));
        assert!(wb.db.queued_questions(&wb.project_id).unwrap().is_empty());
    }
}

#[test]
fn completed_historical_performance_change_requires_real_samples_and_owner_confirmation() {
    let dir = tempfile::tempdir().unwrap();
    for (name, body) in [
        ("old.sh", "sleep 0.03\n"),
        ("new.sh", "sleep 0.001\n"),
        ("later.sh", "exit 0\n"),
    ] {
        std::fs::write(dir.path().join(name), body).unwrap();
    }
    let pack: PackDef = serde_json::from_value(json!({"name":"historical performance","version":1,"stages":[
        {"name":"build","roles":["dev"],"due":[],"checks":[],"quality_checks":{"performance":"sh old.sh"},"stamp_point":false},
        {"name":"delivery","roles":["dev"],"due":[],"quality_checks":{"performance":"sh later.sh"},"stamp_point":false}
    ]})).unwrap();
    let wb = Workbench::for_test(dir.path(), &["dev"], Some(pack)).unwrap();
    let earlier = wb.open_stage(0).unwrap().run_id;
    let config = wb.quality_configuration().unwrap();
    wb.update_quality_commands(
        0,
        &config.version,
        &std::collections::BTreeMap::from([(
            orchestra::QualityCategory::Performance,
            "sh old.sh # initial measurement".into(),
        )]),
    )
    .unwrap();
    wb.run_checks().unwrap();
    assert!(matches!(
        wb.advance().unwrap(),
        orchestra::StageAction::StageOpened { .. }
    ));
    assert!(matches!(
        wb.advance().unwrap(),
        orchestra::StageAction::PackFinished
    ));
    let config = wb.quality_configuration().unwrap();
    wb.update_quality_commands(
        0,
        &config.version,
        &std::collections::BTreeMap::from([(
            orchestra::QualityCategory::Performance,
            "sh new.sh".into(),
        )]),
    )
    .unwrap();
    let card = wb
        .db
        .queued_questions(&wb.project_id)
        .unwrap()
        .into_iter()
        .find(|q| q.payload["sub"] == "quality_revalidation")
        .unwrap();
    let missing = wb.stage_evidence().unwrap().unwrap();
    assert!(matches!(
        wb.confirm_quality_revalidation(&card.id, missing.fingerprint.as_deref().unwrap())
            .unwrap(),
        orchestra::StageAction::Incomplete { .. }
    ));
    wb.run_checks().unwrap();
    let measured = wb.stage_evidence().unwrap().unwrap();
    let check = measured
        .checks
        .iter()
        .find(|c| c.run_id == earlier && c.cmd == "quality:performance")
        .unwrap();
    let event = wb
        .db
        .events(&wb.project_id, Some(&[EventKind::TestRan]))
        .unwrap()
        .into_iter()
        .find(|event| Some(event.id) == check.event_id)
        .unwrap();
    assert_eq!(event.payload["quality"]["command"], "sh new.sh");
    assert_eq!(
        event.payload["quality"]["samples_ms"]
            .as_array()
            .unwrap()
            .len(),
        3
    );
    assert_eq!(check.state, orchestra::CheckState::Failed);
    assert_eq!(
        check.quality.as_ref().unwrap().reason,
        "measurement_changed"
    );
    assert!(matches!(
        wb.confirm_quality_revalidation(&card.id, measured.fingerprint.as_deref().unwrap())
            .unwrap(),
        orchestra::StageAction::Incomplete { .. }
    ));
    wb.confirm_performance_baseline(
        check.event_id.unwrap(),
        measured.fingerprint.as_deref().unwrap(),
        "confirmed changed historical benchmark",
    )
    .unwrap();
    // Q2/Q4: changing the reference cannot retroactively pass the failed fact.
    assert!(wb
        .stage_evidence()
        .unwrap()
        .unwrap()
        .checks
        .iter()
        .any(|c| c.run_id == earlier && c.state == orchestra::CheckState::Failed));
    wb.run_checks().unwrap();
    let ready = wb.stage_evidence().unwrap().unwrap();
    assert!(ready.missing.is_empty(), "{ready:?}");
    assert!(matches!(
        wb.confirm_quality_revalidation(&card.id, ready.fingerprint.as_deref().unwrap())
            .unwrap(),
        orchestra::StageAction::PackFinished
    ));
    assert!(wb
        .db
        .events(&wb.project_id, Some(&[EventKind::TestRan]))
        .unwrap()
        .iter()
        .any(|historic| historic.id == event.id
            && historic.payload["quality"]["verdict"] == "measurement_changed"));
}

#[test]
fn quality_confirmation_from_another_identical_project_is_rejected() {
    let (_one, first) = fixture(&[]);
    let (_two, second) = fixture(&[]);
    let config = first.quality_configuration().unwrap();
    assert_ne!(
        config.version,
        second.quality_configuration().unwrap().version
    );
    let commands =
        std::collections::BTreeMap::from([(orchestra::QualityCategory::Tests, "true".into())]);
    assert!(second
        .update_quality_commands(0, &config.version, &commands)
        .is_err());
    assert!(second.quality_configuration().unwrap().stages[0]
        .commands
        .is_empty());
}

#[test]
fn quality_replay_keeps_independent_checks_when_a_new_runner_uses_the_same_command() {
    let (dir, wb) = fixture(&[("tests", "runner-A")]);
    let mut base = wb.pack().unwrap().clone();
    base.stages[0].checks = vec!["independent-C".into()];
    let first = orchestra::quality_configuration(&wb.db, &wb.project_id, &base).unwrap();
    let removed = orchestra::update_quality_commands(
        &wb.db,
        &wb.project_id,
        &base,
        0,
        &first.version,
        &std::collections::BTreeMap::new(),
    )
    .unwrap();
    let commands = std::collections::BTreeMap::from([(
        orchestra::QualityCategory::Tests,
        "independent-C".into(),
    )]);
    orchestra::update_quality_commands(
        &wb.db,
        &wb.project_id,
        &base,
        0,
        &removed.version,
        &commands,
    )
    .unwrap();
    let once = orchestra::current_process_pack(&wb.db, &wb.project_id, &base).unwrap();
    let twice = orchestra::current_process_pack(&wb.db, &wb.project_id, &once).unwrap();
    assert_eq!(once.stages[0].checks, vec!["independent-C"]);
    assert_eq!(
        serde_json::to_value(&once).unwrap(),
        serde_json::to_value(&twice).unwrap()
    );
    assert_eq!(
        orchestra::quality_configuration(&wb.db, &wb.project_id, &once)
            .unwrap()
            .version,
        orchestra::quality_configuration(&wb.db, &wb.project_id, &base)
            .unwrap()
            .version
    );
    drop(dir);
}

#[test]
fn owner_quality_command_changes_expire_checks_and_exceptions_in_current_flow() {
    let (dir, wb) = fixture(&[("tests", "sh fail.sh")]);
    std::fs::write(dir.path().join("fail.sh"), "exit 7\n").unwrap();
    std::fs::write(dir.path().join("pass.sh"), "exit 0\n").unwrap();
    std::fs::write(dir.path().join("changed.rs"), "fn changed() {}\n").unwrap();
    wb.run_checks().unwrap();
    let old = wb.stage_evidence().unwrap().unwrap();
    let expected = old.fingerprint.as_deref().unwrap();
    let request = wb.request_acceptance_exception(expected).unwrap();
    let selected: Vec<_> = old
        .exceptions
        .iter()
        .map(|e| e.requirement.clone())
        .collect();
    wb.accept_delivery_exception(
        &request.question_id,
        expected,
        &selected,
        "known fixture failure",
    )
    .unwrap();
    assert!(wb
        .stage_evidence()
        .unwrap()
        .unwrap()
        .exceptions
        .iter()
        .all(|e| e.accepted));
    let config = wb.quality_configuration().unwrap();
    let mut commands = config.stages[0].commands.clone();
    commands.insert(orchestra::QualityCategory::Tests, "sh pass.sh".into());
    assert!(wb
        .update_quality_commands(0, "obsolete", &commands)
        .is_err());
    let changed = wb
        .update_quality_commands(0, &config.version, &commands)
        .unwrap();
    assert_ne!(changed.version, config.version);
    assert_eq!(
        changed.stages[0].commands[&orchestra::QualityCategory::Tests],
        "sh pass.sh"
    );
    let current = wb.stage_evidence().unwrap().unwrap();
    assert_ne!(current.fingerprint, old.fingerprint);
    assert!(current
        .checks
        .iter()
        .all(|e| e.state != orchestra::CheckState::Passed));
    assert!(current.exceptions.iter().all(|e| !e.accepted));
    assert!(wb
        .accept_delivery_exception(
            &request.question_id,
            expected,
            &selected,
            "known fixture failure"
        )
        .is_err());
    assert!(wb
        .db
        .events(&wb.project_id, Some(&[EventKind::System]))
        .unwrap()
        .iter()
        .any(|e| e.payload["kind"] == "acceptance_exception"));
    wb.run_checks().unwrap();
    assert!(wb
        .stage_evidence()
        .unwrap()
        .unwrap()
        .checks
        .iter()
        .all(|e| e.state == orchestra::CheckState::Passed));
    commands.insert(orchestra::QualityCategory::Tests, "sh fail.sh".into());
    let returned = wb
        .update_quality_commands(0, &changed.version, &commands)
        .unwrap();
    let current = wb.stage_evidence().unwrap().unwrap();
    assert!(
        current.exceptions.iter().all(|e| !e.accepted),
        "A→B→A cannot revive an old exception"
    );
    assert!(current
        .checks
        .iter()
        .all(|e| e.state != orchestra::CheckState::Passed));
    let unchanged = wb
        .update_quality_commands(0, &returned.version, &commands)
        .unwrap();
    assert_eq!(unchanged.version, returned.version);
}

#[test]
fn changing_one_category_keeps_other_current_checks_and_owner_exceptions() {
    let (dir, wb) = fixture(&[("tests", "sh fail.sh"), ("accessibility", "sh pass.sh")]);
    std::fs::write(dir.path().join("fail.sh"), "exit 7\n").unwrap();
    std::fs::write(dir.path().join("pass.sh"), "exit 0\n").unwrap();
    std::fs::write(dir.path().join("other.sh"), "exit 0\n").unwrap();
    std::fs::write(
        dir.path().join("page.tsx"),
        "export const Page = () => <input />",
    )
    .unwrap();
    wb.run_checks().unwrap();
    let evidence = wb.stage_evidence().unwrap().unwrap();
    let expected = evidence.fingerprint.as_deref().unwrap();
    let selected: Vec<_> = evidence.exceptions.iter().filter(|e| matches!(&e.requirement, orchestra::ExceptionRequirement::Check {cmd,..} if cmd == "quality:tests" || cmd == "sh fail.sh")).map(|e| e.requirement.clone()).collect();
    let request = wb.request_acceptance_exception(expected).unwrap();
    wb.accept_delivery_exception(
        &request.question_id,
        expected,
        &selected,
        "known test fixture",
    )
    .unwrap();
    let config = wb.quality_configuration().unwrap();
    let mut commands = config.stages[0].commands.clone();
    commands.insert(
        orchestra::QualityCategory::Accessibility,
        "sh other.sh".into(),
    );
    wb.update_quality_commands(0, &config.version, &commands)
        .unwrap();
    let current = wb.stage_evidence().unwrap().unwrap();
    assert!(current
        .exceptions
        .iter()
        .filter(|e| selected.contains(&e.requirement))
        .all(|e| e.accepted));
    assert_eq!(
        current
            .checks
            .iter()
            .find(|e| e.cmd == "quality:tests")
            .unwrap()
            .state,
        orchestra::CheckState::Failed
    );
    assert_eq!(
        current
            .checks
            .iter()
            .find(|e| e.cmd == "quality:accessibility")
            .unwrap()
            .state,
        orchestra::CheckState::Stale
    );
    let base = wb.pack().unwrap();
    let once = orchestra::current_process_pack(&wb.db, &wb.project_id, base).unwrap();
    let twice = orchestra::current_process_pack(&wb.db, &wb.project_id, &once).unwrap();
    assert_eq!(
        serde_json::to_value(once).unwrap(),
        serde_json::to_value(twice).unwrap()
    );
}

#[test]
fn new_quality_contract_on_intermediate_stage_is_executed_automatically() {
    let dir = tempfile::tempdir().unwrap();
    let pack: PackDef = serde_json::from_value(json!({"name":"quality fixture","version":1,"stages":[{"name":"build","roles":["dev"],"due":[],"stamp_point":false},{"name":"delivery","roles":["dev"],"due":[],"stamp_point":true}]})).unwrap();
    let wb = Workbench::for_test(dir.path(), &["dev"], Some(pack)).unwrap();
    wb.open_stage(0).unwrap();
    std::fs::write(dir.path().join("pass.sh"), "exit 0\n").unwrap();
    std::fs::write(dir.path().join("changed.rs"), "fn changed() {}\n").unwrap();
    let config = wb.quality_configuration().unwrap();
    let commands = std::collections::BTreeMap::from([(
        orchestra::QualityCategory::Tests,
        "sh pass.sh".into(),
    )]);
    wb.update_quality_commands(0, &config.version, &commands)
        .unwrap();
    orchestra::run_quality_checks(&wb.db, &wb.project_id, dir.path(), wb.pack().unwrap()).unwrap();
    let current = wb.stage_evidence().unwrap().unwrap();
    assert_eq!(
        current
            .checks
            .iter()
            .find(|c| c.cmd == "quality:tests")
            .unwrap()
            .state,
        orchestra::CheckState::Passed
    );
}

#[test]
fn failed_quality_commands_preserve_bounded_stderr_in_both_trace_records() {
    let (dir, wb) = fixture(&[("tests", "sh fail.sh")]);
    std::fs::write(
        dir.path().join("fail.sh"),
        "printf 'synthetic startup failure\\n' >&2\nhead -c 3000 /dev/zero | tr '\\000' x >&2\nexit 7\n",
    )
    .unwrap();
    std::fs::write(dir.path().join("changed.rs"), "fn changed() {}\n").unwrap();
    wb.run_checks().unwrap();
    let events = wb
        .db
        .events(&wb.project_id, Some(&[EventKind::TestRan]))
        .unwrap();
    for cmd in ["sh fail.sh", "quality:tests"] {
        let event = events
            .iter()
            .find(|event| event.payload["cmd"] == cmd)
            .unwrap();
        assert_eq!(event.payload["exit_code"], 7);
        assert_eq!(event.payload["stdout"], "");
        let stderr = event.payload["stderr"].as_str().unwrap();
        assert!(stderr.starts_with("synthetic startup failure\n"));
        assert_eq!(stderr.chars().count(), 2000);
    }
}

#[test]
fn changed_ui_requires_real_tests_accessibility_and_performance_evidence() {
    let (dir, wb) = fixture(&[]);
    std::fs::create_dir(dir.path().join("ui")).unwrap();
    std::fs::write(
        dir.path().join("ui/page.tsx"),
        "export const Page = () => <button>Run</button>",
    )
    .unwrap();
    let evidence = wb.stage_evidence().unwrap().unwrap();
    for key in [
        "quality:tests",
        "quality:accessibility",
        "quality:performance",
    ] {
        assert!(
            evidence
                .missing
                .iter()
                .any(|m| m == &format!("check:{key}")),
            "missing {key}: {evidence:?}"
        );
    }
    assert!(matches!(
        wb.advance().unwrap(),
        orchestra::StageAction::Incomplete { .. }
    ));
}

#[test]
fn declared_quality_runners_produce_real_evidence_and_retest_regressions() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("tests.sh"), "exit 0\n").unwrap();
    std::fs::write(dir.path().join("accessibility.sh"), "exit 0\n").unwrap();
    std::fs::write(
        dir.path().join("performance.sh"),
        "sleep \"$(cat duration)\"\n",
    )
    .unwrap();
    std::fs::write(dir.path().join("duration"), "0.01").unwrap();
    let pack: PackDef = serde_json::from_value(json!({"name":"quality fixture","version":1,"stages":[{"name":"delivery","roles":["dev"],"due":[],"checks":["sh tests.sh","sh accessibility.sh","sh performance.sh"],"quality_checks":{"tests":"sh tests.sh","accessibility":"sh accessibility.sh","performance":"sh performance.sh"},"stamp_point":true}]})).unwrap();
    let wb = Workbench::for_test(dir.path(), &["dev"], Some(pack)).unwrap();
    wb.open_stage(0).unwrap();
    let page = dir.path().join("page.tsx");
    std::fs::write(&page, "export const Page = () => <button>Run</button>").unwrap();
    wb.run_checks().unwrap();
    let evidence = wb.stage_evidence().unwrap().unwrap();
    let performance = evidence
        .checks
        .iter()
        .find(|c| c.cmd == "quality:performance")
        .unwrap();
    assert_eq!(
        performance.state,
        orchestra::CheckState::Passed,
        "{evidence:?}"
    );
    assert_eq!(
        performance.quality.as_ref().unwrap().reason,
        "baseline_established"
    );
    assert!(performance
        .quality
        .as_ref()
        .unwrap()
        .baseline_event_id
        .is_none());
    eprintln!("quality baseline evidence: {:?}", performance.quality);
    let baseline_event = performance.event_id;

    std::fs::write(&page, "export const Page = () => <button>Slow</button>").unwrap();
    std::fs::write(dir.path().join("duration"), "2.0").unwrap();
    wb.run_checks().unwrap();
    let evidence = wb.stage_evidence().unwrap().unwrap();
    let performance = evidence
        .checks
        .iter()
        .find(|c| c.cmd == "quality:performance")
        .unwrap();
    assert_eq!(
        performance.state,
        orchestra::CheckState::Failed,
        "{evidence:?}"
    );
    assert_eq!(performance.quality.as_ref().unwrap().reason, "regressed");
    eprintln!("quality regression evidence: {:?}", performance.quality);
    assert_eq!(
        performance.quality.as_ref().unwrap().baseline_event_id,
        baseline_event
    );
    assert!(evidence
        .missing
        .contains(&"check:quality:performance".into()));

    std::fs::write(&page, "export const Page = () => <button>Fixed</button>").unwrap();
    std::fs::write(dir.path().join("duration"), "0.001").unwrap();
    wb.run_checks().unwrap();
    let evidence = wb.stage_evidence().unwrap().unwrap();
    let performance = evidence
        .checks
        .iter()
        .find(|c| c.cmd == "quality:performance")
        .unwrap();
    assert_eq!(
        performance.state,
        orchestra::CheckState::Passed,
        "{evidence:?}"
    );
    assert_eq!(performance.quality.as_ref().unwrap().reason, "passed");
    eprintln!("quality repair evidence: {:?}", performance.quality);
    assert!(evidence.missing.is_empty(), "{evidence:?}");

    // A changed benchmark definition cannot inherit the old comparison pass.
    std::fs::write(
        dir.path().join("performance.sh"),
        "sleep \"$(cat duration)\"\n# changed workload definition\n",
    )
    .unwrap();
    wb.run_checks().unwrap();
    let changed = wb.stage_evidence().unwrap().unwrap();
    let performance = changed
        .checks
        .iter()
        .find(|c| c.cmd == "quality:performance")
        .unwrap();
    assert_eq!(performance.state, orchestra::CheckState::Failed);
    assert_eq!(
        performance.quality.as_ref().unwrap().reason,
        "measurement_changed"
    );
    let changed_id = performance.event_id.unwrap();
    let expected = changed.fingerprint.as_deref().unwrap();
    assert!(wb
        .confirm_performance_baseline(changed_id, expected, "")
        .is_err());
    assert!(wb
        .confirm_performance_baseline(changed_id, "stale", "new conditions")
        .is_err());
    let confirmed = wb
        .confirm_performance_baseline(changed_id, expected, "fixed benchmark conditions")
        .unwrap();
    assert!(confirmed.event_id > changed_id);
    // Q2: confirmation changes only the reference. The failed execution fact
    // remains failed until a new real three-sample measurement passes.
    assert_eq!(
        wb.stage_evidence()
            .unwrap()
            .unwrap()
            .checks
            .iter()
            .find(|c| c.cmd == "quality:performance")
            .unwrap()
            .state,
        orchestra::CheckState::Failed
    );
    wb.run_checks().unwrap();
    let current = wb.stage_evidence().unwrap().unwrap();
    let measured = current
        .checks
        .iter()
        .find(|c| c.cmd == "quality:performance")
        .unwrap();
    assert_eq!(measured.state, orchestra::CheckState::Passed, "{current:?}");
    assert_eq!(
        measured.quality.as_ref().unwrap().baseline_event_id,
        Some(changed_id)
    );
    assert!(wb
        .confirm_performance_baseline(changed_id, expected, "stale old measurement")
        .is_err());
}

#[test]
fn automatic_checkpoint_coalesces_unchanged_attempts_and_retests_changed_code() {
    let (dir, wb) = fixture(&[("tests", "sh tests.sh")]);
    std::fs::write(dir.path().join("tests.sh"), "exit 0\n").unwrap();
    let file = dir.path().join("math.rs");
    std::fs::write(&file, "fn sum() {}\n").unwrap();
    assert!(matches!(
        wb.advance().unwrap(),
        orchestra::StageAction::AwaitingStamp { .. }
    ));
    let first = wb.stage_evidence().unwrap().unwrap();
    let first_id = first
        .checks
        .iter()
        .find(|c| c.cmd == "quality:tests")
        .unwrap()
        .event_id;
    wb.advance().unwrap();
    let same = wb.stage_evidence().unwrap().unwrap();
    assert_eq!(
        first_id,
        same.checks
            .iter()
            .find(|c| c.cmd == "quality:tests")
            .unwrap()
            .event_id
    );
    std::fs::write(&file, "fn sum() { let _ = 1; }\n").unwrap();
    wb.advance().unwrap();
    let changed = wb.stage_evidence().unwrap().unwrap();
    assert_ne!(
        first_id,
        changed
            .checks
            .iter()
            .find(|c| c.cmd == "quality:tests")
            .unwrap()
            .event_id
    );
}

#[test]
fn missing_quality_evidence_needs_explicit_versioned_owner_exception() {
    let (dir, wb) = fixture(&[]);
    // Migration compatibility fixture: old active projects have no before-work inventory.
    wb.db
        .conn()
        .execute("DELETE FROM quality_baselines", [])
        .unwrap();
    std::fs::write(dir.path().join("logic.rs"), "fn work() {}\n").unwrap();
    let evidence = wb.stage_evidence().unwrap().unwrap();
    let fingerprint = evidence.fingerprint.unwrap();
    let selected: Vec<_> = evidence
        .exceptions
        .iter()
        .map(|c| c.requirement.clone())
        .collect();
    assert!(!selected.is_empty());
    let question = wb
        .request_acceptance_exception(&fingerprint)
        .unwrap()
        .question_id;
    assert!(wb
        .accept_delivery_exception(&question, &fingerprint, &selected, " ")
        .is_err());
    assert!(matches!(
        wb.advance().unwrap(),
        orchestra::StageAction::Incomplete { .. }
    ));
    wb.accept_delivery_exception(
        &question,
        &fingerprint,
        &selected,
        "Owner accepts missing runner for this prototype",
    )
    .unwrap();
    let accepted = wb.stage_evidence().unwrap().unwrap();
    assert!(accepted
        .checks
        .iter()
        .all(|c| c.state == orchestra::CheckState::Missing));
    assert!(accepted.missing.is_empty());
    assert!(matches!(
        wb.advance().unwrap(),
        orchestra::StageAction::AwaitingStamp { .. }
    ));
    std::fs::write(dir.path().join("logic.rs"), "fn work() { todo!() }\n").unwrap();
    assert!(!wb.stage_evidence().unwrap().unwrap().missing.is_empty());
}

proptest::proptest! {
    #![proptest_config(proptest::test_runner::Config { cases: 16, failure_persistence: Some(Box::new(proptest::test_runner::FileFailurePersistence::Direct("proptest-regressions/quality-gates.txt"))), ..Default::default() })]
    #[test]
    fn quality_scope_and_missing_evidence_cannot_be_overridden_by_agent_claims(index in 0usize..4, claim in "[a-zA-Z0-9 ]{0,120}") {
        let (dir, wb) = fixture(&[]);
        let (path, body) = [("security-notes.md", "Notes"), ("logic.rs", "fn work() {}"), ("page.tsx", "<button>Run</button>"), ("auth.rs", "fn check() {}")][index];
        std::fs::write(dir.path().join(path), body).unwrap();
        wb.db.append_event(&wb.project_id, EventKind::AgentMessage, json!({"all_checks_passed":true,"claim":claim}), None, None).unwrap();
        let evidence = wb.stage_evidence().unwrap().unwrap();
        let has = |key: &str| evidence.checks.iter().any(|c| c.cmd == key);
        proptest::prop_assert_eq!(has("quality:tests"), index != 0);
        proptest::prop_assert_eq!(has("quality:accessibility"), index == 2);
        proptest::prop_assert_eq!(has("quality:performance"), index == 2);
        proptest::prop_assert_eq!(has("quality:security"), index == 3);
        proptest::prop_assert!(evidence.checks.iter().all(|c| c.state == orchestra::CheckState::Missing));
        proptest::prop_assert_eq!(evidence.missing.is_empty(), index == 0);
    }
}

proptest::proptest! {
    #![proptest_config(proptest::test_runner::Config { cases: 4, failure_persistence: Some(Box::new(proptest::test_runner::FileFailurePersistence::Direct("proptest-regressions/quality-performance.txt"))), ..Default::default() })]
    #[test]
    fn unsuccessful_performance_runs_never_establish_a_passing_baseline(exit in 1u8..64) {
        let (dir, wb) = fixture(&[("performance", "sh performance.sh")]);
        std::fs::write(dir.path().join("page.tsx"), "<button>Run</button>").unwrap();
        std::fs::write(dir.path().join("performance.sh"), format!("exit {exit}\n")).unwrap();
        wb.run_checks().unwrap();
        let evidence = wb.stage_evidence().unwrap().unwrap();
        let performance = evidence.checks.iter().find(|c| c.cmd == "quality:performance").unwrap();
        proptest::prop_assert_eq!(&performance.state, &orchestra::CheckState::Failed);
        proptest::prop_assert_eq!(performance.exit_code, Some(i32::from(exit)));
        proptest::prop_assert_eq!(&performance.quality.as_ref().unwrap().reason, "execution_failed");
        proptest::prop_assert!(evidence.missing.contains(&"check:quality:performance".into()));
    }
}

proptest::proptest! {
    #![proptest_config(proptest::test_runner::Config::with_cases(8))]
    #[test]
    fn unrelated_command_words_never_declare_a_quality_contract(word in proptest::sample::select(vec!["tests", "performance", "accessibility", "security"])) {
        let dir = tempfile::tempdir().unwrap();
        let command = format!("printf '%s' {word}");
        let pack: PackDef = serde_json::from_value(json!({"name":"untrusted labels","version":1,"stages":[{"name":"delivery","roles":["dev"],"due":[],"checks":[command],"stamp_point":true}]})).unwrap();
        let wb = Workbench::for_test(dir.path(), &["dev"], Some(pack)).unwrap();
        wb.open_stage(0).unwrap();
        std::fs::write(dir.path().join("page.tsx"), "export const page = 1").unwrap();
        wb.run_checks().unwrap();
        let evidence = wb.stage_evidence().unwrap().unwrap();
        for category in ["tests", "performance", "accessibility"] {
            proptest::prop_assert!(evidence.missing.iter().any(|key| key == &format!("check:quality:{category}")), "{evidence:?}");
        }
    }
}

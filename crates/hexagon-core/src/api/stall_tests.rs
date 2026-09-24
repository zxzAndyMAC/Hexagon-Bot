//! 失速监视门面测试（stall-watch 票 01–04 / ADR 0074）。
//!
//! 时钟用假钟手拨，预算保持生产默认 60 秒——断言的是轨迹与门面结果，
//! 不断言日志字符串。

use super::*;
use crate::cards::{self, CardKind};
use crate::provider::{ChatRequest, ChatResponse, ProviderError, ScriptedProvider};
use crate::stallwatch::{Branch, StallClock};
use crate::turn::text_response;
use serde_json::Value;
use std::time::{Duration, Instant};

struct FakeClock(Mutex<Instant>);

impl FakeClock {
    fn new() -> Arc<Self> {
        Arc::new(Self(Mutex::new(Instant::now())))
    }
    fn advance(&self, d: Duration) {
        *self.0.lock().unwrap() += d;
    }
}

impl StallClock for FakeClock {
    fn now(&self) -> Instant {
        *self.0.lock().unwrap()
    }
}

const PAST: Duration = Duration::from_secs(61);

fn with_clock(wb: &mut Workbench) -> Arc<FakeClock> {
    let c = FakeClock::new();
    wb.stall_clock = c.clone();
    c
}

/// 流程包、无项目经理：说完没点名 → 接话人就是自己 → 链停，不另起回合。
fn pack_wb(dir: &Path, roles: &[&str]) -> Workbench {
    let pack: PackDef = serde_json::from_value(json!({
        "name":"t","version":1,
        "stages":[{"name":"规格","roles":["产品策划"],"due":["规格"],
                   "reviews":[{"artifact_kind":"规格","reviewer":"后端"}]}]
    }))
    .unwrap();
    let wb = Workbench::for_test(dir, roles, Some(pack)).unwrap();
    wb.open_stage(0).unwrap();
    wb
}

fn fastpath_wb(dir: &Path, roles: &[&str]) -> Workbench {
    let wb = Workbench::for_test(dir, roles, None).unwrap();
    wb.db
        .conn()
        .execute(
            "UPDATE projects SET mode='fastpath', fastpath_agent_id='a0' WHERE id='p1'",
            [],
        )
        .unwrap();
    wb
}

fn scripted(wb: &mut Workbench, slot: &str, lines: &[&str]) -> Arc<ScriptedProvider> {
    let p = Arc::new(ScriptedProvider::new(
        lines.iter().map(|l| text_response(l)).collect(),
    ));
    wb.register_provider(slot, p.clone());
    p
}

fn owner_says(wb: &Workbench, body: &str) -> UnnamedRoute {
    let (_id, cmd) = crate::commands::send_via_control(&wb.db, &wb.project_id, body, &[]).unwrap();
    assert!(cmd.is_none());
    wb.route_unnamed_owner(body, &[]).unwrap()
}

fn turns(wb: &Workbench, role: &str) -> usize {
    let id = wb.agent_by_role(role).unwrap();
    wb.db
        .timeline(&wb.project_id, None, 1000, Some(&[EventKind::TurnStarted]))
        .unwrap()
        .into_iter()
        .filter(|i| i.event.agent_id.as_deref() == Some(id.as_str()))
        .count()
}

fn owner_messages(wb: &Workbench) -> i64 {
    wb.db
        .conn()
        .query_row(
            "SELECT COUNT(*) FROM messages WHERE project_id='p1' AND author='owner'",
            [],
            |r| r.get(0),
        )
        .unwrap()
}

fn workbench_notes(wb: &Workbench) -> Vec<String> {
    let mut st = wb
        .db
        .conn()
        .prepare("SELECT body FROM messages WHERE project_id='p1' AND author=?1 ORDER BY id")
        .unwrap();
    st.query_map([crate::pm_route::WORKBENCH_AUTHOR], |r| r.get(0))
        .unwrap()
        .collect::<Result<_, _>>()
        .unwrap()
}

fn system_kinds(wb: &Workbench) -> Vec<String> {
    wb.db
        .timeline(&wb.project_id, None, 2000, Some(&[EventKind::System]))
        .unwrap()
        .into_iter()
        .filter_map(|i| i.event.payload["kind"].as_str().map(String::from))
        .collect()
}

fn stall_cards(wb: &Workbench) -> Vec<Value> {
    cards::queued(&wb.db, &wb.project_id)
        .unwrap()
        .into_iter()
        .filter(|c| c.kind == "stall")
        .map(|c| serde_json::to_value(c).unwrap())
        .collect()
}

fn stage_ptr(wb: &Workbench) -> Vec<(String, String)> {
    let mut st = wb
        .db
        .conn()
        .prepare("SELECT id, state FROM stage_runs WHERE project_id='p1' ORDER BY rowid")
        .unwrap();
    st.query_map([], |r| Ok((r.get(0)?, r.get(1)?)))
        .unwrap()
        .collect::<Result<_, _>>()
        .unwrap()
}

/// 三支都不拨阶段指针、不盖章、不远程发布（ADR 0074）。
fn assert_no_side_effects(wb: &Workbench, ptr: &[(String, String)]) {
    assert_eq!(stage_ptr(wb), ptr, "失速动作不拨阶段指针");
    let n = wb
        .db
        .timeline(
            &wb.project_id,
            None,
            2000,
            Some(&[EventKind::Stamped, EventKind::StageStarted]),
        )
        .unwrap()
        .into_iter()
        .filter(|i| i.event.kind == EventKind::Stamped)
        .count();
    assert_eq!(n, 0, "失速动作不盖章");
    assert_eq!(
        cards::count_queued(&wb.db, &wb.project_id, Some(CardKind::Publish)).unwrap(),
        0,
        "失速动作不发起远程发布"
    );
}

fn assert_subkinds_registered(wb: &Workbench) {
    for k in system_kinds(wb) {
        assert!(
            crate::trace::SYSTEM_SUBKINDS.contains(&k.as_str()),
            "词表外 System 子 kind: {k}"
        );
    }
}

fn last_request_text(p: &ScriptedProvider) -> String {
    let req = p.recorded().last().cloned().unwrap();
    req.messages
        .iter()
        .flat_map(|m| m.content.iter())
        .filter_map(|b| match b {
            crate::provider::ContentBlock::Text { text } => Some(text.clone()),
            _ => None,
        })
        .collect::<Vec<_>>()
        .join("\n")
}

// ---------- 票 01：无回复重触发 ----------

#[test]
fn no_reply_after_budget_retriggers_same_agent_once() {
    let dir = tempfile::tempdir().unwrap();
    let mut wb = pack_wb(dir.path(), &["产品策划", "后端"]);
    let clock = with_clock(&mut wb);
    let worker = scripted(&mut wb, "default", &["", ""]);
    let ptr = stage_ptr(&wb);
    wb.run_turn("产品策划", "写一版登录规格").unwrap();
    let owners = owner_messages(&wb);

    assert_eq!(wb.stall_tick().unwrap(), StallTick::Wait("clock"));
    clock.advance(Duration::from_secs(59));
    assert_eq!(wb.stall_tick().unwrap(), StallTick::Wait("clock"));
    clock.advance(Duration::from_secs(2));
    assert_eq!(
        wb.stall_tick().unwrap(),
        StallTick::Retriggered {
            agent_id: "a0".into()
        }
    );
    assert_eq!(turns(&wb, "产品策划"), 2, "新回合，同一个 Agent");
    assert_eq!(turns(&wb, "后端"), 0);
    assert_eq!(owner_messages(&wb), owners, "重触发不新落负责人消息");
    assert!(
        last_request_text(&worker).contains("写一版登录规格"),
        "重触发的简报带原指令"
    );
    assert!(system_kinds(&wb).contains(&"stall_retrigger".to_string()));
    assert_no_side_effects(&wb, &ptr);
    assert_subkinds_registered(&wb);
}

#[test]
fn clock_starts_after_the_turn_not_during_it() {
    // 回合本身跑了 10 分钟：落地那一刻才起算，回合时长不计入。
    struct Slow {
        clock: Arc<FakeClock>,
    }
    impl crate::provider::ModelProvider for Slow {
        fn complete(&self, _r: &ChatRequest) -> Result<ChatResponse, ProviderError> {
            self.clock.advance(Duration::from_secs(600));
            Ok(text_response(""))
        }
    }
    let dir = tempfile::tempdir().unwrap();
    let mut wb = pack_wb(dir.path(), &["产品策划"]);
    let clock = with_clock(&mut wb);
    wb.register_provider(
        "default",
        Arc::new(Slow {
            clock: clock.clone(),
        }),
    );
    wb.run_turn("产品策划", "写规格").unwrap();
    assert_eq!(wb.stall_tick().unwrap(), StallTick::Wait("clock"));
}

#[test]
fn turn_in_flight_does_not_trigger() {
    let dir = tempfile::tempdir().unwrap();
    let mut wb = pack_wb(dir.path(), &["产品策划"]);
    let clock = with_clock(&mut wb);
    scripted(&mut wb, "default", &[""]);
    wb.run_turn("产品策划", "写规格").unwrap();
    clock.advance(Duration::from_secs(600));
    let held = wb.hold_in_flight();
    assert_eq!(wb.stall_tick().unwrap(), StallTick::Wait("in_flight"));
    drop(held);
    assert_eq!(turns(&wb, "产品策划"), 1);
}

#[test]
fn network_wait_suspension_does_not_trigger() {
    // 等网探针在回合里（在飞）；预算耗尽挂起后 run 是 interrupted + 恢复卡——轮到负责人。
    let dir = tempfile::tempdir().unwrap();
    let mut wb = pack_wb(dir.path(), &["产品策划"]);
    let clock = with_clock(&mut wb);
    scripted(&mut wb, "default", &[""]);
    wb.run_turn("产品策划", "写规格").unwrap();
    let run = wb.active_run().unwrap().unwrap();
    orchestra::suspend_run(&wb.db, &wb.project_id, &run.id, "a0").unwrap();
    clock.advance(Duration::from_secs(600));
    assert_eq!(wb.stall_tick().unwrap(), StallTick::Wait("owner_waits"));
    assert_eq!(turns(&wb, "产品策划"), 1);
}

#[test]
fn pause_stops_the_clock_and_resume_restarts_it() {
    let dir = tempfile::tempdir().unwrap();
    let mut wb = pack_wb(dir.path(), &["产品策划"]);
    let clock = with_clock(&mut wb);
    scripted(&mut wb, "default", &["", ""]);
    wb.run_turn("产品策划", "写规格").unwrap();
    clock.advance(Duration::from_secs(30));
    orchestra::pause(&wb.db, &wb.project_id).unwrap();
    clock.advance(Duration::from_secs(600));
    assert_eq!(wb.stall_tick().unwrap(), StallTick::Wait("frozen"));
    orchestra::resume(&wb.db, &wb.project_id).unwrap();
    // 恢复后重新起算 60 秒——暂停前走过的 30 秒不带过来。
    assert_eq!(wb.stall_tick().unwrap(), StallTick::Wait("clock"));
    clock.advance(Duration::from_secs(45));
    assert_eq!(wb.stall_tick().unwrap(), StallTick::Wait("clock"));
    clock.advance(Duration::from_secs(16));
    assert!(matches!(
        wb.stall_tick().unwrap(),
        StallTick::Retriggered { .. }
    ));
}

#[test]
fn team_asleep_does_not_trigger() {
    let dir = tempfile::tempdir().unwrap();
    let mut wb = pack_wb(dir.path(), &["产品策划"]);
    let clock = with_clock(&mut wb);
    scripted(&mut wb, "default", &[""]);
    wb.run_turn("产品策划", "写规格").unwrap();
    orchestra::sleep_all(&wb.db, &wb.project_id).unwrap();
    clock.advance(Duration::from_secs(600));
    assert_eq!(wb.stall_tick().unwrap(), StallTick::Wait("frozen"));
    assert_eq!(turns(&wb, "产品策划"), 1);
}

#[test]
fn fast_path_no_reply_retriggers_too() {
    let dir = tempfile::tempdir().unwrap();
    let mut wb = fastpath_wb(dir.path(), &["后端", "产品策划"]);
    let clock = with_clock(&mut wb);
    // 方案调用 + 执行调用都空；重触发一轮也空。
    scripted(&mut wb, "default", &["", "", ""]);
    wb.dispatch("后端", "修登录 bug", &[]).unwrap();
    clock.advance(PAST);
    assert_eq!(
        wb.stall_tick().unwrap(),
        StallTick::Retriggered {
            agent_id: "a0".into()
        }
    );
    assert_eq!(turns(&wb, "后端"), 2);
    let n: i64 = wb
        .db
        .conn()
        .query_row("SELECT COUNT(*) FROM stage_runs", [], |r| r.get(0))
        .unwrap();
    assert_eq!(n, 0, "快速通道不占阶段");
}

#[test]
fn visible_reply_is_not_no_reply() {
    let dir = tempfile::tempdir().unwrap();
    let mut wb = pack_wb(dir.path(), &["产品策划", "后端"]);
    let clock = with_clock(&mut wb);
    scripted(&mut wb, "default", &["规格初稿在写"]);
    wb.run_turn("产品策划", "写规格").unwrap();
    clock.advance(PAST);
    // 有回复、进度没动、花名册无项目经理 → 空转入卡（票 03），不是重触发。
    let t = wb.stall_tick().unwrap();
    assert!(
        matches!(
            t,
            StallTick::Carded {
                branch: Branch::IdleSpin,
                ..
            }
        ),
        "{t:?}"
    );
    assert_eq!(turns(&wb, "产品策划"), 1);
}

// ---------- 票 02：无回复失速卡 ----------

fn silent_twice(dir: &Path, extra: &[&str]) -> (Workbench, Arc<FakeClock>, Arc<ScriptedProvider>) {
    let mut wb = pack_wb(dir, &["产品策划", "后端"]);
    let clock = with_clock(&mut wb);
    let mut lines = vec!["", ""];
    lines.extend_from_slice(extra);
    let worker = scripted(&mut wb, "default", &lines);
    wb.run_turn("产品策划", "写一版登录规格").unwrap();
    clock.advance(PAST);
    assert!(matches!(
        wb.stall_tick().unwrap(),
        StallTick::Retriggered { .. }
    ));
    (wb, clock, worker)
}

#[test]
fn still_no_reply_after_retrigger_cards_without_waking_pm() {
    let dir = tempfile::tempdir().unwrap();
    let (wb, clock, _w) = silent_twice(dir.path(), &[]);
    let ptr = stage_ptr(&wb);
    assert_eq!(wb.stall_tick().unwrap(), StallTick::Wait("clock"));
    clock.advance(PAST);
    let t = wb.stall_tick().unwrap();
    let StallTick::Carded {
        question_id,
        branch,
        retry,
    } = t
    else {
        panic!("{t:?}")
    };
    assert_eq!(branch, Branch::NoReply);
    assert!(retry, "第一张卡有再试一次");
    let card = cards::get(&wb.db, &question_id).unwrap();
    assert_eq!(card.kind, "stall");
    assert_eq!(card.agent_id.as_deref(), Some("a0"));
    assert_eq!(card.payload["branch"], "no_reply");
    assert_eq!(workbench_notes(&wb).len(), 1, "入卡写一条工作台注记");
    assert_eq!(turns(&wb, "产品策划"), 2, "不第三次自动重触发");
    // 卡在队：再拨多久都不出手。
    clock.advance(Duration::from_secs(3600));
    assert_eq!(wb.stall_tick().unwrap(), StallTick::Wait("carded"));
    assert_eq!(stall_cards(&wb).len(), 1);
    assert_no_side_effects(&wb, &ptr);
    assert_subkinds_registered(&wb);
}

#[test]
fn stall_card_is_not_released_by_autonomy() {
    let dir = tempfile::tempdir().unwrap();
    let (wb, clock, _w) = silent_twice(dir.path(), &[]);
    // 存储档拉到 L4：自治放行面不认 stall 卡，仍等负责人。
    wb.db
        .conn()
        .execute("UPDATE projects SET autonomy='L4' WHERE id='p1'", [])
        .unwrap();
    clock.advance(PAST);
    let StallTick::Carded { question_id, .. } = wb.stall_tick().unwrap() else {
        panic!()
    };
    clock.advance(Duration::from_secs(3600));
    wb.stall_tick().unwrap();
    let card = cards::get(&wb.db, &question_id).unwrap();
    assert_eq!(card.state, cards::CardState::Queued);
    assert!(card.answered_by.is_none());
}

#[test]
fn retry_is_one_round_then_only_acknowledge() {
    let dir = tempfile::tempdir().unwrap();
    let (wb, clock, worker) = silent_twice(dir.path(), &[""]);
    clock.advance(PAST);
    let StallTick::Carded { question_id, .. } = wb.stall_tick().unwrap() else {
        panic!()
    };
    let owners = owner_messages(&wb);
    assert_eq!(
        wb.stall_retry(&question_id).unwrap(),
        StallTick::Retriggered {
            agent_id: "a0".into()
        }
    );
    assert_eq!(turns(&wb, "产品策划"), 3, "再试一次只重复刚失败的那一动");
    assert_eq!(turns(&wb, "后端"), 0, "卡上不改派");
    assert_eq!(owner_messages(&wb), owners);
    assert!(last_request_text(&worker).contains("写一版登录规格"));
    clock.advance(PAST);
    let t = wb.stall_tick().unwrap();
    let StallTick::Carded {
        question_id: q2,
        retry,
        ..
    } = t
    else {
        panic!("{t:?}")
    };
    assert!(!retry, "同一段失速再现：下一张卡只留知道了");
    assert!(matches!(wb.stall_retry(&q2), Err(ApiError::BadInput(_))));
    assert_eq!(turns(&wb, "产品策划"), 3);
}

#[test]
fn acknowledge_closes_until_new_owner_message() {
    let dir = tempfile::tempdir().unwrap();
    let (mut wb, clock, _w) = silent_twice(dir.path(), &[]);
    clock.advance(PAST);
    let StallTick::Carded { question_id, .. } = wb.stall_tick().unwrap() else {
        panic!()
    };
    let notes = workbench_notes(&wb).len();
    wb.stall_ack(&question_id).unwrap();
    assert_eq!(workbench_notes(&wb).len(), notes + 1, "知道了写一条注记");
    assert!(stall_cards(&wb).is_empty());
    assert!(system_kinds(&wb).contains(&"stall_closed".to_string()));
    clock.advance(Duration::from_secs(3600));
    assert_eq!(wb.stall_tick().unwrap(), StallTick::Wait("closed"));
    assert_eq!(turns(&wb, "产品策划"), 2, "收场后不再叫醒");

    // 新的负责人消息 → 重新计时。无项目经理：交给当前阶段激活名单第一位。
    scripted(&mut wb, "default", &["", ""]);
    owner_says(&wb, "再看看登录规格");
    assert_eq!(wb.stall_tick().unwrap(), StallTick::Wait("clock"));
    clock.advance(PAST);
    assert!(matches!(
        wb.stall_tick().unwrap(),
        StallTick::Retriggered { .. }
    ));
    assert_subkinds_registered(&wb);
}

#[test]
fn acknowledge_closes_until_new_activation() {
    let dir = tempfile::tempdir().unwrap();
    let (mut wb, clock, _w) = silent_twice(dir.path(), &[]);
    clock.advance(PAST);
    let StallTick::Carded { question_id, .. } = wb.stall_tick().unwrap() else {
        panic!()
    };
    wb.stall_ack(&question_id).unwrap();
    assert_eq!(wb.stall_tick().unwrap(), StallTick::Wait("closed"));
    // 退回重开 = 新的激活。
    wb.db
        .conn()
        .execute(
            "UPDATE stage_runs SET state='done' WHERE project_id='p1'",
            [],
        )
        .unwrap();
    wb.open_stage(0).unwrap();
    scripted(&mut wb, "default", &["", ""]);
    wb.run_turn("产品策划", "重写规格").unwrap();
    clock.advance(PAST);
    assert!(matches!(
        wb.stall_tick().unwrap(),
        StallTick::Retriggered { .. }
    ));
}

#[test]
fn acknowledge_then_owner_wake_without_a_turn_does_not_fire() {
    // 收场后负责人只唤醒、不跑回合：旧的无回复不能立刻重触发（票 02 重新计时）。
    let dir = tempfile::tempdir().unwrap();
    let (wb, clock, _w) = silent_twice(dir.path(), &[]);
    clock.advance(PAST);
    let StallTick::Carded { question_id, .. } = wb.stall_tick().unwrap() else {
        panic!()
    };
    wb.stall_ack(&question_id).unwrap();
    orchestra::set_agent_sleeping(&wb.db, &wb.project_id, "a0", true).unwrap();
    orchestra::set_agent_sleeping(&wb.db, &wb.project_id, "a0", false).unwrap();
    clock.advance(PAST);
    assert_eq!(wb.stall_tick().unwrap(), StallTick::Wait("no_activity"));
    assert_eq!(turns(&wb, "产品策划"), 2);
}

#[test]
fn acknowledge_then_dispatch_reopens_the_clock() {
    // 收场后 dispatch 是新的激活：监视重新开，空回复再等满预算才重触发。
    let dir = tempfile::tempdir().unwrap();
    let (mut wb, clock, _w) = silent_twice(dir.path(), &[]);
    clock.advance(PAST);
    let StallTick::Carded { question_id, .. } = wb.stall_tick().unwrap() else {
        panic!()
    };
    wb.stall_ack(&question_id).unwrap();
    scripted(&mut wb, "default", &["", ""]);
    wb.dispatch("后端", "修登录", &[]).unwrap();
    assert_eq!(
        wb.stall_tick().unwrap(),
        StallTick::Wait("clock"),
        "dispatch 是新的激活，收场后重新开钟"
    );
    clock.advance(PAST);
    assert!(
        matches!(wb.stall_tick().unwrap(), StallTick::Retriggered { .. }),
        "空回复等满预算才重触发，不再永远 closed"
    );
}

// ---------- 票 03：空转调查 ----------

/// 项目经理 + 两个角色。决策槽按脚本回放；主对话槽给被派到的角色。
fn pm_fixture(
    dir: &Path,
    decision_lines: &[&str],
    worker_lines: &[&str],
) -> (
    Workbench,
    Arc<FakeClock>,
    Arc<ScriptedProvider>,
    Arc<ScriptedProvider>,
) {
    let pack: PackDef = serde_json::from_value(json!({
        "name":"t","version":1,
        "stages":[{"name":"规格","roles":["产品策划"],"due":["规格"]}]
    }))
    .unwrap();
    let mut wb = Workbench::for_test(dir, &["项目经理", "产品策划", "后端"], Some(pack)).unwrap();
    wb.open_stage(0).unwrap();
    let clock = with_clock(&mut wb);
    let decision = scripted(&mut wb, "decision", decision_lines);
    let worker = scripted(&mut wb, "default", worker_lines);
    wb.set_decision_slot("项目经理", Some("decision")).unwrap();
    (wb, clock, decision, worker)
}

#[test]
fn idle_spin_wakes_pm_and_investigation_hold_closes() {
    let dir = tempfile::tempdir().unwrap();
    // 负责人的话 → 派给后端；后端说完 → 先不派活（正常派活的第一次先不派活）；
    // 60 秒没进度 → 调查 → 项目经理再次先不派活 → 收场。
    let (wb, clock, decision, worker) = pm_fixture(
        dir.path(),
        &["后端", "先不派活", "先不派活"],
        &["方案：先看过期判断", "看过了，没问题"],
    );
    let ptr = stage_ptr(&wb);
    owner_says(&wb, "查一下登录过期");
    assert_eq!(turns(&wb, "后端"), 1);
    assert_eq!(decision.recorded().len(), 2);
    assert_eq!(wb.stall_tick().unwrap(), StallTick::Wait("clock"));
    clock.advance(PAST);
    assert_eq!(wb.stall_tick().unwrap(), StallTick::Closed);
    assert_eq!(decision.recorded().len(), 3, "唤醒项目经理做一次封闭选择");
    let req = decision.recorded().last().cloned().unwrap();
    assert!(req.tools.is_empty(), "调查是封闭选择，不是聊天回合");
    assert!(last_request_text(&decision).contains(crate::stallwatch::INVESTIGATION_SPEAKER));
    assert_eq!(turns(&wb, "后端"), 1, "空转不重触发同一个人");
    assert_eq!(turns(&wb, "项目经理"), 0);
    assert_eq!(worker.recorded().len(), 2);
    assert!(stall_cards(&wb).is_empty(), "调查里的先不派活不入卡");
    assert_eq!(workbench_notes(&wb).len(), 1, "收场写一条工作台注记");
    let kinds = system_kinds(&wb);
    assert!(kinds.contains(&"stall_investigation".to_string()));
    assert!(kinds.contains(&"stall_closed".to_string()));
    clock.advance(Duration::from_secs(3600));
    assert_eq!(wb.stall_tick().unwrap(), StallTick::Wait("closed"));
    assert_no_side_effects(&wb, &ptr);
    assert_subkinds_registered(&wb);
}

#[test]
fn investigation_dispatch_uses_pm_routing_then_cards_if_still_idle() {
    let dir = tempfile::tempdir().unwrap();
    // 调查派给产品策划；产品策划说完没进度 → 正常先不派活 → 再空转入卡（不把空转再跑一遍）。
    let (wb, clock, decision, _worker) = pm_fixture(
        dir.path(),
        &["后端", "先不派活", "产品策划", "先不派活"],
        &["方案", "看过了", "方案二", "我也看了"],
    );
    owner_says(&wb, "查一下登录过期");
    clock.advance(PAST);
    assert_eq!(
        wb.stall_tick().unwrap(),
        StallTick::Investigated {
            role: "产品策划".into()
        }
    );
    assert_eq!(turns(&wb, "产品策划"), 1, "派活沿用项目经理既有路由");
    assert_eq!(decision.recorded().len(), 4);
    clock.advance(PAST);
    let t = wb.stall_tick().unwrap();
    assert!(
        matches!(
            t,
            StallTick::Carded {
                branch: Branch::IdleSpin,
                retry: true,
                ..
            }
        ),
        "{t:?}"
    );
    assert_eq!(decision.recorded().len(), 4, "同一段失速不自动再调查");
}

#[test]
fn without_pm_idle_spin_cards_without_retry() {
    let dir = tempfile::tempdir().unwrap();
    let mut wb = pack_wb(dir.path(), &["产品策划", "后端"]);
    let clock = with_clock(&mut wb);
    scripted(&mut wb, "default", &["规格我在想"]);
    let ptr = stage_ptr(&wb);
    wb.run_turn("产品策划", "写规格").unwrap();
    clock.advance(PAST);
    let t = wb.stall_tick().unwrap();
    let StallTick::Carded {
        question_id,
        branch,
        retry,
    } = t
    else {
        panic!("{t:?}")
    };
    assert_eq!(branch, Branch::IdleSpin);
    assert!(!retry, "没有项目经理：卡上不出现再试一次");
    let card = cards::get(&wb.db, &question_id).unwrap();
    assert_eq!(card.payload["retry"], false);
    assert!(card.agent_id.is_none());
    assert_eq!(workbench_notes(&wb).len(), 1);
    assert!(matches!(
        wb.stall_retry(&question_id),
        Err(ApiError::BadInput(_))
    ));
    assert_eq!(turns(&wb, "产品策划"), 1);
    assert_no_side_effects(&wb, &ptr);
}

#[test]
fn review_rework_is_not_idle_spin() {
    let dir = tempfile::tempdir().unwrap();
    let mut wb = pack_wb(dir.path(), &["产品策划", "后端"]);
    let clock = with_clock(&mut wb);
    scripted(&mut wb, "default", &["收到驳回，在改"]);
    let run = wb.active_run().unwrap().unwrap();
    wb.db
        .append_event(
            &wb.project_id,
            EventKind::ReviewRejected,
            json!({"artifact_kind":"规格","reviewer":"后端"}),
            Some("a1"),
            Some(&run.id),
        )
        .unwrap();
    wb.run_turn("产品策划", "按复审意见改规格").unwrap();
    clock.advance(Duration::from_secs(3600));
    assert_eq!(wb.stall_tick().unwrap(), StallTick::Wait("review_rework"));
    assert!(stall_cards(&wb).is_empty());
}

#[test]
fn owner_facing_card_is_not_idle_spin() {
    let dir = tempfile::tempdir().unwrap();
    let mut wb = pack_wb(dir.path(), &["产品策划", "后端"]);
    let clock = with_clock(&mut wb);
    scripted(&mut wb, "default", &["要装个依赖"]);
    wb.run_turn("产品策划", "写规格").unwrap();
    wb.request_publish("origin").unwrap();
    clock.advance(Duration::from_secs(3600));
    assert_eq!(wb.stall_tick().unwrap(), StallTick::Wait("owner_waits"));
}

#[test]
fn fast_path_idle_spin_with_no_stage_pointer() {
    let dir = tempfile::tempdir().unwrap();
    let mut wb = fastpath_wb(dir.path(), &["后端", "产品策划", "项目经理"]);
    let clock = with_clock(&mut wb);
    let decision = scripted(&mut wb, "decision", &["先不派活", "先不派活"]);
    wb.set_decision_slot("项目经理", Some("decision")).unwrap();
    // 负责人的话 → 先不派活（第一次仍是空转信号）→ 60 秒 → 调查 → 再先不派活 → 收场。
    assert!(matches!(
        owner_says(&wb, "有人吗"),
        UnnamedRoute::Held { .. }
    ));
    clock.advance(PAST);
    assert_eq!(wb.stall_tick().unwrap(), StallTick::Closed);
    assert_eq!(decision.recorded().len(), 2);
}

// ---------- 票 04：调查超时 ----------

#[test]
fn investigation_without_closed_choice_cards_immediately() {
    let dir = tempfile::tempdir().unwrap();
    let (wb, clock, decision, _w) = pm_fixture(dir.path(), &["先不派活", "架构师"], &[]);
    owner_says(&wb, "有人吗");
    clock.advance(PAST);
    let t = wb.stall_tick().unwrap();
    let StallTick::Carded {
        question_id,
        branch,
        retry,
    } = t
    else {
        panic!("{t:?}")
    };
    assert_eq!(branch, Branch::InvestigationTimeout, "同一拍立刻入卡");
    assert!(retry);
    let card = cards::get(&wb.db, &question_id).unwrap();
    assert_eq!(card.agent_id.as_deref(), Some("a0"), "调查超时卡归项目经理");
    assert_eq!(workbench_notes(&wb).len(), 1);
    // 不自动重试调查。
    clock.advance(Duration::from_secs(3600));
    assert_eq!(wb.stall_tick().unwrap(), StallTick::Wait("carded"));
    assert_eq!(decision.recorded().len(), 2);
    assert_subkinds_registered(&wb);
}

#[test]
fn investigation_that_never_opens_cards_after_budget() {
    let dir = tempfile::tempdir().unwrap();
    let (wb, clock, decision, _w) = pm_fixture(dir.path(), &["先不派活"], &[]);
    owner_says(&wb, "有人吗");
    // 决策槽指向没注册的槽：调查连回合都开不了。
    wb.set_decision_slot("项目经理", Some("gone")).unwrap();
    clock.advance(PAST);
    assert_eq!(wb.stall_tick().unwrap(), StallTick::InvestigationPending);
    clock.advance(Duration::from_secs(30));
    assert_eq!(
        wb.stall_tick().unwrap(),
        StallTick::Wait("investigation_pending")
    );
    clock.advance(Duration::from_secs(31));
    let t = wb.stall_tick().unwrap();
    assert!(
        matches!(
            t,
            StallTick::Carded {
                branch: Branch::InvestigationTimeout,
                retry: true,
                ..
            }
        ),
        "{t:?}"
    );
    assert_eq!(decision.recorded().len(), 1, "没有自动重试调查");
}

#[test]
fn investigation_retry_wakes_pm_once_then_only_acknowledge() {
    let dir = tempfile::tempdir().unwrap();
    let (wb, clock, decision, _w) = pm_fixture(dir.path(), &["先不派活", "架构师", "设计师"], &[]);
    let ptr = stage_ptr(&wb);
    owner_says(&wb, "有人吗");
    clock.advance(PAST);
    let StallTick::Carded { question_id, .. } = wb.stall_tick().unwrap() else {
        panic!()
    };
    let t = wb.stall_retry(&question_id).unwrap();
    let StallTick::Carded {
        question_id: q2,
        branch,
        retry,
    } = t
    else {
        panic!("{t:?}")
    };
    assert_eq!(decision.recorded().len(), 3, "再试一次只再唤醒项目经理一轮");
    assert_eq!(branch, Branch::InvestigationTimeout);
    assert!(!retry, "下一张只留知道了");
    assert!(matches!(wb.stall_retry(&q2), Err(ApiError::BadInput(_))));
    wb.stall_ack(&q2).unwrap();
    assert_eq!(turns(&wb, "后端") + turns(&wb, "产品策划"), 0, "卡上不改派");
    assert_no_side_effects(&wb, &ptr);
}

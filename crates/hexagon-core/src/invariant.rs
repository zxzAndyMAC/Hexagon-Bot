//! 不变量伴随件（rsi-research 票 07）：独立路径校验 trace 完整性，
//! 让「轨迹可信」从纪律变成可证。三类检查：
//!   envelope_self_consistent — 信封载荷重算指纹 ≠ 落盘指纹（信封被改/写坏）
//!   dispatch_pairing         — 有用量账无信封（派发绕过了信封落盘点）
//!   dangling_turn            — turn_started 无收尾（崩溃痕迹未被收编）
//!   decision_shape           — decision 载荷缺 kind/chosen/eligible
//! 检查只读不改；违规落 System{kind:"invariant_violation"} 事件留痕。
//! 代价模型：误报 = 一条告警事件；漏报 = 坏轨迹冒充证据——实现偏向多报。

use crate::db::Db;
use crate::trace::EventKind;
use serde_json::{json, Value};

/// 只跑检不动库：返回违规明细。
pub fn check(db: &Db, project_id: &str) -> Result<Vec<Value>, crate::trace::TraceError> {
    let mut violations = Vec::new();

    // 1. 信封自洽：用落盘的 layers/messages/tools/params 重算指纹,
    //    与 payload.fingerprint 比对——配方必须与 turn.rs 同一个。
    let mut st = db.conn().prepare(
        "SELECT id, agent_id, stage_run_id, payload FROM events
         WHERE project_id=?1 AND kind='system'
           AND json_extract(payload,'$.kind')='request_envelope'
         ORDER BY id",
    )?;
    let envelopes: Vec<(i64, Option<String>, Option<String>, Value)> = st
        .query_map([project_id], |r| {
            Ok((
                r.get(0)?,
                r.get(1)?,
                r.get(2)?,
                serde_json::from_str::<Value>(&r.get::<_, String>(3)?).unwrap_or_default(),
            ))
        })?
        .collect::<Result<_, _>>()?;
    for (eid, agent, run, p) in &envelopes {
        let empty = vec![];
        let layers = p["layers"].as_array().unwrap_or(&empty);
        let msgs = p["messages"].as_array().unwrap_or(&empty);
        let tools_vec: Vec<&str> = p["tools"]
            .as_array()
            .map(|a| a.iter().filter_map(|t| t.as_str()).collect())
            .unwrap_or_default();
        let expected =
            crate::turn::prompt::envelope_fingerprint(layers, msgs, &tools_vec, &p["params"]);
        if expected != p["fingerprint"].as_str().unwrap_or("") {
            violations.push(json!({
                "check": "envelope_self_consistent",
                "event_id": eid, "agent_id": agent, "stage_run_id": run,
                "expected": expected, "actual": p["fingerprint"],
            }));
        }
    }

    // 2. 派发配对：每个 agent 的用量行数（每次派发一账）≤ 信封数。
    //    信封少 = 有派发绕过了落盘点——这条防的是「悄悄发包」。
    let mut st = db.conn().prepare(
        "SELECT u.agent_id, COUNT(*) FROM usage u
         JOIN agents a ON a.id=u.agent_id WHERE a.project_id=?1
         GROUP BY u.agent_id",
    )?;
    let usage_counts: Vec<(String, i64)> = st
        .query_map([project_id], |r| Ok((r.get(0)?, r.get(1)?)))?
        .collect::<Result<_, _>>()?;
    for (agent, calls) in usage_counts {
        let envs = envelopes
            .iter()
            .filter(|(_, a, _, _)| a.as_deref() == Some(agent.as_str()))
            .count() as i64;
        if envs < calls {
            violations.push(json!({
                "check": "dispatch_pairing",
                "agent_id": agent,
                "usage_rows": calls, "envelopes": envs,
            }));
        }
    }

    // 3. 悬挂回合：turn_started 后无 turn_finished/turn_failed。
    //    与 detect_interrupted 同判据,但本件只报告不收编——收编是
    //    恢复闸的事,伴随件的职责是「说」。
    let mut st = db.conn().prepare(
        "SELECT stage_run_id, agent_id FROM events e
         WHERE e.project_id=?1 AND e.kind='turn_started'
           AND e.id = (
             SELECT MAX(id) FROM events
             WHERE project_id=e.project_id
               AND stage_run_id IS e.stage_run_id
               AND agent_id IS e.agent_id
               AND kind IN ('turn_started','turn_finished','turn_failed'))",
    )?;
    let dangling: Vec<(Option<String>, Option<String>)> = st
        .query_map([project_id], |r| Ok((r.get(0)?, r.get(1)?)))?
        .collect::<Result<_, _>>()?;
    for (run, agent) in dangling {
        violations.push(json!({
            "check": "dangling_turn", "stage_run_id": run, "agent_id": agent,
        }));
    }

    // 4. 决策载荷形状：带 decision 的事件必须有 kind/chosen/eligible。
    let mut st = db.conn().prepare(
        "SELECT id, payload FROM events
         WHERE project_id=?1 AND json_extract(payload,'$.decision') IS NOT NULL",
    )?;
    let rows: Vec<(i64, String)> = st
        .query_map([project_id], |r| Ok((r.get(0)?, r.get(1)?)))?
        .collect::<Result<_, _>>()?;
    for (eid, raw) in rows {
        let p: Value = serde_json::from_str(&raw).unwrap_or_default();
        let d = &p["decision"];
        // chosen 两种合法形状：单选字符串（flag_route/adjudication）或
        // 字符串数组（roster 决策一次选多个角色）——两种都在词典里有名。
        let chosen_ok = d["chosen"].is_string()
            || d["chosen"]
                .as_array()
                .map(|a| a.iter().all(|x| x.is_string()))
                .unwrap_or(false);
        if !(d["kind"].is_string() && chosen_ok && d["eligible"].is_array()) {
            violations.push(json!({
                "check": "decision_shape", "event_id": eid, "decision": d,
            }));
        }
    }

    Ok(violations)
}

/// 检查 + 落痕：每条违规一个 System{kind:"invariant_violation"} 事件。
/// 返回违规数（0 = trace 自洽）。
pub fn check_and_log(db: &Db, project_id: &str) -> Result<usize, crate::trace::TraceError> {
    let vs = check(db, project_id)?;
    for v in &vs {
        db.append_event(
            project_id,
            EventKind::System,
            json!({
                "kind": "invariant_violation",
                "check": v["check"], "detail": v,
            }),
            v["agent_id"].as_str(),
            v["stage_run_id"].as_str(),
        )?;
    }
    Ok(vs.len())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::provider::ScriptedProvider;
    use crate::tools::{Registry, ToolContext};
    use crate::turn::{run_turn, text_response};

    fn setup() -> (Db, Registry, ToolContext, tempfile::TempDir) {
        let db = Db::open_in_memory().unwrap();
        db.conn()
            .execute(
                "INSERT INTO projects (id, dir, name, mode) VALUES ('p1','/tmp/x','x','pack')",
                [],
            )
            .unwrap();
        db.conn()
            .execute(
                "INSERT INTO agents (id, project_id, role, status) VALUES ('a1','p1','后端','active')",
                [],
            )
            .unwrap();
        let dir = tempfile::tempdir().unwrap();
        (
            db,
            Registry::builtin(),
            ToolContext {
                project_id: "p1".into(),
                agent_id: "a1".into(),
                repo_root: dir.path().to_path_buf(),
                stage_run_id: None,
                owned_globs: vec![],
                tiers: crate::artifacts::TierMap::new(),
            },
            dir,
        )
    }

    #[test]
    fn clean_turn_passes_check() {
        let (db, reg, ctx, _dir) = setup();
        let provider = ScriptedProvider::new(vec![text_response("done")]);
        run_turn(&db, &provider, &reg, &ctx, vec![], "干活").unwrap();
        assert!(check(&db, "p1").unwrap().is_empty());
        assert_eq!(check_and_log(&db, "p1").unwrap(), 0);
    }

    #[test]
    fn tampered_envelope_flags_violation() {
        let (db, reg, ctx, _dir) = setup();
        let provider = ScriptedProvider::new(vec![text_response("done")]);
        run_turn(&db, &provider, &reg, &ctx, vec![], "干活").unwrap();
        // 篡改信封载荷：改一个 layer 的 hash——指纹立刻对不上
        db.conn()
            .execute(
                "UPDATE events SET payload = json_set(payload,'$.layers[0].sha','0000')
                 WHERE json_extract(payload,'$.kind')='request_envelope'",
                [],
            )
            .unwrap();
        let vs = check(&db, "p1").unwrap();
        assert!(vs.iter().any(|v| v["check"] == "envelope_self_consistent"));
        assert_eq!(check_and_log(&db, "p1").unwrap(), vs.len());
        let n: i64 = db
            .conn()
            .query_row(
                "SELECT COUNT(*) FROM events
                 WHERE json_extract(payload,'$.kind')='invariant_violation'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(n, vs.len() as i64);
    }

    #[test]
    fn missing_envelope_flags_dispatch_pairing() {
        let (db, reg, ctx, _dir) = setup();
        let provider = ScriptedProvider::new(vec![text_response("done")]);
        run_turn(&db, &provider, &reg, &ctx, vec![], "干活").unwrap();
        // 删掉信封——用量账还在,配对立刻失衡
        db.conn()
            .execute(
                "DELETE FROM events
                 WHERE json_extract(payload,'$.kind')='request_envelope'",
                [],
            )
            .unwrap();
        let vs = check(&db, "p1").unwrap();
        assert!(vs.iter().any(|v| v["check"] == "dispatch_pairing"));
    }

    #[test]
    fn dangling_turn_reported_not_repaired() {
        let (db, _reg, ctx, _dir) = setup();
        db.append_event(
            "p1",
            EventKind::TurnStarted,
            json!({"agent": "a1"}),
            Some(&ctx.agent_id),
            None,
        )
        .unwrap();
        let vs = check(&db, "p1").unwrap();
        assert!(vs.iter().any(|v| v["check"] == "dangling_turn"));
        // 只报告不修复——turn_failed 没被补写
        let n: i64 = db
            .conn()
            .query_row(
                "SELECT COUNT(*) FROM events WHERE kind='turn_failed'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(n, 0);
    }

    #[test]
    fn roster_decision_with_array_chosen_is_clean() {
        // 回归(review 发现)：roster 决策 chosen 是数组,旧检查按字符串
        // 校验 → 每次 open_stage 误报 invariant_violation。两种形状都合法。
        let (db, _reg, ctx, _dir) = setup();
        db.append_event(
            "p1",
            EventKind::StageStarted,
            json!({"stage": "做", "decision": {
                "kind": "roster",
                "eligible": ["后端", "前端"],
                "chosen": ["后端"],
                "alternatives": [{"option": "前端", "reason": "not_chosen"}],
            }}),
            Some(&ctx.agent_id),
            None,
        )
        .unwrap();
        let vs = check(&db, "p1").unwrap();
        assert!(
            !vs.iter().any(|v| v["check"] == "decision_shape"),
            "roster 数组 chosen 不该报违规: {vs:?}"
        );
        // 缺 eligible 的坏载荷仍然抓得到
        db.append_event(
            "p1",
            EventKind::System,
            json!({"decision": {"kind": "x", "chosen": "y"}}),
            None,
            None,
        )
        .unwrap();
        let vs = check(&db, "p1").unwrap();
        assert!(vs.iter().any(|v| v["check"] == "decision_shape"));
    }

    // ---------- 生成式事件流（arch 票 08）----------
    //
    // 误报 = 一条告警事件；漏报 = 坏轨迹冒充证据——实现偏向多报，所以
    // 属性测试钉两头：良构流必须零误报，注入违规必须报对检查名。

    mod prop_tests {
        use super::tests::setup;
        use super::*;
        use crate::provider::{ChatRequest, ContentBlock, Message, Role};
        use proptest::collection;
        use proptest::prelude::*;

        /// 良构信封：走与生产同一构造函数——指纹必然自洽。
        fn good_envelope(call: usize) -> Value {
            let req = ChatRequest {
                model_slot: "default".into(),
                messages: vec![Message {
                    role: Role::User,
                    content: vec![ContentBlock::Text { text: "hi".into() }],
                }],
                tools: vec![],
            };
            crate::turn::prompt::request_envelope(call, &req, &req.messages, &[])
        }

        /// 良构事件流构件：信封/配对回合/合法决策/无关事件。
        /// 用量账按「每派发一账」约束——条数 ≤ 该 agent 的信封数。
        #[derive(Debug, Clone)]
        enum Op {
            Envelope,
            TurnPair,
            Decision(bool), // true: chosen 为数组（roster 形）
            Misc(usize),
        }

        fn op() -> impl Strategy<Value = Op> {
            prop_oneof![
                3 => Just(Op::Envelope),
                2 => Just(Op::TurnPair),
                2 => any::<bool>().prop_map(Op::Decision),
                3 => (0..5usize).prop_map(Op::Misc),
            ]
        }

        /// 按 ops 落事件流；返回 (信封数,)。
        fn emit(db: &Db, ops: &[Op]) -> usize {
            let misc_kinds = [
                EventKind::AgentActivated,
                EventKind::FlagSubmitted,
                EventKind::TestRan,
                EventKind::ArtifactDelivered,
                EventKind::Stamped,
            ];
            let mut envs = 0usize;
            for op in ops {
                match op {
                    Op::Envelope => {
                        db.append_event(
                            "p1",
                            EventKind::System,
                            good_envelope(envs),
                            Some("a1"),
                            None,
                        )
                        .unwrap();
                        envs += 1;
                    }
                    Op::TurnPair => {
                        for k in [EventKind::TurnStarted, EventKind::TurnFinished] {
                            db.append_event("p1", k, json!({}), Some("a1"), None)
                                .unwrap();
                        }
                    }
                    Op::Decision(array) => {
                        let chosen = if *array {
                            json!(["后端"])
                        } else {
                            json!("后端")
                        };
                        db.append_event(
                            "p1",
                            EventKind::StageStarted,
                            json!({"decision": {"kind": "roster", "chosen": chosen,
                                   "eligible": ["后端", "前端"]}}),
                            None,
                            None,
                        )
                        .unwrap();
                    }
                    Op::Misc(i) => {
                        db.append_event(
                            "p1",
                            misc_kinds[i % misc_kinds.len()],
                            json!({"i": i}),
                            None,
                            None,
                        )
                        .unwrap();
                    }
                }
            }
            envs
        }

        proptest! {
            /// 良构流零误报：任意组合的合法信封/配对回合/合法决策/
            /// 无关事件/配对用量 → check 恒为空。
            #[test]
            fn clean_streams_never_flag(ops in collection::vec(op(), 0..14)) {
                let (db, _reg, _ctx, _dir) = setup();
                let envs = emit(&db, &ops);
                // 用量账合法：条数 ≤ 信封数
                for _ in 0..envs {
                    db.conn()
                        .execute(
                            "INSERT INTO usage (project_id, agent_id, model)
                             VALUES ('p1','a1','default')",
                            [],
                        )
                        .unwrap();
                }
                let vs = check(&db, "p1").unwrap();
                prop_assert!(vs.is_empty(), "良构流误报: {vs:?}");
            }

            /// 注入必报：每类破坏落进对应检查名（不错报别家）。
            #[test]
            fn injected_violations_always_caught(
                ops in collection::vec(op(), 0..10),
                inject in 0..4usize,
            ) {
                let (db, _reg, _ctx, _dir) = setup();
                emit(&db, &ops);
                let expect = match inject {
                    // 信封被改：指纹重算对不上
                    0 => {
                        db.append_event(
                            "p1", EventKind::System, good_envelope(99), Some("a1"), None,
                        )
                        .unwrap();
                        db.conn()
                            .execute(
                                "UPDATE events SET payload =
                                 json_set(payload,'$.fingerprint','deadbeef')
                                 WHERE id = (SELECT MAX(id) FROM events
                                     WHERE json_extract(payload,'$.kind')='request_envelope')",
                                [],
                            )
                            .unwrap();
                        "envelope_self_consistent"
                    }
                    // 悬挂回合：started 为最新事件且无收尾
                    1 => {
                        db.append_event(
                            "p1", EventKind::TurnStarted, json!({}), Some("a1"), None,
                        )
                        .unwrap();
                        "dangling_turn"
                    }
                    // 决策缺 eligible
                    2 => {
                        db.append_event(
                            "p1",
                            EventKind::StageStarted,
                            json!({"decision": {"kind": "roster", "chosen": "后端"}}),
                            None,
                            None,
                        )
                        .unwrap();
                        "decision_shape"
                    }
                    // 用量无信封：派发绕过落盘点
                    _ => {
                        db.conn()
                            .execute(
                                "INSERT INTO agents (id, project_id, role, status)
                                 VALUES ('a2','p1','前端','active')",
                                [],
                            )
                            .unwrap();
                        db.conn()
                            .execute(
                                "INSERT INTO usage (project_id, agent_id, model)
                                 VALUES ('p1','a2','default')",
                                [],
                            )
                            .unwrap();
                        "dispatch_pairing"
                    }
                };
                let vs = check(&db, "p1").unwrap();
                prop_assert!(
                    vs.iter().any(|v| v["check"] == expect),
                    "注入 {expect} 未报: {vs:?}"
                );
            }
        }
    }
}

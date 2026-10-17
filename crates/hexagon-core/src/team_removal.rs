//! 「移出团队」（ui-redesign gap D2，2026-10-16；原型 settings#team 底部 ghost 红）。
//!
//! 语义：软删。`agents.removed_at` 记时间，名单 / 点名 / 按角色派活 / 激活读面一律
//! 滤掉（`AND removed_at IS NULL`），按 id 回看历史（时间线、产物作者、用量）仍取得到
//! 角色名。被否决：硬删——agents 被事件、消息、产物、待决等十余张表外键引用，轨迹是
//! 业务事实，删行要么被外键拒、要么连历史一起删。
//!
//! 守门（CONTEXT.md 没写移出语义，按 2026-10-16 协调会话裁决「有在跑的活或有待决就拒，
//! 带原因码」）。代价模型：误拒（false positive）花负责人一次「先休眠 / 先处理待决卡」，
//! 误放（false negative）会让在途回合或待决卡挂在一个名单里已不存在的 Agent 上、
//! 裁决后无人接手——实现偏向拒：
//! - `agent_not_found`：id 不在本项目或已移出（不做幂等成功：UI 已不列出它，再点是陈旧视图）。
//! - `agent_busy`：状态仍是 active（本阶段被激活），或还有没收尾的回合（turn_started
//!   之后没有 turn_finished / turn_failed）——先休眠，等回合收尾。
//! - `agent_has_pending`：名下还有 queued 待决卡——先在待决坞裁决。
//! - `agent_is_fastpath`：快速通道指定的就是它——移出后快速通道无人可派。
//!
//! 授权名单、记住的权限规则、经验条目不删：它们只在 Agent 被调度时生效，
//! 移出后不会再被调度；留着作审计。重新从模板添加同名角色得到新 id，授权从空开始
//! （安装 / 授权 ≠ 模板携带，CONTEXT.md「角色模板」）。
use crate::db::Db;
use crate::trace::{EventKind, TraceError};
use serde_json::json;

#[derive(Debug, thiserror::Error)]
pub enum RemoveAgentError {
    #[error("agent not found in this team: {0}")]
    AgentNotFound(String),
    #[error("agent is still working; put it to sleep and let its turn finish first: {0}")]
    AgentBusy(String),
    #[error("agent still has {1} pending item(s) waiting for the owner: {0}")]
    AgentHasPending(String, i64),
    #[error("agent is the fast-path assignee: {0}")]
    AgentIsFastpath(String),
    #[error(transparent)]
    Cards(#[from] crate::cards::CardsError),
    #[error(transparent)]
    Trace(#[from] TraceError),
    #[error("sqlite: {0}")]
    Sqlite(#[from] rusqlite::Error),
}

/// 还有没收尾的主回合（不看子 Agent 回合，同 `orchestra::detect_interrupted` 口径）。
fn has_open_turn(db: &Db, project_id: &str, agent_id: &str) -> Result<bool, rusqlite::Error> {
    db.conn().query_row(
        "SELECT EXISTS(SELECT 1 FROM events s WHERE s.project_id=?1 AND s.agent_id=?2
           AND s.kind='turn_started' AND COALESCE(json_extract(s.payload,'$.subagent'),0)=0
           AND NOT EXISTS(SELECT 1 FROM events t WHERE t.project_id=s.project_id
             AND t.agent_id=s.agent_id AND t.id>s.id
             AND t.kind IN ('turn_finished','turn_failed')
             AND COALESCE(json_extract(t.payload,'$.subagent'),0)=0))",
        rusqlite::params![project_id, agent_id],
        |r| r.get(0),
    )
}

fn refusal(
    db: &Db,
    project_id: &str,
    agent_id: &str,
) -> Result<Option<RemoveAgentError>, RemoveAgentError> {
    let row: Option<(String, Option<String>)> = match db.conn().query_row(
        "SELECT status, removed_at FROM agents WHERE project_id=?1 AND id=?2",
        rusqlite::params![project_id, agent_id],
        |r| Ok((r.get(0)?, r.get(1)?)),
    ) {
        Ok(v) => Some(v),
        Err(rusqlite::Error::QueryReturnedNoRows) => None,
        Err(e) => return Err(e.into()),
    };
    let id = agent_id.to_string();
    let Some((status, removed_at)) = row else {
        return Ok(Some(RemoveAgentError::AgentNotFound(id)));
    };
    if removed_at.is_some() {
        return Ok(Some(RemoveAgentError::AgentNotFound(id)));
    }
    if status == "active" || has_open_turn(db, project_id, agent_id)? {
        return Ok(Some(RemoveAgentError::AgentBusy(id)));
    }
    let pending = crate::cards::count_queued_for_agent(db, project_id, agent_id)?;
    if pending > 0 {
        return Ok(Some(RemoveAgentError::AgentHasPending(id, pending)));
    }
    let fastpath: Option<String> = db.conn().query_row(
        "SELECT fastpath_agent_id FROM projects WHERE id=?1",
        [project_id],
        |r| r.get(0),
    )?;
    if fastpath.as_deref() == Some(agent_id) {
        return Ok(Some(RemoveAgentError::AgentIsFastpath(id)));
    }
    Ok(None)
}

/// 负责人把一个 Agent 移出本项目团队。成功记 `System{kind:"agent_removed"}`
/// + 宿主诊断；拒绝记 Warn 诊断（原因码即错误码）。
pub fn remove_agent(db: &Db, project_id: &str, agent_id: &str) -> Result<(), RemoveAgentError> {
    let started = std::time::Instant::now();
    if let Some(err) = refusal(db, project_id, agent_id)? {
        let code = crate::errcode::ErrorCode::code(&err);
        crate::diag::note(
            crate::diag::CLASS_REJECT,
            true,
            Some(project_id),
            Some(agent_id),
            None,
            None,
            "agent_remove",
            &code,
            started,
        );
        return Err(err);
    }
    // 读角色名也带 project_id（review S3，2026-10-17）：曾只按 id 取，守门判定按
    // (project_id, id) 过而这里不过滤，读写口径不一致；与 refusal 同口径才算一处判定。
    let role: String = db.conn().query_row(
        "SELECT role FROM agents WHERE project_id=?1 AND id=?2",
        rusqlite::params![project_id, agent_id],
        |r| r.get(0),
    )?;
    // agents 行的状态 / 移出写路径只在属主 orchestra（AGENTS.md D09，review S3 起
    // removed_at 也收进属主，check-arch 守）；先落休眠再记移出，此后
    // write_agent_status 的 `removed_at IS NULL` 守卫让任何激活都碰不到它。
    crate::orchestra::write_agent_status(db, project_id, agent_id, true)?;
    crate::orchestra::write_agent_removed(db, project_id, agent_id)?;
    db.append_event(
        project_id,
        EventKind::System,
        json!({"kind": "agent_removed", "agent_id": agent_id, "role": role, "by": "owner"}),
        Some(agent_id),
        None,
    )?;
    crate::diag::note(
        crate::diag::CLASS_HOST,
        false,
        Some(project_id),
        Some(agent_id),
        None,
        None,
        "agent_remove",
        "agent_removed",
        started,
    );
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cards::CardKind;
    use crate::errcode::ErrorCode;

    fn team(roles: &[&str]) -> Db {
        let db = Db::open_in_memory().unwrap();
        db.conn()
            .execute(
                "INSERT INTO projects (id, dir, name, mode) VALUES ('p1','/tmp/x','t','pack')",
                [],
            )
            .unwrap();
        for (i, r) in roles.iter().enumerate() {
            db.conn()
                .execute(
                    "INSERT INTO agents (id, project_id, role) VALUES (?1,'p1',?2)",
                    rusqlite::params![format!("a{i}"), r],
                )
                .unwrap();
        }
        db
    }

    fn listed(db: &Db) -> Vec<String> {
        crate::orchestra::team(db, "p1", std::path::Path::new("/tmp/x"))
            .unwrap()
            .into_iter()
            .map(|r| r.id)
            .collect()
    }

    #[test]
    fn sleeping_idle_agent_leaves_roster_but_keeps_history() {
        let db = team(&["前端", "后端"]);
        db.append_event("p1", EventKind::TurnStarted, json!({}), Some("a0"), None)
            .unwrap();
        db.append_event("p1", EventKind::TurnFinished, json!({}), Some("a0"), None)
            .unwrap();
        remove_agent(&db, "p1", "a0").unwrap();
        assert_eq!(listed(&db), vec!["a1".to_string()]);
        assert_eq!(crate::roles::team_roles(&db, "p1").unwrap(), vec!["后端"]);
        // 历史按 id 仍可回看角色名
        let role: String = db
            .conn()
            .query_row("SELECT role FROM agents WHERE id='a0'", [], |r| r.get(0))
            .unwrap();
        assert_eq!(role, "前端");
        let n: i64 = db
            .conn()
            .query_row(
                "SELECT COUNT(*) FROM events WHERE kind='system' AND json_extract(payload,'$.kind')='agent_removed'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(n, 1);
        // 移出后激活写不进去
        crate::orchestra::write_agent_status(&db, "p1", "a0", false).unwrap();
        let st: String = db
            .conn()
            .query_row("SELECT status FROM agents WHERE id='a0'", [], |r| r.get(0))
            .unwrap();
        assert_eq!(st, "sleeping");
    }

    #[test]
    fn active_or_mid_turn_agent_is_refused_as_busy() {
        let db = team(&["前端"]);
        crate::orchestra::write_agent_status(&db, "p1", "a0", false).unwrap();
        let e = remove_agent(&db, "p1", "a0").unwrap_err();
        assert_eq!(e.code(), "agent_busy");
        crate::orchestra::write_agent_status(&db, "p1", "a0", true).unwrap();
        db.append_event("p1", EventKind::TurnStarted, json!({}), Some("a0"), None)
            .unwrap();
        let e = remove_agent(&db, "p1", "a0").unwrap_err();
        assert_eq!(e.code(), "agent_busy");
        assert_eq!(listed(&db), vec!["a0".to_string()]);
    }

    #[test]
    fn queued_pending_item_blocks_until_answered() {
        let db = team(&["前端"]);
        let q = crate::cards::enqueue(&db, "p1", Some("a0"), CardKind::Permission, json!({}), None)
            .unwrap();
        let e = remove_agent(&db, "p1", "a0").unwrap_err();
        assert_eq!(e.code(), "agent_has_pending");
        crate::cards::answer(&db, &q, "owner").unwrap();
        remove_agent(&db, "p1", "a0").unwrap();
    }

    #[test]
    fn fastpath_assignee_and_unknown_or_removed_are_refused() {
        let db = team(&["前端", "后端"]);
        db.conn()
            .execute(
                "UPDATE projects SET fastpath_agent_id='a1' WHERE id='p1'",
                [],
            )
            .unwrap();
        assert_eq!(
            remove_agent(&db, "p1", "a1").unwrap_err().code(),
            "agent_is_fastpath"
        );
        assert_eq!(
            remove_agent(&db, "p1", "zz").unwrap_err().code(),
            "agent_not_found"
        );
        remove_agent(&db, "p1", "a0").unwrap();
        assert_eq!(
            remove_agent(&db, "p1", "a0").unwrap_err().code(),
            "agent_not_found"
        );
    }

    #[test]
    fn removed_role_can_be_added_again_from_template() {
        let db = team(&[]);
        let def = crate::presets::RoleDef {
            name: "前端".into(),
            duty: "d".into(),
            reviewer: None,
            model_slot: "chat".into(),
            globs: vec![],
            skills: vec![],
        };
        let first = crate::roles::create_role(&db, "p1", &def).unwrap();
        remove_agent(&db, "p1", &first).unwrap();
        let again = crate::roles::create_role(&db, "p1", &def).unwrap();
        assert_ne!(first, again);
        assert_eq!(listed(&db), vec![again]);
    }

    // D12（review S3，2026-10-17）：守门判定配属性测试。不变量：
    // ① 拒绝码按 not_found > busy > pending > fastpath 的优先序唯一确定；
    // ② 被拒时零副作用（removed_at 不动、不记 agent_removed）；
    // ③ 放行时恰好落一次移出，且再移同一 id 必拒 not_found（不做幂等成功）；
    // ④ 别的项目里的同 id Agent 一律 not_found（project_id 过滤）。
    mod props {
        use super::*;
        use proptest::prelude::*;

        #[derive(Debug, Clone)]
        struct Shape {
            /// 0 = 不存在，1 = 在本项目，2 = 在别的项目
            home: u8,
            removed: bool,
            active: bool,
            open_turn: bool,
            pending: u8,
            fastpath: bool,
        }

        fn shape() -> impl Strategy<Value = Shape> {
            (
                0u8..3,
                any::<bool>(),
                any::<bool>(),
                any::<bool>(),
                0u8..3,
                any::<bool>(),
            )
                .prop_map(|(home, removed, active, open_turn, pending, fastpath)| {
                    Shape {
                        home,
                        removed,
                        active,
                        open_turn,
                        pending,
                        fastpath,
                    }
                })
        }

        fn build(s: &Shape) -> Db {
            let db = team(&[]);
            db.conn()
                .execute(
                    "INSERT INTO projects (id, dir, name, mode) VALUES ('p2','/tmp/y','u','pack')",
                    [],
                )
                .unwrap();
            let pid = match s.home {
                0 => return db,
                1 => "p1",
                _ => "p2",
            };
            db.conn()
                .execute(
                    "INSERT INTO agents (id, project_id, role) VALUES ('a0', ?1, '前端')",
                    [pid],
                )
                .unwrap();
            crate::orchestra::write_agent_status(&db, pid, "a0", !s.active).unwrap();
            if s.open_turn {
                db.append_event(pid, EventKind::TurnStarted, json!({}), Some("a0"), None)
                    .unwrap();
            }
            for _ in 0..s.pending {
                crate::cards::enqueue(&db, pid, Some("a0"), CardKind::Permission, json!({}), None)
                    .unwrap();
            }
            if s.fastpath {
                db.conn()
                    .execute(
                        "UPDATE projects SET fastpath_agent_id='a0' WHERE id=?1",
                        [pid],
                    )
                    .unwrap();
            }
            if s.removed {
                crate::orchestra::write_agent_removed(&db, pid, "a0").unwrap();
            }
            db
        }

        fn expected(s: &Shape) -> Option<&'static str> {
            if s.home != 1 || s.removed {
                Some("agent_not_found")
            } else if s.active || s.open_turn {
                Some("agent_busy")
            } else if s.pending > 0 {
                Some("agent_has_pending")
            } else if s.fastpath {
                Some("agent_is_fastpath")
            } else {
                None
            }
        }

        fn removed_marks(db: &Db) -> (i64, i64) {
            let rows: i64 = db
                .conn()
                .query_row(
                    "SELECT COUNT(*) FROM agents WHERE removed_at IS NOT NULL",
                    [],
                    |r| r.get(0),
                )
                .unwrap();
            let events: i64 = db
                .conn()
                .query_row(
                    "SELECT COUNT(*) FROM events WHERE kind='system' AND json_extract(payload,'$.kind')='agent_removed'",
                    [],
                    |r| r.get(0),
                )
                .unwrap();
            (rows, events)
        }

        proptest! {
            #[test]
            fn refusal_is_ordered_and_side_effect_free(s in shape()) {
                let db = build(&s);
                let before = removed_marks(&db);
                let judged = refusal(&db, "p1", "a0").unwrap().map(|e| e.code());
                prop_assert_eq!(judged.as_deref(), expected(&s));
                match remove_agent(&db, "p1", "a0") {
                    Err(e) => {
                        let code = e.code();
                        prop_assert_eq!(Some(code.as_str()), expected(&s));
                        prop_assert_eq!(removed_marks(&db), before);
                    }
                    Ok(()) => {
                        prop_assert_eq!(expected(&s), None);
                        prop_assert_eq!(removed_marks(&db), (before.0 + 1, before.1 + 1));
                        let again = remove_agent(&db, "p1", "a0").unwrap_err();
                        prop_assert_eq!(again.code(), "agent_not_found");
                    }
                }
            }
        }
    }
}

//! 自治档位与归来摘要（票 17；五档存储见 hands-free 票 01 / ADR 0064）。
//!
//! 档位语义（项目级，新项目默认 L4）：
//! - **L0**：一切决策排队等负责人；
//! - **L1**：声明内自动——回填边、会诊唤醒命中即执行；打回裁决与新权限仍排队；
//! - **L2**：协调自治——打回路由裁决、阶段推进自动跑；**盖章点、安全网必问、
//!   新权限问题永远等负责人**（这三条不在档位控制内，见 permissions/orchestra）。
//! - **L3 / L4**：历史协调档。2026-10-01 负责人 Q3/Q10 将工具访问审批
//!   独立为 approval_mode；rank 不再放行安全网或新权限。非最终盖章点
//!   自动通过，最终验收仍等人（票 02）。
//!   提案负责人盖章、技能/MCP 授权确认、自然语言安装确认只在 L4 自动通过
//!   （`harnessgate` 读存储档，票 04）。`execution_rank` 仍封顶 2。
//!   内置永不、项目否定、远程发布、开场项目说明草案任何档都不放行。
//! - 升级通道（escalation）在任何档位都排负责人——升级的定义就是超出声明自治。
//!
//! 归来摘要：**工作台模板生成**（不经模型）——按事件类型聚合成结构化摘要，
//! 可折叠块进时间线；内容只来自事件表，与实际发生严格一致。

use rusqlite::params;
use serde_json::json;

use crate::db::Db;
use crate::trace::EventKind;

#[derive(Debug, thiserror::Error)]
pub enum AutonomyError {
    #[error("sqlite: {0}")]
    Sqlite(#[from] rusqlite::Error),
    #[error(transparent)]
    Cards(#[from] crate::cards::CardsError),
    #[error("trace: {0}")]
    Trace(#[from] crate::trace::TraceError),
    #[error("db: {0}")]
    Db(#[from] crate::db::DbError),
    #[error("invalid autonomy level: {0}")]
    BadLevel(String),
    /// ADR 0069：自治不再分档。任何写入都拒绝，已存列不动。
    #[error("autonomy gears are gone")]
    GearsRemoved,
    #[error("json: {0}")]
    Json(#[from] serde_json::Error),
}

/// 归来摘要（ADR 0054）：模板化计数表 + 待办 + 交付清单。
/// 同一形状既作 IPC 返回也作 ReturnSummary 事件载荷。
#[derive(Debug, Clone, serde::Serialize, ts_rs::TS)]
#[ts(export, export_to = "../../../ui/src/gen/")]
pub struct ReturnSummary {
    #[ts(type = "number")] // JS number 域（wire 上是 JSON number，ts-rs 默认 bigint 不符 wire）
    pub since_event: i64,
    pub deliveries: Vec<DeliveryRow>,
    pub reviews: ReviewCounts,
    pub flags: FlagCounts,
    pub permissions: PermCounts,
    pub stages: StageCounts,
    pub pending_todos: Vec<TodoCount>,
    pub attention: ReturnAttention,
}

/// Reliability 23: current uncertainty is separate from historical decisions.
#[derive(Debug, Clone, serde::Serialize, ts_rs::TS)]
#[ts(export, export_to = "../../../ui/src/gen/")]
pub struct ReturnAttention {
    #[ts(type = "number")]
    pub unresolved_actions: i64,
    #[ts(type = "number")]
    pub unknown_cost_records: i64,
    #[ts(type = "number")]
    pub budget_stops: i64,
    #[ts(type = "number")]
    pub exception_decisions: i64,
    #[ts(type = "number")]
    pub policy_candidates: i64,
}

#[derive(Debug, Clone, serde::Serialize, ts_rs::TS)]
#[ts(export, export_to = "../../../ui/src/gen/")]
pub struct DeliveryRow {
    pub path: Option<String>,
    pub kind: Option<String>,
}

#[derive(Debug, Clone, serde::Serialize, ts_rs::TS)]
#[ts(export, export_to = "../../../ui/src/gen/")]
pub struct ReviewCounts {
    #[ts(type = "number")] // JS number 域（wire 上是 JSON number，ts-rs 默认 bigint 不符 wire）
    pub passed: i64,
    #[ts(type = "number")] // JS number 域（wire 上是 JSON number，ts-rs 默认 bigint 不符 wire）
    pub rejected: i64,
}

#[derive(Debug, Clone, serde::Serialize, ts_rs::TS)]
#[ts(export, export_to = "../../../ui/src/gen/")]
pub struct FlagCounts {
    #[ts(type = "number")] // JS number 域（wire 上是 JSON number，ts-rs 默认 bigint 不符 wire）
    pub submitted: i64,
    #[ts(type = "number")] // JS number 域（wire 上是 JSON number，ts-rs 默认 bigint 不符 wire）
    pub adjudicated: i64,
    #[ts(type = "number")] // JS number 域（wire 上是 JSON number，ts-rs 默认 bigint 不符 wire）
    pub escalated: i64,
}

#[derive(Debug, Clone, serde::Serialize, ts_rs::TS)]
#[ts(export, export_to = "../../../ui/src/gen/")]
pub struct PermCounts {
    #[ts(type = "number")] // JS number 域（wire 上是 JSON number，ts-rs 默认 bigint 不符 wire）
    pub asked: i64,
    #[ts(type = "number")] // JS number 域（wire 上是 JSON number，ts-rs 默认 bigint 不符 wire）
    pub allowed: i64,
    #[ts(type = "number")] // JS number 域（wire 上是 JSON number，ts-rs 默认 bigint 不符 wire）
    pub denied: i64,
}

#[derive(Debug, Clone, serde::Serialize, ts_rs::TS)]
#[ts(export, export_to = "../../../ui/src/gen/")]
pub struct StageCounts {
    #[ts(type = "number")] // JS number 域（wire 上是 JSON number，ts-rs 默认 bigint 不符 wire）
    pub finished: i64,
    #[ts(type = "number")] // JS number 域（wire 上是 JSON number，ts-rs 默认 bigint 不符 wire）
    pub skipped: i64,
    #[ts(type = "number")] // JS number 域（wire 上是 JSON number，ts-rs 默认 bigint 不符 wire）
    pub rewound: i64,
}

#[derive(Debug, Clone, serde::Serialize, ts_rs::TS)]
#[ts(export, export_to = "../../../ui/src/gen/")]
pub struct TodoCount {
    pub kind: String,
    #[ts(type = "number")] // JS number 域（wire 上是 JSON number，ts-rs 默认 bigint 不符 wire）
    pub count: i64,
}

/// 词表内的档 → 存储秩 0–4。词表外是 `BadLevel`，不入库。
pub fn parse_level(lv: &str) -> Result<u8, AutonomyError> {
    Ok(match lv {
        "L0" => 0,
        "L1" => 1,
        "L2" => 2,
        "L3" => 3,
        "L4" => 4,
        _ => return Err(AutonomyError::BadLevel(lv.into())),
    })
}

/// 离开时的放行秩。ADR 0069 取消档位之后恒为原先的 L4（4）。
///
/// 不读 `projects.autonomy`。夹具把列写成 L0 不再收紧协调盖章或授权。
/// Owner 2026-10-01: 工具权限独立读 approval_mode，不使用本函数。
/// 被否决：继续 `SELECT autonomy`——测试夹具的 L0 会把离开时的放行打回成一切排队。
/// 列还在，只给旧夹具和 `level()` 读口；放行不看它。
pub fn rank(_db: &Db, _project_id: &str) -> Result<u8, rusqlite::Error> {
    Ok(4)
}

/// 执行档。原先把存储档封顶到 2，避免只有名字的高档被当成已经放行。
///
/// ADR 0069 之后不再读列，恒为原先 L4 的封顶 2。打回路由因此按协调自治走。
/// 被否决：继续读列再 `min(2)`。夹具写成 L0 时，回填和复审唤醒会停住。
pub fn execution_rank(_db: &Db, _project_id: &str) -> Result<u8, rusqlite::Error> {
    Ok(2)
}

pub fn level(db: &Db, project_id: &str) -> Result<String, rusqlite::Error> {
    db.conn().query_row(
        "SELECT autonomy FROM projects WHERE id=?1",
        [project_id],
        |r| r.get(0),
    )
}

/// 设档。ADR 0069 起一律拒绝，不写列、不落变档事件。
///
/// 离开时的放行等于原先的 L4，但那不是一档可以再选。被否决：仍接受
/// 「L4」当作确认当前放手——那仍是可调档。false positive 是负责人
/// 以为自己收紧了，离开后却按高档放行。
pub fn set_level(db: &Db, project_id: &str, lv: &str) -> Result<(), AutonomyError> {
    let started = std::time::Instant::now();
    let _ = (db, lv);
    crate::diag::note(
        "拒绝",
        true,
        Some(project_id),
        None,
        None,
        None,
        "set_autonomy",
        "gears_removed",
        started,
    );
    Err(AutonomyError::GearsRemoved)
}

/// 审查者档位（shadow/live，票 05）。live 只在 autonomy ≥ L1 生效；
/// 这里不强制档位组合——L0+live 退化为 shadow 判定（adjudicate 内拦）。
pub fn set_reviewer_mode(db: &Db, project_id: &str, mode: &str) -> Result<(), AutonomyError> {
    if !matches!(mode, "shadow" | "live") {
        return Err(AutonomyError::BadLevel(format!(
            "invalid reviewer mode: {mode}"
        )));
    }
    db.conn().execute(
        "UPDATE projects SET reviewer_mode=?1 WHERE id=?2",
        params![mode, project_id],
    )?;
    Ok(())
}

/// 当前审查者档位。
pub fn reviewer_mode(db: &Db, project_id: &str) -> Result<String, rusqlite::Error> {
    db.conn().query_row(
        "SELECT reviewer_mode FROM projects WHERE id=?1",
        [project_id],
        |r| r.get(0),
    )
}

/// 负责人离开：打标记事件（挂起语义 = 必问类照常排队，无人裁决而已）。
pub fn leave(db: &Db, project_id: &str) -> Result<i64, AutonomyError> {
    let id = db.append_event(
        project_id,
        EventKind::System,
        json!({"note": "owner_away"}),
        None,
        None,
    )?;
    log::info!("owner away marker: project={project_id} event={id}");
    Ok(id)
}

/// 负责人归来：自上次离开标记以来的事件 → 模板化摘要 → ReturnSummary 事件。
/// 无离开标记则汇总全部事件。
pub fn back(db: &Db, project_id: &str) -> Result<ReturnSummary, AutonomyError> {
    let since: i64 = db
        .conn()
        .query_row(
            "SELECT COALESCE(MAX(id),0) FROM events
             WHERE project_id=?1 AND kind='system' AND json_extract(payload,'$.note')='owner_away'",
            [project_id],
            |r| r.get(0),
        )
        .unwrap_or(0);

    let count = |kind: &str| -> i64 {
        db.conn()
            .query_row(
                "SELECT COUNT(*) FROM events WHERE project_id=?1 AND kind=?2 AND id>?3",
                params![project_id, kind, since],
                |r| r.get(0),
            )
            .unwrap_or(0)
    };

    // 待办：仍排队的必问/盖章/升级/发布卡
    let todos: Vec<TodoCount> = crate::cards::queued_kind_counts(db, project_id)?
        .into_iter()
        .map(|(kind, count)| TodoCount { kind, count })
        .collect();

    // 离开期间交付的产物（带 kind）
    let mut st = db.conn().prepare(
        "SELECT json_extract(payload,'$.path'), json_extract(payload,'$.kind')
         FROM events WHERE project_id=?1 AND kind='artifact_delivered' AND id>?2",
    )?;
    let deliveries: Vec<DeliveryRow> = st
        .query_map(params![project_id, since], |r| {
            Ok(DeliveryRow {
                path: r.get(0)?,
                kind: r.get(1)?,
            })
        })?
        .collect::<Result<_, _>>()?;

    let costs = crate::usage::project_summary(db, project_id)?.total;
    let attention = ReturnAttention {
        unresolved_actions: db.conn().query_row("SELECT COUNT(*) FROM tool_actions a WHERE project_id=?1 AND state='unknown' AND NOT EXISTS(SELECT 1 FROM action_resolutions r WHERE r.action_id=a.id)", [project_id], |r|r.get(0))?,
        unknown_cost_records: costs.unknown_requests.saturating_add(costs.legacy_unknown_records),
        budget_stops: db.conn().query_row("SELECT COUNT(*) FROM events WHERE project_id=?1 AND id>?2 AND ((kind='turn_failed' AND json_extract(payload,'$.code')='budget-exceeded') OR kind='usage_cap_hit')", params![project_id,since], |r|r.get(0))?,
        exception_decisions: db.conn().query_row("SELECT COUNT(*) FROM events WHERE project_id=?1 AND id>?2 AND kind='system' AND json_extract(payload,'$.kind')='acceptance_exception'", params![project_id,since], |r|r.get(0))?,
        policy_candidates: db.conn().query_row("SELECT COUNT(*) FROM proposals WHERE project_id=?1 AND surface='pack_copy' AND status IN ('queued','in_review','awaiting_stamp')",[project_id],|r|r.get(0))?,
    };
    let summary = ReturnSummary {
        attention,
        since_event: since,
        deliveries,
        reviews: ReviewCounts {
            passed: count("review_passed"),
            rejected: count("review_rejected"),
        },
        flags: FlagCounts {
            submitted: count("flag_submitted"),
            adjudicated: count("flag_adjudicated"),
            escalated: count("escalated"),
        },
        permissions: PermCounts {
            asked: count("permission_asked"),
            allowed: count("permission_allowed"),
            denied: count("permission_denied"),
        },
        stages: StageCounts {
            finished: count("stage_finished"),
            skipped: count("stage_skipped"),
            rewound: count("stage_rewound"),
        },
        pending_todos: todos,
    };

    db.append_event(
        project_id,
        EventKind::ReturnSummary,
        serde_json::to_value(&summary)?,
        None,
        None,
    )?;
    log::info!("return summary: project={project_id} since={since}");
    Ok(summary)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn setup() -> Db {
        let db = Db::open_in_memory().unwrap();
        db.conn()
            .execute(
                "INSERT INTO projects (id, dir, name, mode) VALUES ('p','/tmp/x','x','pack')",
                [],
            )
            .unwrap();
        db
    }

    #[test]
    fn level_validate_and_change() {
        let db = setup();
        // 票 01 / ADR 0064：省略档位的新行默认 L4（曾是 L0）。
        // 已有行显式写入的 L0 不被迁移改写，见 0014 的 INSERT SELECT。
        assert_eq!(level(&db, "p").unwrap(), "L4");
        // 票 04 也不抬封顶：提案/授权/安装读 rank，执行档仍是 2。
        assert_eq!(execution_rank(&db, "p").unwrap(), 2);
        // ADR 0069：设档一律拒绝，列保持默认，也不写变档事件。
        assert!(matches!(
            set_level(&db, "p", "L3"),
            Err(AutonomyError::GearsRemoved)
        ));
        assert!(set_level(&db, "p", "L9").is_err());
        assert_eq!(level(&db, "p").unwrap(), "L4");
        assert_eq!(rank(&db, "p").unwrap(), 4);
        let n: i64 = db
            .conn()
            .query_row(
                "SELECT COUNT(*) FROM events WHERE kind='autonomy_changed'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(n, 0);
    }

    #[test]
    fn summary_covers_each_event_class() {
        let db = setup();
        // 离开前的事不计入
        db.append_event(
            "p",
            EventKind::ArtifactDelivered,
            json!({"path":"a.md","kind":"规格"}),
            None,
            None,
        )
        .unwrap();
        leave(&db, "p").unwrap();
        // 离开期间：交付/复审/打回/权限/阶段推进
        db.append_event(
            "p",
            EventKind::ArtifactDelivered,
            json!({"path":"b.md","kind":"接口说明"}),
            None,
            None,
        )
        .unwrap();
        db.append_event("p", EventKind::ReviewPassed, json!({}), None, None)
            .unwrap();
        db.append_event("p", EventKind::ReviewRejected, json!({}), None, None)
            .unwrap();
        db.append_event("p", EventKind::FlagSubmitted, json!({}), None, None)
            .unwrap();
        db.append_event(
            "p",
            EventKind::FlagAdjudicated,
            json!({"agreed":true}),
            None,
            None,
        )
        .unwrap();
        db.append_event("p", EventKind::PermissionAsked, json!({}), None, None)
            .unwrap();
        db.append_event("p", EventKind::StageFinished, json!({}), None, None)
            .unwrap();
        crate::cards::enqueue(
            &db,
            "p",
            None,
            crate::cards::CardKind::Permission,
            json!({}),
            None,
        )
        .unwrap();

        let s = serde_json::to_value(back(&db, "p").unwrap()).unwrap();
        assert_eq!(s["deliveries"].as_array().unwrap().len(), 1); // 只有 b.md
        assert_eq!(s["reviews"]["passed"], 1);
        assert_eq!(s["reviews"]["rejected"], 1);
        assert_eq!(s["flags"]["submitted"], 1);
        assert_eq!(s["flags"]["adjudicated"], 1);
        assert_eq!(s["permissions"]["asked"], 1);
        assert_eq!(s["stages"]["finished"], 1);
        assert_eq!(s["pending_todos"][0]["count"], 1);
        // ReturnSummary 事件本身也落了
        let n: i64 = db
            .conn()
            .query_row(
                "SELECT COUNT(*) FROM events WHERE kind='return_summary'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(n, 1);
        // 再次 back：since 仍是同一个 away 标记
        let s2 = serde_json::to_value(back(&db, "p").unwrap()).unwrap();
        assert_eq!(s2["reviews"]["passed"], 1);
    }
}

//! 自治档位与归来摘要（票 17；五档存储见 hands-free 票 01 / ADR 0064）。
//!
//! 档位语义（项目级，新项目默认 L4）：
//! - **L0**：一切决策排队等负责人；
//! - **L1**：声明内自动——回填边、会诊唤醒命中即执行；打回裁决与新权限仍排队；
//! - **L2**：协调自治——打回路由裁决、阶段推进自动跑；**盖章点、安全网必问、
//!   新权限问题永远等负责人**（这三条不在档位控制内，见 permissions/orchestra）。
//! - **L3 / L4**：安全网和新的权限询问直接放行（`permissions` 读存储档 `rank`，
//!   票 03）。盖章点也读存储档：非最终盖章点自动通过，最终验收仍等人（票 02）。
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

/// 存储档 L0–L4 → 0–4。词表外（不该入库）按 0：脏值不升档。
pub fn rank(db: &Db, project_id: &str) -> Result<u8, rusqlite::Error> {
    let level = level(db, project_id)?;
    Ok(parse_level(&level).unwrap_or(0))
}

/// 执行档。存储档见 `rank`。
///
/// 票 01 把执行档封顶到 2，避免只有名字的高档被当成已经放行。
/// 票 02 的盖章、票 03 的安全网和新权限、票 04 的提案/授权/安装都不读这里，
/// 它们读存储档 `rank`。被否决：把 `min(2)` 改成 `min(4)`。
/// false positive 的代价是未审的安装或提案生效，而且会连坐仍读执行档的打回路径。
pub fn execution_rank(db: &Db, project_id: &str) -> Result<u8, rusqlite::Error> {
    Ok(rank(db, project_id)?.min(2))
}

pub fn level(db: &Db, project_id: &str) -> Result<String, rusqlite::Error> {
    db.conn().query_row(
        "SELECT autonomy FROM projects WHERE id=?1",
        [project_id],
        |r| r.get(0),
    )
}

/// 变档：校验 + 落 AutonomyChanged 事件。
pub fn set_level(db: &Db, project_id: &str, lv: &str) -> Result<(), AutonomyError> {
    // 先校验再写：非法档不碰行，原档保持（票 01）。
    parse_level(lv)?;
    let old = level(db, project_id)?;
    db.conn().execute(
        "UPDATE projects SET autonomy=?1 WHERE id=?2",
        params![lv, project_id],
    )?;
    db.append_event(
        project_id,
        EventKind::AutonomyChanged,
        json!({"from": old, "to": lv}),
        None,
        None,
    )?;
    log::info!("autonomy {project_id}: {old} -> {lv}");
    Ok(())
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

    let summary = ReturnSummary {
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
        assert!(set_level(&db, "p", "L9").is_err());
        assert_eq!(level(&db, "p").unwrap(), "L4");
        set_level(&db, "p", "L3").unwrap();
        assert_eq!(level(&db, "p").unwrap(), "L3");
        assert_eq!(rank(&db, "p").unwrap(), 3);
        assert_eq!(execution_rank(&db, "p").unwrap(), 2);
        set_level(&db, "p", "L2").unwrap();
        assert_eq!(rank(&db, "p").unwrap(), 2);
        // 非法档不落事件；两次成功变档各一条
        let n: i64 = db
            .conn()
            .query_row(
                "SELECT COUNT(*) FROM events WHERE kind='autonomy_changed'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(n, 2);
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

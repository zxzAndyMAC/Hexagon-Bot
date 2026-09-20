//! 自治档位与归来摘要（票 17）。
//!
//! 档位语义（项目级，默认 L0）：
//! - **L0**：一切决策排队等负责人；
//! - **L1**：声明内自动——回填边、会诊唤醒命中即执行；打回裁决与新权限仍排队；
//! - **L2**：协调自治——打回路由裁决、阶段推进自动跑；**盖章点、安全网必问、
//!   新权限问题永远等负责人**（这三条不在档位控制内，见 permissions/orchestra）。
//! - 升级通道（escalation）在任何档位都排负责人——升级的定义就是超出声明自治。
//!
//! 归来摘要：**工作台模板生成**（不经模型）——按事件类型聚合成结构化摘要，
//! 可折叠块进时间线；内容只来自事件表，与实际发生严格一致。

use rusqlite::params;
use serde_json::{json, Value};

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
}

/// L0/L1/L2 → 0/1/2。
pub fn rank(db: &Db, project_id: &str) -> Result<u8, rusqlite::Error> {
    let level: String = db.conn().query_row(
        "SELECT autonomy FROM projects WHERE id=?1",
        [project_id],
        |r| r.get(0),
    )?;
    Ok(match level.as_str() {
        "L1" => 1,
        "L2" => 2,
        _ => 0,
    })
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
    if !matches!(lv, "L0" | "L1" | "L2") {
        return Err(AutonomyError::BadLevel(lv.into()));
    }
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
pub fn back(db: &Db, project_id: &str) -> Result<Value, AutonomyError> {
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
    let todos: Vec<Value> = crate::cards::queued_kind_counts(db, project_id)?
        .into_iter()
        .map(|(kind, count)| json!({"kind": kind, "count": count}))
        .collect();

    // 离开期间交付的产物（带 kind）
    let mut st = db.conn().prepare(
        "SELECT json_extract(payload,'$.path'), json_extract(payload,'$.kind')
         FROM events WHERE project_id=?1 AND kind='artifact_delivered' AND id>?2",
    )?;
    let deliveries: Vec<Value> = st
        .query_map(params![project_id, since], |r| {
            Ok(json!({"path": r.get::<_, Option<String>>(0)?,
                      "kind": r.get::<_, Option<String>>(1)?}))
        })?
        .collect::<Result<_, _>>()?;

    let summary = json!({
        "since_event": since,
        "deliveries": deliveries,
        "reviews": {
            "passed": count("review_passed"),
            "rejected": count("review_rejected"),
        },
        "flags": {
            "submitted": count("flag_submitted"),
            "adjudicated": count("flag_adjudicated"),
            "escalated": count("escalated"),
        },
        "permissions": {
            "asked": count("permission_asked"),
            "allowed": count("permission_allowed"),
            "denied": count("permission_denied"),
        },
        "stages": {
            "finished": count("stage_finished"),
            "skipped": count("stage_skipped"),
            "rewound": count("stage_rewound"),
        },
        "pending_todos": todos,
    });

    db.append_event(
        project_id,
        EventKind::ReturnSummary,
        summary.clone(),
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
        assert_eq!(level(&db, "p").unwrap(), "L0");
        assert!(set_level(&db, "p", "L9").is_err());
        set_level(&db, "p", "L2").unwrap();
        assert_eq!(rank(&db, "p").unwrap(), 2);
        // 变档落事件
        let n: i64 = db
            .conn()
            .query_row(
                "SELECT COUNT(*) FROM events WHERE kind='autonomy_changed'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(n, 1);
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

        let s = back(&db, "p").unwrap();
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
        let s2 = back(&db, "p").unwrap();
        assert_eq!(s2["reviews"]["passed"], 1);
    }
}

//! 用量账本（票 12）：计量、汇总、上限硬闸。
//!
//! - 计量：每次模型响应记 prompt/completion tokens；每次工具结果按
//!   字节/4 估算工具输出 tokens 一并记入行。
//! - 成本：单价表从项目目录 `.hexagon/prices.json` 读（可改、不落盘死价），
//!   缺文件则成本按 0 计——token 数永远真实记录，钱只是本地估算。
//! - 上限：`projects.usage_limit_cents`（分）对 `SUM(cost_millicents)/1000`；
//!   触顶 → 全员休眠 + UsageCapHit/TeamSlept 事件，之后任何 Agent 不被调度
//!   （turn 内核在召模型前先查账）。上限压过自治档位。

use rusqlite::params;
use serde_json::{json, Value};

use crate::db::Db;
use crate::provider::Usage;
use crate::tools::ToolContext;
use crate::trace::EventKind;

/// 单档价格：每 1K token 的毫分（1 分 = 1000 毫分）。
#[derive(Debug, Clone, Copy, Default)]
pub struct Price {
    pub prompt_per_1k_mc: i64,
    pub completion_per_1k_mc: i64,
}

/// 从 `.hexagon/prices.json` 读价格表。格式：
/// `{ "default": {...}, "models": { "<slot>": {...} } }`
fn price_for(repo_root: &std::path::Path, model_slot: &str) -> Price {
    let Ok(text) = std::fs::read_to_string(repo_root.join(".hexagon/prices.json")) else {
        return Price::default();
    };
    let Ok(v) = serde_json::from_str::<Value>(&text) else {
        return Price::default();
    };
    let entry = v
        .get("models")
        .and_then(|m| m.get(model_slot))
        .or_else(|| v.get("default")); // D13-exempt: prices.json 价目兜底，非槽位绑定回退
    entry
        .map(|e| Price {
            prompt_per_1k_mc: e["prompt_per_1k_mc"].as_i64().unwrap_or(0),
            completion_per_1k_mc: e["completion_per_1k_mc"].as_i64().unwrap_or(0),
        })
        .unwrap_or_default()
}

/// 记一行账。`tool_output_bytes` 是本轮工具结果合计字节数，按 /4 估 token。
pub fn record(
    db: &Db,
    ctx: &ToolContext,
    model_slot: &str,
    usage: &Usage,
    tool_output_bytes: usize,
) -> Result<(), rusqlite::Error> {
    let price = price_for(&ctx.repo_root, model_slot);
    let cost = (usage.prompt_tokens as i64 * price.prompt_per_1k_mc
        + usage.completion_tokens as i64 * price.completion_per_1k_mc)
        / 1000;
    db.conn().execute(
        "INSERT INTO usage (project_id, agent_id, model, prompt_tokens,
                            completion_tokens, tool_output_tokens, cost_millicents,
                            stage_run_id)
         VALUES (?1,?2,?3,?4,?5,?6,?7,?8)",
        params![
            ctx.project_id,
            ctx.agent_id,
            model_slot,
            usage.prompt_tokens as i64,
            usage.completion_tokens as i64,
            // 双口径分工（context-window 票 01）：tool_output 账仍按字节/4
            // 粗估——账务近似量级即可；撞限判定走 turn/context.rs 的
            // cl100k 真分词，两侧精度要求不同，勿互相对齐。
            (tool_output_bytes / 4) as i64,
            cost,
            ctx.stage_run_id,
        ],
    )?;
    Ok(())
}

/// 项目已花费（毫分）。
pub fn spent_mc(db: &Db, project_id: &str) -> Result<i64, rusqlite::Error> {
    db.conn().query_row(
        "SELECT COALESCE(SUM(cost_millicents),0) FROM usage WHERE project_id=?1",
        [project_id],
        |r| r.get(0),
    )
}

/// 上限硬闸：触顶则全员休眠 + 落事件，返回 true。未设上限或未到顶返回 false。
/// 幂等：已全部休眠时不再重复落 TeamSlept（UsageCapHit 只在由未到顶转为触顶时落一次）。
pub fn enforce_cap(db: &Db, project_id: &str) -> Result<bool, crate::trace::TraceError> {
    let limit: Option<i64> = db.conn().query_row(
        "SELECT usage_limit_cents FROM projects WHERE id=?1",
        [project_id],
        |r| r.get(0),
    )?;
    let Some(limit_cents) = limit else {
        return Ok(false);
    };
    if spent_mc(db, project_id)? < limit_cents * 1000 {
        return Ok(false);
    }
    let active: i64 = db.conn().query_row(
        "SELECT COUNT(*) FROM agents WHERE project_id=?1 AND status != 'sleeping'",
        [project_id],
        |r| r.get(0),
    )?;
    if active == 0 {
        return Ok(true); // 已触顶且已休眠：仍是闸，但事件不重复
    }
    crate::orchestra::write_team_sleeping(db, project_id)?;
    log::warn!("usage cap hit: project={project_id} limit={limit_cents}¢ — team slept");
    db.append_event(
        project_id,
        EventKind::UsageCapHit,
        json!({ "limit_cents": limit_cents, "spent_mc": spent_mc(db, project_id)?,
                "code": crate::trace::FailureCode::BudgetExceeded.as_str() }),
        None,
        None,
    )?;
    db.append_event(
        project_id,
        EventKind::TeamSlept,
        // 票 02：用量触顶是闭集失败理由（budget-exceeded）——回放报告
        // 按 code 聚合,不带 code 的触顶会漏出失败分布。
        json!({ "reason": "usage_cap", "code": crate::trace::FailureCode::BudgetExceeded.as_str() }),
        None,
        None,
    )?;
    Ok(true)
}

/// 多维汇总：按 Agent × 模型 × 阶段分组，附项目总计与上限。
/// `stage` 为阶段名；NULL = 未分阶段的历史/非阶段账。
/// 用量汇总行（ADR 0054）：agent×model×stage 账本维。
#[derive(Debug, Clone, serde::Serialize, ts_rs::TS)]
#[ts(export, export_to = "../../../ui/src/gen/")]
pub struct UsageRow {
    pub agent_id: Option<String>,
    pub model: Option<String>,
    pub stage: Option<String>,
    #[ts(type = "number")] // JS number 域（wire 是 JSON number）
    pub prompt_tokens: i64,
    #[ts(type = "number")] // JS number 域（wire 是 JSON number）
    pub completion_tokens: i64,
    #[ts(type = "number")] // JS number 域（wire 是 JSON number）
    pub tool_output_tokens: i64,
    #[ts(type = "number")] // JS number 域（wire 是 JSON number）
    pub cost_mc: i64,
    #[ts(type = "number")] // JS number 域（wire 是 JSON number）
    pub calls: i64,
}

/// 项目总计：spent/limit/tokens。
#[derive(Debug, Clone, serde::Serialize, ts_rs::TS)]
#[ts(export, export_to = "../../../ui/src/gen/")]
pub struct UsageTotal {
    #[ts(type = "number")] // JS number 域（wire 是 JSON number）
    pub spent_mc: i64,
    #[ts(type = "number | null")]
    pub limit_cents: Option<i64>,
    #[ts(type = "number")] // JS number 域（wire 是 JSON number）
    pub tokens: i64,
}

/// 用量面返回体：明细行 + 总计（原 `_total` 哨兵行已拆——哨兵行正是
/// 本票要消的类型盲区，UI 曾靠 `r._total` 可选字段辨认它）。
#[derive(Debug, Clone, serde::Serialize, ts_rs::TS)]
#[ts(export, export_to = "../../../ui/src/gen/")]
pub struct UsageSummary {
    pub rows: Vec<UsageRow>,
    pub total: UsageTotal,
}

/// 时间序列行：bucket × agent。
#[derive(Debug, Clone, serde::Serialize, ts_rs::TS)]
#[ts(export, export_to = "../../../ui/src/gen/")]
pub struct UsageBucket {
    pub bucket: String,
    pub agent_id: Option<String>,
    #[ts(type = "number")] // JS number 域（wire 是 JSON number）
    pub prompt_tokens: i64,
    #[ts(type = "number")] // JS number 域（wire 是 JSON number）
    pub completion_tokens: i64,
    #[ts(type = "number")] // JS number 域（wire 是 JSON number）
    pub tool_output_tokens: i64,
    #[ts(type = "number")] // JS number 域（wire 是 JSON number）
    pub cost_mc: i64,
}

pub fn summarize(db: &Db, project_id: &str) -> Result<Vec<UsageRow>, rusqlite::Error> {
    let mut st = db.conn().prepare(
        "SELECT u.agent_id, u.model, sr.stage_name,
                SUM(u.prompt_tokens), SUM(u.completion_tokens),
                SUM(u.tool_output_tokens), SUM(u.cost_millicents), COUNT(*)
         FROM usage u LEFT JOIN stage_runs sr ON sr.id = u.stage_run_id
         WHERE u.project_id=?1
         GROUP BY u.agent_id, u.model, u.stage_run_id
         ORDER BY SUM(u.cost_millicents) DESC",
    )?;
    let rows = st
        .query_map([project_id], |r| {
            Ok(UsageRow {
                agent_id: r.get(0)?,
                model: r.get(1)?,
                stage: r.get(2)?,
                prompt_tokens: r.get(3)?,
                completion_tokens: r.get(4)?,
                tool_output_tokens: r.get(5)?,
                cost_mc: r.get(6)?,
                calls: r.get(7)?,
            })
        })?
        .collect::<Result<Vec<_>, _>>()?;
    Ok(rows)
}

/// 项目用量总览（ADR 0052 读组）：summarize 多维汇总 + 总计/上限/tokens 尾行。
/// 壳层经控制连接直调，不占 wb 锁。
pub fn project_summary(db: &Db, project_id: &str) -> Result<UsageSummary, rusqlite::Error> {
    let rows = summarize(db, project_id)?;
    let limit: Option<i64> = db.conn().query_row(
        "SELECT usage_limit_cents FROM projects WHERE id=?1",
        [project_id],
        |r| r.get(0),
    )?;
    let tokens: i64 = db.conn().query_row(
        "SELECT COALESCE(SUM(prompt_tokens+completion_tokens+tool_output_tokens),0)
         FROM usage WHERE project_id=?1",
        [project_id],
        |r| r.get(0),
    )?;
    Ok(UsageSummary {
        rows,
        total: UsageTotal {
            spent_mc: spent_mc(db, project_id)?,
            limit_cents: limit,
            tokens,
        },
    })
}

/// 撞限压力面（.scratch/context-window 票 03 / ADR 0068）：近 14 天
/// `context_overflow` 待决卡与 `context_compacted` 机械压缩事件计数。
/// 恢复层（重建回合/锚定摘要）的触发闸：同项目两周 ≥2 张撞限卡才值得建
/// ——度量先于设计，无数据时不引入静默改写通道。
#[derive(Debug, Clone, serde::Serialize, ts_rs::TS)]
#[ts(export, export_to = "../../../ui/src/gen/")]
pub struct ContextPressure {
    #[ts(type = "number")] // JS number 域（wire 是 JSON number）
    pub overflow_cards_14d: i64,
    #[ts(type = "number")] // JS number 域（wire 是 JSON number）
    pub compactions_14d: i64,
}

/// 近 14 天撞限/压缩计数：直查 events（ADR 0052 读组，不占 wb 锁）。
/// 撞限卡以 escalated+payload.reason 辨认；压缩以 system+payload.kind 辨认。
/// 字符串字面量与写入侧 turn/context.rs 的发卡点同字——改名必须双侧同改
/// （D10 常量化是挂账的未做面）。注意：超限载荷落 spill 指针的事件
/// json_extract 取不到字段——撞限/压缩载荷天然小（远低于
/// EVENT_PAYLOAD_CAP），不计失真。
pub fn context_pressure(db: &Db, project_id: &str) -> Result<ContextPressure, rusqlite::Error> {
    let (overflow, compacted): (i64, i64) = db.conn().query_row(
        "SELECT
            COUNT(*) FILTER (WHERE kind='escalated'
                AND json_extract(payload,'$.reason')='context_overflow'),
            COUNT(*) FILTER (WHERE kind='system'
                AND json_extract(payload,'$.kind')='context_compacted')
         FROM events
         WHERE project_id=?1 AND created_at >= datetime('now','-14 days')",
        [project_id],
        |r| Ok((r.get(0)?, r.get(1)?)),
    )?;
    Ok(ContextPressure {
        overflow_cards_14d: overflow,
        compactions_14d: compacted,
    })
}

/// 用量时间序列：按 bucket × Agent 分组，带三类 token 与成本。
/// `granularity`: "day"（YYYY-MM-DD）| "hour"（YYYY-MM-DD HH:00）。
/// `from`/`to`：日期串 YYYY-MM-DD（含当天，`to` 含整日）；None = 不限。
pub fn series(
    db: &Db,
    project_id: &str,
    granularity: &str,
    from: Option<&str>,
    to: Option<&str>,
) -> Result<Vec<UsageBucket>, rusqlite::Error> {
    let bucket = if granularity == "hour" {
        "strftime('%Y-%m-%d %H:00', u.created_at)"
    } else {
        "date(u.created_at)"
    };
    let sql = format!(
        "SELECT {bucket}, u.agent_id,
                SUM(u.prompt_tokens), SUM(u.completion_tokens),
                SUM(u.tool_output_tokens), SUM(u.cost_millicents)
         FROM usage u
         WHERE u.project_id=?1
           AND (?2 IS NULL OR u.created_at >= ?2)
           AND (?3 IS NULL OR u.created_at < date(?3, '+1 day'))
         GROUP BY 1, u.agent_id ORDER BY 1"
    );
    let mut st = db.conn().prepare(&sql)?;
    let rows = st
        .query_map(params![project_id, from, to], |r| {
            Ok(UsageBucket {
                bucket: r.get(0)?,
                agent_id: r.get(1)?,
                prompt_tokens: r.get(2)?,
                completion_tokens: r.get(3)?,
                tool_output_tokens: r.get(4)?,
                cost_mc: r.get(5)?,
            })
        })?
        .collect::<Result<Vec<_>, _>>()?;
    Ok(rows)
}

/// 设/清项目用量上限（分）；None = 不限。控制通道写（ADR 0052）。
pub fn set_limit(
    db: &Db,
    project_id: &str,
    limit_cents: Option<i64>,
) -> Result<(), rusqlite::Error> {
    db.conn().execute(
        "UPDATE projects SET usage_limit_cents=?1 WHERE id=?2",
        params![limit_cents, project_id],
    )?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::Db;

    fn setup(limit_cents: Option<i64>) -> (Db, ToolContext, tempfile::TempDir) {
        let dir = tempfile::tempdir().unwrap();
        let db = Db::open_in_memory().unwrap();
        db.conn()
            .execute(
                "INSERT INTO projects (id, dir, name, mode, usage_limit_cents)
                 VALUES ('p','/tmp/x','x','pack',?1)",
                params![limit_cents],
            )
            .unwrap();
        db.conn()
            .execute(
                "INSERT INTO agents (id, project_id, role, status) VALUES
                 ('a0','p','产品策划','active'),('a1','p','后端','active')",
                [],
            )
            .unwrap();
        let ctx = ToolContext {
            project_id: "p".into(),
            agent_id: "a0".into(),
            repo_root: dir.path().to_path_buf(),
            stage_run_id: None,
            owned_globs: vec![],
            tiers: Default::default(),
            sessions: Default::default(),
            caps: Default::default(),
            ..Default::default()
        };
        (db, ctx, dir)
    }

    #[test]
    fn context_pressure_counts_overflow_cards_and_compactions() {
        let (db, _ctx, _d) = setup(None);
        // 无关事件不计入：别的升级卡、别的 system subkind
        db.append_event(
            "p",
            EventKind::System,
            json!({"kind": "steering_injected"}),
            None,
            None,
        )
        .unwrap();
        db.append_event(
            "p",
            EventKind::Escalated,
            json!({"reason": "flag", "sub": "x"}),
            None,
            None,
        )
        .unwrap();
        // 撞限卡（payload.reason）+ 机械压缩（payload.kind）
        db.append_event(
            "p",
            EventKind::Escalated,
            json!({"reason": "context_overflow", "code": "budget-exceeded"}),
            Some("a1"),
            None,
        )
        .unwrap();
        db.append_event(
            "p",
            EventKind::System,
            json!({"kind": "context_compacted", "removed": 3}),
            Some("a1"),
            None,
        )
        .unwrap();
        // 14 天外的不计
        let old = db
            .append_event(
                "p",
                EventKind::Escalated,
                json!({"reason": "context_overflow"}),
                None,
                None,
            )
            .unwrap();
        db.conn()
            .execute(
                "UPDATE events SET created_at=datetime('now','-20 days') WHERE id=?1",
                [old],
            )
            .unwrap();
        let p = context_pressure(&db, "p").unwrap();
        assert_eq!((p.overflow_cards_14d, p.compactions_14d), (1, 1));
        // 无事件项目 → 0 不报错
        let z = context_pressure(&db, "ghost").unwrap();
        assert_eq!((z.overflow_cards_14d, z.compactions_14d), (0, 0));
    }

    #[test]
    fn records_and_summarizes() {
        let (db, ctx, _d) = setup(None);
        let u = Usage {
            prompt_tokens: 1000,
            completion_tokens: 500,
        };
        record(&db, &ctx, "chat", &u, 400).unwrap();
        record(&db, &ctx, "chat", &u, 0).unwrap();
        let rows: Vec<serde_json::Value> = summarize(&db, "p")
            .unwrap()
            .iter()
            .map(|r| serde_json::to_value(r).unwrap())
            .collect();
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0]["prompt_tokens"], 2000);
        assert_eq!(rows[0]["tool_output_tokens"], 100); // 400B / 4
        assert_eq!(rows[0]["calls"], 2);
    }

    #[test]
    fn summarize_groups_by_stage() {
        let (db, ctx, _d) = setup(None);
        db.conn()
            .execute(
                "INSERT INTO stage_runs (id, project_id, stage_name, seq)
                 VALUES ('sr1','p','接口',1),('sr2','p','实现',2)",
                [],
            )
            .unwrap();
        let u = Usage {
            prompt_tokens: 100,
            completion_tokens: 0,
        };
        let mut ctx2 = ToolContext {
            stage_run_id: Some("sr1".into()),
            ..ctx
        };
        record(&db, &ctx2, "chat", &u, 0).unwrap();
        ctx2.stage_run_id = Some("sr2".into());
        record(&db, &ctx2, "chat", &u, 0).unwrap();
        record(&db, &ctx2, "chat", &u, 0).unwrap(); // sr2 两笔
        let rows: Vec<serde_json::Value> = summarize(&db, "p")
            .unwrap()
            .iter()
            .map(|r| serde_json::to_value(r).unwrap())
            .collect();
        assert_eq!(rows.len(), 2);
        let stages: std::collections::BTreeSet<_> =
            rows.iter().map(|r| r["stage"].as_str().unwrap()).collect();
        assert_eq!(stages.iter().copied().collect::<Vec<_>>(), ["实现", "接口"]);
        let impl_row = rows.iter().find(|r| r["stage"] == "实现").unwrap();
        assert_eq!(impl_row["calls"], 2);
        assert_eq!(impl_row["prompt_tokens"], 200);
    }

    #[test]
    fn cost_from_prices_json() {
        let (db, ctx, d) = setup(None);
        std::fs::create_dir_all(d.path().join(".hexagon")).unwrap();
        std::fs::write(
            d.path().join(".hexagon/prices.json"),
            r#"{"default":{"prompt_per_1k_mc":10,"completion_per_1k_mc":30}}"#,
        )
        .unwrap();
        record(
            &db,
            &ctx,
            "chat",
            &Usage {
                prompt_tokens: 1000,
                completion_tokens: 1000,
            },
            0,
        )
        .unwrap();
        assert_eq!(spent_mc(&db, "p").unwrap(), 40); // 10+30 毫分
    }

    #[test]
    fn cap_sleeps_team_and_blocks() {
        let (db, ctx, d) = setup(Some(1)); // 上限 1 分 = 1000 毫分
        std::fs::create_dir_all(d.path().join(".hexagon")).unwrap();
        std::fs::write(
            d.path().join(".hexagon/prices.json"),
            r#"{"default":{"prompt_per_1k_mc":2000,"completion_per_1k_mc":0}}"#,
        )
        .unwrap();
        record(
            &db,
            &ctx,
            "chat",
            &Usage {
                prompt_tokens: 1000, // 2000 毫分 = 2 分 > 1 分上限
                completion_tokens: 0,
            },
            0,
        )
        .unwrap();
        assert!(enforce_cap(&db, "p").unwrap());
        // 全员已休眠
        let sleeping: i64 = db
            .conn()
            .query_row(
                "SELECT COUNT(*) FROM agents WHERE project_id='p' AND status='sleeping'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(sleeping, 2);
        // 事件落了；再次 enforce 仍是闸但不重复 TeamSlept
        let hits: i64 = db
            .conn()
            .query_row(
                "SELECT COUNT(*) FROM events WHERE project_id='p' AND kind='usage_cap_hit'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(hits, 1);
        assert!(enforce_cap(&db, "p").unwrap());
        let slept: i64 = db
            .conn()
            .query_row(
                "SELECT COUNT(*) FROM events WHERE project_id='p' AND kind='team_slept'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(slept, 1);
    }

    #[test]
    fn no_limit_never_caps() {
        let (db, ctx, _d) = setup(None);
        record(
            &db,
            &ctx,
            "chat",
            &Usage {
                prompt_tokens: 999_999,
                completion_tokens: 0,
            },
            0,
        )
        .unwrap();
        assert!(!enforce_cap(&db, "p").unwrap());
    }
}

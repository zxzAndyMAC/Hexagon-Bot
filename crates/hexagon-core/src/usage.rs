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
        .or_else(|| v.get("default"));
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
    db.conn().execute(
        "UPDATE agents SET status='sleeping' WHERE project_id=?1",
        [project_id],
    )?;
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
pub fn summarize(db: &Db, project_id: &str) -> Result<Vec<Value>, rusqlite::Error> {
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
            Ok(json!({
                "agent_id": r.get::<_, Option<String>>(0)?,
                "model": r.get::<_, Option<String>>(1)?,
                "stage": r.get::<_, Option<String>>(2)?,
                "prompt_tokens": r.get::<_, i64>(3)?,
                "completion_tokens": r.get::<_, i64>(4)?,
                "tool_output_tokens": r.get::<_, i64>(5)?,
                "cost_mc": r.get::<_, i64>(6)?,
                "calls": r.get::<_, i64>(7)?,
            }))
        })?
        .collect::<Result<Vec<_>, _>>()?;
    Ok(rows)
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
        };
        (db, ctx, dir)
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
        let rows = summarize(&db, "p").unwrap();
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
        let rows = summarize(&db, "p").unwrap();
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

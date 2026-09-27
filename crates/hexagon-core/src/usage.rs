//! 用量账本（票 12）：计量、汇总、上限硬闸。
//!
//! - 每次模型实际派发先记 request 身份，结束后记录可得用量及状态。
//!   工具输出字节/4 独立记账，不计入请求数；旧不可分类行保留 legacy。
//! - `.hexagon/prices.json` 的缺失、损坏、不匹配与缺少供应商用量均
//!   标记未知费用；数字成本只是已知部分的本地估算，不把未知说成免费。
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
fn price_for(repo_root: &std::path::Path, model_slot: &str) -> Option<Price> {
    let text = std::fs::read_to_string(repo_root.join(".hexagon/prices.json")).ok()?;
    let v: Value = serde_json::from_str(&text).ok()?;
    let entry = v
        .get("models")
        .and_then(|m| m.get(model_slot))
        .or_else(|| v.get("default"))?; // D13-exempt: explicit price-table default, not a model-slot fallback
    let prompt = entry["prompt_per_1k_mc"].as_i64()?;
    let completion = entry["completion_per_1k_mc"].as_i64()?;
    (prompt >= 0 && completion >= 0).then_some(Price {
        prompt_per_1k_mc: prompt,
        completion_per_1k_mc: completion,
    })
}

fn cost_for(price: Price, usage: &Usage) -> Option<i64> {
    // Reliability 12: malformed supplier counts can overflow even i128 when
    // both products are added. Unknown costs extra reconciliation; wrapped
    // cheap/negative costs can bypass a real budget. Never wrap or panic.
    let prompt = (usage.prompt_tokens as i128).checked_mul(price.prompt_per_1k_mc as i128)?;
    let completion =
        (usage.completion_tokens as i128).checked_mul(price.completion_per_1k_mc as i128)?;
    let cost = prompt.checked_add(completion)? / 1000;
    i64::try_from(cost).ok()
}

// Reliability 13 / D06: false negatives postpone a request; false positives
// can dispatch unbudgeted work. Compare in i128, reject exhausted known spend,
// and never wrap a cents limit or summed reservations into an affordable value.
fn reservation_fits(spent: i64, held: i64, needed: i64, limit_cents: i64) -> bool {
    let limit = i128::from(limit_cents) * 1000;
    i128::from(spent) < limit && i128::from(spent) + i128::from(held) + i128::from(needed) <= limit
}

fn request_lock_path(root: &std::path::Path, id: &str) -> std::path::PathBuf {
    root.join(".hexagon/request-locks")
        .join(format!("{id}.lock"))
}

fn mark_request_unknown(db: &Db, project: &str, id: &str) -> Result<(), rusqlite::Error> {
    let started = std::time::Instant::now();
    let changed = db.conn().execute("UPDATE usage SET request_state='outcome_unknown',reserved_mc=0,cost_known=0 WHERE project_id=?1 AND request_id=?2 AND request_state='pending'",params![project,id])?;
    if changed > 0 {
        let (agent, activation): (Option<String>, Option<String>) = db.conn().query_row(
            "SELECT agent_id,stage_run_id FROM usage WHERE project_id=?1 AND request_id=?2",
            params![project, id],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )?;
        crate::diag::note(
            crate::diag::CLASS_REJECT,
            true,
            Some(project),
            agent.as_deref(),
            activation.as_deref(),
            None,
            "request_recovery",
            // Request identity is not an event/trace ID. Keep its explicit
            // namespace in the reason detail, without any request contents.
            &format!("unsettled_request_cost_unknown:{id}"),
            started,
        );
    }
    Ok(())
}

struct PendingRequest<'a> {
    db: &'a Db,
    project: &'a str,
    id: &'a str,
    _lock: std::fs::File,
}

impl Drop for PendingRequest<'_> {
    fn drop(&mut self) {
        // Reliability 13: unwinding after dispatch is not evidence of a free
        // request. Release only the estimate and retain an unknown charge.
        // Successful settlement already changed state, so this CAS is a no-op.
        let _ = mark_request_unknown(self.db, self.project, self.id);
    }
}

/// Reconcile stale local reservations without replaying a model request. A
/// live OS lock is stronger evidence than a timeout or a reusable process ID.
pub fn recover_requests(
    db: &Db,
    root: &std::path::Path,
    project: &str,
) -> Result<(), rusqlite::Error> {
    let rows = {
        let mut st = db.conn().prepare("SELECT request_id,reserved_mc FROM usage WHERE project_id=?1 AND record_kind='request' AND request_state='pending'")?;
        let rows = st
            .query_map([project], |r| {
                Ok((r.get::<_, String>(0)?, r.get::<_, i64>(1)?))
            })?
            .collect::<Result<Vec<_>, _>>()?;
        rows
    };
    for (id, reserved) in rows {
        // IDs are host-generated. Corrupt imported ledger text is never a path.
        if !id
            .strip_prefix("request")
            .is_some_and(|n| !n.is_empty() && n.bytes().all(|c| c.is_ascii_digit()))
        {
            continue;
        }
        let path = request_lock_path(root, &id);
        match std::fs::OpenOptions::new()
            .read(true)
            .write(true)
            .open(&path)
        {
            Ok(file) => match file.try_lock() {
                Ok(()) => mark_request_unknown(db, project, &id)?,
                Err(std::fs::TryLockError::WouldBlock) => (),
                Err(std::fs::TryLockError::Error(_)) => (), // Cannot prove abandonment: retain the reservation.
            },
            // Pre-13 pending requests have no reservation/lock to preserve.
            Err(e) if e.kind() == std::io::ErrorKind::NotFound && reserved == 0 => {
                mark_request_unknown(db, project, &id)?
            }
            Err(_) => (),
        }
    }
    Ok(())
}

/// One row per dispatch attempt, before provider IO. Repeated tool-output rows
/// are separate records, never model calls. Unknown prices do not imply zero.
pub fn complete_project_request(
    ctx: &ToolContext,
    provider: &dyn crate::provider::ModelProvider,
    req: &crate::provider::ChatRequest,
    purpose: &str,
) -> Result<crate::provider::ChatResponse, crate::provider::ProviderError> {
    // Reliability 12 / D01: opening intake must not hold the Workbench or
    // control-connection mutex during network IO. Use its own project ledger.
    let db = Db::open(ctx.repo_root.join(".hexagon/state.db"))
        .map_err(|e| crate::provider::ProviderError::Refused(format!("request ledger: {e}")))?;
    request(
        &db,
        ctx,
        &req.model_slot,
        purpose,
        provider,
        Some(req),
        || provider.complete(req),
    )
}

pub fn request(
    db: &Db,
    ctx: &ToolContext,
    model_slot: &str,
    purpose: &str,
    provider: &dyn crate::provider::ModelProvider,
    chat: Option<&crate::provider::ChatRequest>,
    send: impl FnOnce() -> Result<crate::provider::ChatResponse, crate::provider::ProviderError>,
) -> Result<crate::provider::ChatResponse, crate::provider::ProviderError> {
    use crate::provider::ProviderError;
    let started = std::time::Instant::now();
    let _evaluation_work = crate::evaluation::control::work_lease(&ctx.repo_root)
        .map_err(|_| ProviderError::Interrupted)?;
    let ledger_error = |e: String| ProviderError::Refused(format!("request ledger: {e}"));
    recover_requests(db, &ctx.repo_root, &ctx.project_id)
        .map_err(|e| ledger_error(e.to_string()))?;
    // Reliability 13: admission and reservation share one SQLite writer
    // transaction. Separate reads let all four workers spend the same balance.
    // No transaction remains open during provider IO.
    let tx =
        rusqlite::Transaction::new_unchecked(db.conn(), rusqlite::TransactionBehavior::Immediate)
            .map_err(|e| ledger_error(e.to_string()))?;
    if enforce_cap(db, &ctx.project_id).map_err(|e| ledger_error(e.to_string()))? {
        crate::diag::note(
            crate::diag::CLASS_REJECT,
            true,
            Some(&ctx.project_id),
            Some(&ctx.agent_id),
            ctx.stage_run_id.as_deref(),
            None,
            "model_request",
            "known_budget_exhausted",
            started,
        );
        tx.commit().map_err(|e| ledger_error(e.to_string()))?;
        return Err(ProviderError::BudgetUnavailable);
    }
    let id = format!(
        "request{}",
        db.next_id("model_request")
            .map_err(|e| ledger_error(e.to_string()))?
    );
    let price = price_for(&ctx.repo_root, model_slot);
    let mut reserved_mc = 0;
    let mut estimated_prompt = None;
    let mut output_limit = None;
    if let (Some(price), Some(req), Some(output)) = (price, chat, provider.output_token_limit()) {
        let input = serde_json::to_string(&(&req.messages, &req.tools))
            .map_err(|e| ledger_error(e.to_string()))?;
        let estimate = crate::provider::Usage {
            prompt_tokens: tiktoken_rs::cl100k_base_singleton()
                .encode_ordinary(&input)
                .len() as u64,
            completion_tokens: output,
            prompt_reported: true,
            completion_reported: true,
            ..Default::default()
        };
        reserved_mc = cost_for(price, &estimate).unwrap_or(i64::MAX);
        estimated_prompt = Some(estimate.prompt_tokens.min(i64::MAX as u64) as i64);
        output_limit = Some(output.min(i64::MAX as u64) as i64);
        let limit: Option<i64> = db
            .conn()
            .query_row(
                "SELECT usage_limit_cents FROM projects WHERE id=?1",
                [&ctx.project_id],
                |r| r.get(0),
            )
            .map_err(|e| ledger_error(e.to_string()))?;
        if let Some(limit) = limit {
            let spent = spent_mc(db, &ctx.project_id).map_err(|e| ledger_error(e.to_string()))?;
            let in_flight: i64 = db.conn().query_row("SELECT COALESCE(SUM(reserved_mc),0) FROM usage WHERE project_id=?1 AND request_state='pending'", [&ctx.project_id], |r| r.get(0)).map_err(|e| ledger_error(e.to_string()))?;
            if !reservation_fits(spent, in_flight, reserved_mc, limit) {
                crate::diag::note(
                    crate::diag::CLASS_REJECT,
                    true,
                    Some(&ctx.project_id),
                    Some(&ctx.agent_id),
                    ctx.stage_run_id.as_deref(),
                    None,
                    "request_budget",
                    "insufficient_estimated_balance",
                    started,
                );
                return Err(ProviderError::BudgetUnavailable);
            }
        }
    }
    crate::diag::note(
        crate::diag::CLASS_JUDGE,
        false,
        Some(&ctx.project_id),
        Some(&ctx.agent_id),
        ctx.stage_run_id.as_deref(),
        None,
        "model_request",
        if price.is_some() {
            "price_available"
        } else {
            "price_unknown_continue"
        },
        started,
    );
    let basis = price.map(|p|json!({"model_slot":model_slot,"prompt_per_1k_mc":p.prompt_per_1k_mc,"completion_per_1k_mc":p.completion_per_1k_mc}).to_string());
    let lock_path = request_lock_path(&ctx.repo_root, &id);
    let lock_dir = lock_path.parent().expect("request lock has a parent");
    if lock_dir
        .symlink_metadata()
        .is_ok_and(|m| m.file_type().is_symlink())
        || lock_path
            .symlink_metadata()
            .is_ok_and(|m| m.file_type().is_symlink())
    {
        return Err(ledger_error("request lock path is a symlink".into()));
    }
    std::fs::create_dir_all(lock_dir).map_err(|e| ledger_error(e.to_string()))?;
    let lock = std::fs::OpenOptions::new()
        .read(true)
        .write(true)
        .create(true)
        .truncate(false)
        .open(lock_path)
        .map_err(|e| ledger_error(e.to_string()))?;
    lock.try_lock().map_err(|e| ledger_error(e.to_string()))?;
    db.conn().execute("INSERT INTO usage(project_id,agent_id,model,prompt_tokens,completion_tokens,tool_output_tokens,cost_millicents,stage_run_id,record_kind,request_id,purpose,request_state,price_basis,cost_known,actual_model) VALUES (?1,?2,?3,0,0,0,0,?4,'request',?5,?6,'pending',?7,0,?8)",params![ctx.project_id,ctx.agent_id,model_slot,ctx.stage_run_id,id,purpose,basis,provider.billing_model()]).map_err(|e|ledger_error(e.to_string()))?;
    db.conn().execute("UPDATE usage SET reserved_mc=?2,estimated_prompt_tokens=?3,output_limit=?4 WHERE request_id=?1",params![id,reserved_mc,estimated_prompt,output_limit]).map_err(|e|ledger_error(e.to_string()))?;
    tx.commit().map_err(|e| ledger_error(e.to_string()))?;
    let _pending = PendingRequest {
        db,
        project: &ctx.project_id,
        id: &id,
        _lock: lock,
    };
    // Evaluation D08: commit the local attempt before admission, but do not
    // dispatch until the shared authority has reserved this actual request.
    let mut evaluation = match crate::evaluation::budget::admit(ctx, provider, &id, purpose) {
        Ok(guard) => guard,
        Err(error) => {
            let _ = crate::evaluation::control::budget_refused(&ctx.repo_root);
            crate::diag::note(
                crate::diag::CLASS_REJECT,
                true,
                Some(&ctx.project_id),
                Some(&ctx.agent_id),
                ctx.stage_run_id.as_deref(),
                None,
                "evaluation_budget_admission",
                &format!("admission_refused:{id}"),
                started,
            );
            db.conn().execute("UPDATE usage SET request_state='not_sent',reserved_mc=0,cost_known=1 WHERE request_id=?1 AND request_state='pending'", [&id]).map_err(|e| ledger_error(e.to_string()))?;
            return Err(ledger_error(error.to_string()));
        }
    };
    // D09: the ledger writer can wait behind another connection. Recheck
    // after that wait, before marking this request dispatched.
    if crate::evaluation::control::checkpoint(&ctx.repo_root).is_err() {
        db.conn().execute("UPDATE usage SET request_state='not_sent',reserved_mc=0,cost_known=1 WHERE request_id=?1 AND request_state='pending'",[&id]).map_err(|e|ledger_error(e.to_string()))?;
        return Err(ProviderError::Interrupted);
    }
    if let Some(guard) = &mut evaluation {
        guard.dispatch().map_err(|e| ledger_error(e.to_string()))?;
    }
    #[cfg(test)]
    crate::evaluation::recovery::crash_at(crate::evaluation::recovery::CrashPoint::Reserved);
    let _watch =
        crate::evaluation::control::watch(&ctx.repo_root, ctx.sessions.clone(), ctx.tasks.clone())
            .map_err(|e| ledger_error(e.to_string()))?;
    let result = send();
    #[cfg(test)]
    crate::evaluation::recovery::crash_at(crate::evaluation::recovery::CrashPoint::Returned);
    let usage = match &result {
        Ok(response) => Some(&response.usage),
        Err(error) => error.usage(),
    };
    let cost = usage
        .filter(|u| u.prompt_reported || u.completion_reported)
        .and_then(|u| price.and_then(|p| cost_for(p, u)));
    // Native service tools can be billed separately even when their usage
    // counters are missing. Preserve the priced portion, flag the remainder.
    let extra_unpriced = usage.is_some_and(|u| u.unpriced) || result.as_ref().is_ok_and(|r| r.content.iter().any(|b| matches!(b,
        crate::provider::ContentBlock::Opaque { raw } if matches!(raw["type"].as_str(), Some("server_tool_use" | "web_search_tool_result"))
    )));
    let unpriced =
        extra_unpriced || usage.is_some_and(|u| !u.prompt_reported || !u.completion_reported);
    let state = match &result {
        Ok(_) => "succeeded",
        Err(error) if matches!(error.cause(), ProviderError::Interrupted) => "interrupted",
        Err(error) if matches!(error.cause(), ProviderError::MissingCredential(_)) => "not_sent",
        Err(_) => "failed",
    };
    db.conn().execute("UPDATE usage SET reserved_mc=0,prompt_tokens=?1,completion_tokens=?2,cost_millicents=?3,cost_known=?4,request_state=?5,prompt_known=?7,completion_known=?8 WHERE request_id=?6 AND request_state='pending'",params![usage.map(|u|u.prompt_tokens.min(i64::MAX as u64) as i64).unwrap_or(0),usage.map(|u|u.completion_tokens.min(i64::MAX as u64) as i64).unwrap_or(0),cost.unwrap_or(0),cost.is_some() && !unpriced,state,id,usage.is_some_and(|u|u.prompt_reported),usage.is_some_and(|u|u.completion_reported)]).map_err(|e|ledger_error(e.to_string()))?;
    if let Some(guard) = &evaluation {
        guard
            .settle(usage, result.is_ok(), state == "not_sent", extra_unpriced)
            .map_err(|e| ledger_error(e.to_string()))?;
    }
    #[cfg(test)]
    crate::evaluation::recovery::crash_at(crate::evaluation::recovery::CrashPoint::Persisted);
    // D09: a tool-only response has no text delta at which streaming could
    // notice stop. Settle actual usage, then refuse the late tool response.
    if crate::evaluation::control::checkpoint(&ctx.repo_root).is_err() {
        return Err(ProviderError::Interrupted);
    }
    result.map_err(ProviderError::into_cause)
}

/// 记一行账。`tool_output_bytes` 是本轮工具结果合计字节数，按 /4 估 token。
pub fn record(
    db: &Db,
    ctx: &ToolContext,
    model_slot: &str,
    usage: &Usage,
    tool_output_bytes: usize,
) -> Result<(), rusqlite::Error> {
    let cost = price_for(&ctx.repo_root, model_slot).and_then(|p| cost_for(p, usage));
    db.conn().execute(
        "INSERT INTO usage (project_id, agent_id, model, prompt_tokens,
                            completion_tokens, tool_output_tokens, cost_millicents,
                            stage_run_id,record_kind,cost_known)
         VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10)",
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
            cost.unwrap_or(0),
            ctx.stage_run_id,
            // Reliability 12: this compatibility API mixes model and tool
            // counters; nonempty tool output cannot prove it is a tool-only row.
            // Only record_tools may assert that; keep actual request count unknown.
            "legacy",
            cost.is_some(),
        ],
    )?;
    Ok(())
}

/// Tool-output metering is deliberately not a request, including empty output.
pub fn record_tools(
    db: &Db,
    ctx: &ToolContext,
    model_slot: &str,
    bytes: usize,
) -> Result<(), rusqlite::Error> {
    db.conn().execute("INSERT INTO usage(project_id,agent_id,model,prompt_tokens,completion_tokens,tool_output_tokens,cost_millicents,stage_run_id,record_kind,cost_known) VALUES (?1,?2,?3,0,0,?4,0,?5,'tool',1)",params![ctx.project_id,ctx.agent_id,model_slot,(bytes/4).min(i64::MAX as usize) as i64,ctx.stage_run_id])?;
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
    if i128::from(spent_mc(db, project_id)?) < i128::from(limit_cents) * 1000 {
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
    #[ts(type = "number")]
    pub unknown_requests: i64,
    #[ts(type = "number")]
    pub legacy_unknown_records: i64,
    #[ts(type = "number")]
    pub unknown_token_records: i64,
}

/// 项目总计：spent/limit/tokens。
#[derive(Debug, Clone, serde::Serialize, ts_rs::TS)]
#[ts(export, export_to = "../../../ui/src/gen/")]
pub struct UsageTotal {
    /// Reliability 13: in-flight estimate, never a settled supplier charge.
    #[ts(type = "number")]
    pub reserved_mc: i64,
    #[ts(type = "number")] // JS number 域（wire 是 JSON number）
    pub spent_mc: i64,
    #[ts(type = "number | null")]
    pub limit_cents: Option<i64>,
    #[ts(type = "number")] // JS number 域（wire 是 JSON number）
    pub tokens: i64,
    #[ts(type = "number")]
    pub unknown_requests: i64,
    #[ts(type = "number")]
    pub legacy_unknown_records: i64,
    #[ts(type = "number")]
    pub unknown_token_records: i64,
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
                SUM(u.tool_output_tokens), SUM(u.cost_millicents),
                SUM(u.record_kind='request' AND u.request_state!='not_sent'),
                SUM(u.record_kind='request' AND u.cost_known=0 AND u.request_state!='not_sent'),
                SUM(u.record_kind='legacy'),
                SUM(u.record_kind='legacy' OR (u.record_kind='request' AND u.request_state!='not_sent' AND (u.prompt_known=0 OR u.completion_known=0)))
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
                unknown_requests: r.get(8)?,
                legacy_unknown_records: r.get(9)?,
                unknown_token_records: r.get(10)?,
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
    let unknown_requests = rows.iter().map(|r| r.unknown_requests).sum();
    let legacy_unknown_records = rows.iter().map(|r| r.legacy_unknown_records).sum();
    let unknown_token_records = rows.iter().map(|r| r.unknown_token_records).sum();
    Ok(UsageSummary {
        rows,
        total: UsageTotal {
            reserved_mc: db.conn().query_row("SELECT COALESCE(SUM(reserved_mc),0) FROM usage WHERE project_id=?1 AND request_state='pending'", [project_id], |r| r.get(0))?,
            spent_mc: spent_mc(db, project_id)?,
            limit_cents: limit,
            tokens,
            unknown_requests,
            legacy_unknown_records,
            unknown_token_records,
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
            unpriced: false,
            prompt_reported: true,
            completion_reported: true,
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
                                                        // D06 / 12: both mixed rows remain legacy, even with tool-output bytes;
                                                        // neither provides evidence of an actual individual model request.
        assert_eq!(rows[0]["calls"], 0);
        assert_eq!(rows[0]["legacy_unknown_records"], 2);
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
            unpriced: false,
            prompt_reported: true,
            completion_reported: true,
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
        // D06 / 12: keep historical grouping without fabricating dispatches.
        assert_eq!(impl_row["calls"], 0);
        assert_eq!(impl_row["legacy_unknown_records"], 2);
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
                unpriced: false,
                prompt_reported: true,
                completion_reported: true,
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
                unpriced: false,
                prompt_reported: true,
                completion_reported: true,
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
                unpriced: false,
                prompt_reported: true,
                completion_reported: true,
                prompt_tokens: 999_999,
                completion_tokens: 0,
            },
            0,
        )
        .unwrap();
        assert!(!enforce_cap(&db, "p").unwrap());
    }

    proptest::proptest! {
        #[test]
        fn request_reservation_is_monotone_and_never_admits_exhausted_balance(
            limit in 1i64..=i64::MAX,
            held in 0i64..=i64::MAX,
            needed in 0i64..=i64::MAX,
            extra in 0i64..=i64::MAX,
        ) {
            proptest::prop_assert!(!reservation_fits(i64::MAX, held, needed, 1));
            proptest::prop_assert!(reservation_fits(0, 0, 0, limit));
            if reservation_fits(0, held.saturating_add(extra), needed, limit) {
                proptest::prop_assert!(reservation_fits(0, held, needed, limit));
            }
            if reservation_fits(0, held, needed.saturating_add(extra), limit) {
                proptest::prop_assert!(reservation_fits(0, held, needed, limit));
            }
            // Huge positive limits and costs stay positive in the wide domain.
            proptest::prop_assert!(reservation_fits(0, i64::MAX, i64::MAX, i64::MAX));
            proptest::prop_assert!(!reservation_fits(1000, 0, 0, 1));
        }

        #[test]
        fn cost_estimates_never_overflow_on_supplier_counts(
            prompt in (u64::MAX - 1024)..=u64::MAX,
            completion in (u64::MAX - 1024)..=u64::MAX,
        ) {
            let usage = Usage { unpriced: false, prompt_reported: true, completion_reported: true, prompt_tokens: prompt, completion_tokens: completion };
            let price = Price { prompt_per_1k_mc: i64::MAX, completion_per_1k_mc: i64::MAX };
            // Both terms far exceed the ledger range. Unknown is required;
            // wrapping into a small or negative amount would bypass the cap.
            proptest::prop_assert_eq!(cost_for(price, &usage), None);
        }
    }
}

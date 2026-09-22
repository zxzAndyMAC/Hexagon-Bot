//! 上下文窗口管理（turn 内核的「装得下什么」侧）：
//! 估算 → 轻量裁剪（spill）→ 机械降级（transcript）→ 撞限升级卡，
//! 以及消息向量卫生（悬空 tool_use 修复）。

use crate::db::Db;
use crate::provider::{ContentBlock, Message, Role};
use crate::tools::{ToolContext, ToolError};
use crate::trace::EventKind;
use crate::turn::{TurnError, TurnOutcome};
use serde_json::json;

/// 上下文估算上限（US37）：~120k tok；超了轻量裁剪后仍超 → 暂停问负责人。
pub(super) const CONTEXT_CAP_TOKENS: usize = 120_000;
/// 轻量裁剪的单块上限（字符）：超长 tool_result 截断带标记，其余不动。
const TRIM_BLOCK_CHARS: usize = 4_000;

/// 悬空 tool_use 修复（票 13，OPE `_repair_dangling_tool_calls`）：
/// assistant 消息里没等到结果的每个 ToolUse，紧跟其后补 error
/// tool_result stub——Anthropic 形状 provider 拒收孤儿 tool_use。
/// stub 如实写「中断无结果」，模型需要可自行重发。
/// 截断续推路径的配对保证也走这里：截断回复里的 tool_use 不执行。
/// 返回补的 stub 数。
pub fn repair_dangling_tool_uses(messages: &mut Vec<Message>) -> usize {
    let answered: std::collections::HashSet<String> = messages
        .iter()
        .flat_map(|m| m.content.iter())
        .filter_map(|b| match b {
            ContentBlock::ToolResult { tool_use_id, .. } => Some(tool_use_id.clone()),
            _ => None,
        })
        .collect();
    let mut stubs_made = 0usize;
    let mut i = 0;
    while i < messages.len() {
        let dangling: Vec<String> = if matches!(messages[i].role, Role::Assistant) {
            messages[i]
                .content
                .iter()
                .filter_map(|b| match b {
                    ContentBlock::ToolUse { id, .. } if !answered.contains(id) => Some(id.clone()),
                    _ => None,
                })
                .collect()
        } else {
            vec![]
        };
        if !dangling.is_empty() {
            stubs_made += dangling.len();
            let stubs: Vec<ContentBlock> = dangling
                .into_iter()
                .map(|id| ContentBlock::ToolResult {
                    tool_use_id: id,
                    content: "{\"error\":\"tool call interrupted — no result recorded; \
                              re-issue the call if it is still needed\"}"
                        .into(),
                    is_error: true,
                    images: vec![],
                })
                .collect();
            // 后续已有 Tool 消息则并入其头部，否则插一条新 Tool 消息——
            // 保持「assistant → tool_result」紧邻序。
            if i + 1 < messages.len() && matches!(messages[i + 1].role, Role::Tool) {
                let mut merged = stubs;
                merged.append(&mut messages[i + 1].content);
                messages[i + 1].content = merged;
            } else {
                messages.insert(
                    i + 1,
                    Message {
                        role: Role::Tool,
                        content: stubs,
                    },
                );
            }
        }
        i += 1;
    }
    stubs_made
}

/// 模型可自救的工具错 → is_error 回喂；基建错（trace/db/sqlite）上抛。
pub(super) fn model_visible(e: &ToolError) -> bool {
    matches!(
        e,
        ToolError::PathEscape(_)
            | ToolError::Io(_)
            | ToolError::BadInput(_)
            | ToolError::Exec(_)
            | ToolError::Artifact(_)
            | ToolError::UnknownQuestion(_)
    )
}

/// 上下文估算（US37）：按 ~4 字符/tok 粗算，只用于撞限判断不用于计费。
pub(super) fn estimate_tokens(messages: &[Message]) -> usize {
    let chars: usize = messages
        .iter()
        .flat_map(|m| m.content.iter())
        .map(|b| match b {
            ContentBlock::Text { text } => text.len(),
            ContentBlock::ToolUse { input, .. } => input.to_string().len(),
            // 票 02：tool_result 内嵌图也要计入——漏算的话 5MB base64
            // 绕过撞限检测直接撑爆请求体。
            ContentBlock::ToolResult {
                content, images, ..
            } => content.len() + images.iter().map(|i| i.data.len()).sum::<usize>(),
            // 图按 base64 字符粗算（≈真实 token 代价同量级，宁高估）
            ContentBlock::Image { data, .. } => data.len(),
            // 票 04：server 工具块按序列化尺寸计
            ContentBlock::Opaque { raw } => raw.to_string().len(),
        })
        .sum();
    chars / 4
}

/// 轻量裁剪（US37 + openworker-borrow 票 02）：超长 tool_result 走 spill——
/// 完整内容落 `.hexagon/spill/`，上下文留 head+marker+tail；被裁部分可读回，
/// 不再永久丢失。spill 失败时 spill_trim 内部退回旧式纯截断。
pub(super) fn trim_context(ctx: &ToolContext, mut messages: Vec<Message>) -> Vec<Message> {
    // 票 02：历史图先剥——只有最新一条 Tool 消息保留图，更早的
    // tool_result 图字节是上下文黑洞（单张可达 5MB base64）。剥图留
    // 指针注记，模型要再看可 fs_read 重读（与 spill 同一指针语义）。
    let last_tool = messages.iter().rposition(|m| m.role == Role::Tool);
    for (mi, m) in messages.iter_mut().enumerate() {
        for b in &mut m.content {
            if let ContentBlock::ToolResult {
                content, images, ..
            } = b
            {
                if Some(mi) != last_tool && !images.is_empty() {
                    let n = images.len();
                    images.clear();
                    content.push_str(&format!(
                        "\n[{n} image(s) elided from history — fs_read again if still needed]"
                    ));
                }
                if content.len() > TRIM_BLOCK_CHARS {
                    *content = crate::tools::spill_trim(ctx, content, TRIM_BLOCK_CHARS);
                }
            }
        }
    }
    messages
}

/// 撞限前的机械降级（openworker-borrow 票 06）：最旧轮次逐字落 transcript
/// 文件，出站视图换机械状态块（trace 派生）+ 首条负责人指令指引。
/// 零模型参与——守 US37「影响语义的决策不自动做」，compaction.py 的
/// LLM 摘要部分刻意不搬。
pub(super) fn mechanical_compact(
    db: &Db,
    ctx: &ToolContext,
    mut messages: Vec<Message>,
) -> Vec<Message> {
    // 保留 [0]=system、[1]=首条负责人指令；切 [2..k) 移出。
    if messages.len() <= 3 {
        return messages;
    }
    let mut k = messages.len() / 2;
    // 切点不能落在 Tool 消息上：那会把它和（已移出的）tool_use 拆开，
    // 出站历史出现孤儿 tool_result，Anthropic 类供应商直接 400。
    while k < messages.len() && messages[k].role == Role::Tool {
        k += 1;
    }
    if k <= 2 || k >= messages.len() {
        return messages;
    }
    let removed: Vec<Message> = messages.drain(2..k).collect();
    let Some(transcript) = write_transcript(ctx, &removed) else {
        // transcript 落不了盘就不悄悄丢消息——原样返回走升级路径
        return messages;
    };
    let block = format!(
        "[机械降级] 最旧 {} 条消息已逐字移到 {transcript}，需要细节用 fs_read 读。\n\
         {}\n\
         负责人首条指令见上文首条消息（逐字保留）。\n\
         续写契约：不重复已答问题、不复盘已交付内容。",
        removed.len(),
        crate::provenance::state_block(db, ctx),
    );
    messages.insert(
        2,
        Message {
            role: Role::User,
            content: vec![ContentBlock::Text { text: block }],
        },
    );
    let _ = db.append_event(
        &ctx.project_id,
        EventKind::System,
        json!({"kind": "context_compacted", "removed": k - 2, "transcript": transcript}),
        Some(&ctx.agent_id),
        ctx.stage_run_id.as_deref(),
    );
    messages
}

/// 逐字 transcript：消息与块原样落盘（tool_use/tool_result 含全部字段）。
fn write_transcript(ctx: &ToolContext, removed: &[Message]) -> Option<String> {
    let dir = ctx.repo_root.join(crate::tools::SPILL_DIR);
    std::fs::create_dir_all(&dir).ok()?;
    let n = std::fs::read_dir(&dir)
        .ok()?
        .filter_map(|e| e.ok())
        .filter(|e| e.file_name().to_string_lossy().starts_with("transcript-"))
        .count()
        + 1;
    let rel = format!("{}/transcript-{n}.md", crate::tools::SPILL_DIR);
    let mut s = String::new();
    for m in removed {
        s.push_str(&format!("## {:?}\n", m.role));
        for b in &m.content {
            match b {
                ContentBlock::Text { text } => {
                    s.push_str(text);
                    s.push('\n');
                }
                ContentBlock::ToolUse { id, name, input } => {
                    s.push_str(&format!("[tool_use {name} #{id}] {input}\n"));
                }
                ContentBlock::ToolResult {
                    tool_use_id,
                    content,
                    is_error,
                    images,
                } => {
                    s.push_str(&format!(
                        "[tool_result #{tool_use_id} err={is_error} images={}] {content}\n",
                        images.len()
                    ));
                }
                ContentBlock::Image { media_type, .. } => {
                    // transcript 记图元信息不落字节——transcript 是文本档案
                    s.push_str(&format!("[image {media_type}]\n"));
                }
                ContentBlock::Opaque { raw } => {
                    // 票 04：server 工具块记类型不记体（结果体可能很大）
                    let t = raw["type"].as_str().unwrap_or("opaque");
                    s.push_str(&format!("[server_tool {t}]\n"));
                }
            }
        }
        s.push('\n');
    }
    std::fs::write(ctx.repo_root.join(&rel), s).ok()?;
    Some(rel)
}

/// 撞限升级（US37）：escalation 待决卡（sub=context_overflow）问负责人。
pub(super) fn context_overflow(
    db: &Db,
    ctx: &ToolContext,
    est_tokens: usize,
    reason: &str,
) -> Result<TurnOutcome, TurnError> {
    let role: String = db
        .conn()
        .query_row(
            "SELECT role FROM agents WHERE id=?1",
            [&ctx.agent_id],
            |r| r.get(0),
        )
        .unwrap_or_else(|_| ctx.agent_id.clone());
    // 卡表写口归 cards.rs（arch-review 票 04）
    let qid = crate::cards::enqueue(
        db,
        &ctx.project_id,
        Some(&ctx.agent_id),
        crate::cards::CardKind::Escalation,
        json!({
            "sub": "context_overflow",
            "role": role,
            "est_tokens": est_tokens,
            "cap": CONTEXT_CAP_TOKENS,
            "reason": reason,
        }),
        None,
    )?;
    db.append_event(
        &ctx.project_id,
        EventKind::Escalated,
        // 票 02：上下文撞限升级带闭集 code——容量触顶归 budget-exceeded。
        json!({"reason": "context_overflow", "code": crate::trace::FailureCode::BudgetExceeded.as_str(),
               "question_id": qid, "est_tokens": est_tokens, "cap": CONTEXT_CAP_TOKENS}),
        Some(&ctx.agent_id),
        ctx.stage_run_id.as_deref(),
    )?;
    Ok(TurnOutcome::AwaitingPermission(qid))
}

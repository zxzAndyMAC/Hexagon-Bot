//! 研究助手（US36）：激活 Agent 在任务中上派一层只读嵌套回合。
//!
//! 结构性约束（不靠自觉）：
//! - 嵌套注册表 = `Registry::readonly()`：fs_read/artifact_read + mcp:* 共享件
//!   —— 无 fs_write/fs_patch/bash/artifact_write/git/research →
//!   不写盘、不执行命令、不 git、不再派生；
//! - MCP 授权闸门照旧按父 agent_id 判 → 用不了父未授权的服务；
//! - 嵌套 ctx 继承父 agent_id/stage_run_id/owned_globs →
//!   事件、用量、步数全部记父 Agent；
//! - 父休眠嵌套不跑（前置检查 + run_turn 休眠语义双闸）。
//!
//! 回包 {answer, citations} 经 ToolResult 喂回父 Agent——引用 = 嵌套段内
//! 实际读过的路径/产物，不是模型自称。

use crate::db::Db;
use crate::provider::ModelProvider;
use crate::tools::{CallOutcome, Registry, ToolContext, ToolError};
use crate::trace::EventKind;
use crate::turn::prompt::{LayerLevel, PromptLayer};
use crate::turn::{run_turn, TurnOutcome};
use serde_json::{json, Value};

/// turn 层截获入口：与 Registry::call 同款事件留痕，然后跑嵌套回合。
pub fn call_nested(
    db: &Db,
    provider: &dyn ModelProvider,
    registry: &Registry,
    ctx: &ToolContext,
    input: Value,
) -> Result<CallOutcome, ToolError> {
    let question = input["question"].as_str().unwrap_or("").to_string();
    db.append_event(
        &ctx.project_id,
        EventKind::ToolCalled,
        json!({"tool": "research", "input": {"question": question}}),
        Some(&ctx.agent_id),
        ctx.stage_run_id.as_deref(),
    )?;
    let result = run_nested(db, provider, registry, ctx, &question);
    let (ok, payload) = match &result {
        Ok(v) => (true, json!({"tool": "research", "output": v})),
        Err(e) => (false, json!({"tool": "research", "error": e.to_string()})),
    };
    db.append_event(
        &ctx.project_id,
        EventKind::ToolResult,
        json!({"tool": "research", "ok": ok, "result": payload}),
        Some(&ctx.agent_id),
        ctx.stage_run_id.as_deref(),
    )?;
    result.map(CallOutcome::Done)
}

fn run_nested(
    db: &Db,
    provider: &dyn ModelProvider,
    registry: &Registry,
    ctx: &ToolContext,
    question: &str,
) -> Result<Value, ToolError> {
    if question.trim().is_empty() {
        return Err(ToolError::BadInput("research needs a question".into()));
    }
    // 父休眠嵌套不跑（前置闸；run_turn 内还有一道）
    let status: String = db
        .conn()
        .query_row(
            "SELECT status FROM agents WHERE id=?1",
            [&ctx.agent_id],
            |r| r.get(0),
        )
        .map_err(ToolError::Sqlite)?;
    if status == "sleeping" {
        return Err(ToolError::Exec("parent agent sleeping".into()));
    }

    // 只读注册表 + 继承父归属（用量/授权/步数全记父）
    let ro = registry.readonly();
    let nctx = ctx.clone();

    // 嵌套段标记：事件与消息的水位线
    let ev_mark: i64 = db
        .conn()
        .query_row("SELECT COALESCE(MAX(id),0) FROM events", [], |r| r.get(0))
        .map_err(ToolError::Sqlite)?;
    let msg_mark: i64 = db
        .conn()
        .query_row("SELECT COALESCE(MAX(id),0) FROM messages", [], |r| r.get(0))
        .map_err(ToolError::Sqlite)?;

    let outcome = run_turn(
        db,
        provider,
        &ro,
        &nctx,
        vec![PromptLayer::new(
            LayerLevel::RoleDef,
            "你是只读研究助手：只读搜索、阅读、比对、摘要；回答必须给出依据引用。\
             你没有写盘/命令/git/派生能力——只做观察，不下结论代替执行。",
        )],
        question,
    )
    .map_err(|e| ToolError::Exec(format!("research turn failed: {e}")))?;

    match outcome {
        TurnOutcome::Finished => {
            // 引用 = 嵌套段内实际读过的路径/产物（读类 tool_called 的入参）
            let mut st = db
                .conn()
                .prepare(
                    "SELECT payload FROM events
                     WHERE project_id=?1 AND agent_id=?2 AND id>?3 AND kind='tool_called'
                     ORDER BY id",
                )
                .map_err(ToolError::Sqlite)?;
            let mut citations = Vec::new();
            let rows = st
                .query_map(
                    rusqlite::params![ctx.project_id, ctx.agent_id, ev_mark],
                    |r| r.get::<_, String>(0),
                )
                .map_err(ToolError::Sqlite)?;
            for p in rows.flatten() {
                let v: Value = serde_json::from_str(&p).unwrap_or_default();
                let tool = v["tool"].as_str().unwrap_or("");
                if !matches!(tool, "fs_read" | "artifact_read") {
                    continue;
                }
                if let Some(path) = v["input"]["path"].as_str().or(v["input"]["name"].as_str()) {
                    citations.push(json!({"via": tool, "ref": path}));
                }
            }
            // 答案 = 嵌套段最后一条 agent 消息
            let answer: Option<String> = db
                .conn()
                .query_row(
                    "SELECT body FROM messages
                     WHERE project_id=?1 AND author=?2 AND id>?3
                     ORDER BY id DESC LIMIT 1",
                    rusqlite::params![ctx.project_id, ctx.agent_id, msg_mark],
                    |r| r.get(0),
                )
                .ok();
            Ok(json!({
                "answer": answer.unwrap_or_default(),
                "citations": citations,
            }))
        }
        TurnOutcome::SkippedSleeping => Err(ToolError::Exec("parent agent sleeping".into())),
        TurnOutcome::SkippedCap => Err(ToolError::Exec("usage cap reached".into())),
        TurnOutcome::AwaitingPermission(_) => Err(ToolError::Exec(
            "research layer may not escalate — denied by structure".into(),
        )),
        TurnOutcome::Truncated => Err(ToolError::Exec("research output truncated".into())),
        TurnOutcome::Failed(e) => Err(ToolError::Exec(format!("research failed: {e}"))),
        // 票 04：负责人叫停嵌套研究 = 模型可读的普通工具错（叫停不是基建事故）
        TurnOutcome::Interrupted => Err(ToolError::Exec("interrupted by owner".into())),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::provider::ScriptedProvider;
    use crate::turn::{text_response, tool_response};

    fn setup() -> (crate::api::Workbench, tempfile::TempDir) {
        let dir = tempfile::tempdir().unwrap();
        let wb = crate::api::Workbench::for_test(dir.path(), &["后端"], None).unwrap();
        (wb, dir)
    }

    fn ctx(wb: &crate::api::Workbench) -> ToolContext {
        ToolContext {
            project_id: wb.project_id.clone(),
            agent_id: "a0".into(),
            repo_root: wb.repo_root.clone(),
            stage_run_id: None,
            owned_globs: vec![],
            tiers: crate::artifacts::TierMap::new(),
            sessions: Default::default(),
            caps: Default::default(),
        }
    }

    /// 空问题连回合都不起——BadInput，且不留嵌套段事件。
    #[test]
    fn empty_question_rejected_before_turn() {
        let (wb, _d) = setup();
        let provider = ScriptedProvider::new(vec![text_response("x")]);
        let r = call_nested(&wb.db, &provider, &wb.registry, &ctx(&wb), json!({}));
        assert!(matches!(r, Err(ToolError::BadInput(_))));
        assert!(provider.recorded().is_empty(), "空问题不许召模型");
    }

    /// 父休眠 → 前置闸拒跑（run_turn 内第二道闸也兜得住）。
    #[test]
    fn sleeping_parent_rejected() {
        let (wb, _d) = setup();
        crate::orchestra::write_agent_status(&wb.db, "p1", "a0", true).unwrap();
        let provider = ScriptedProvider::new(vec![text_response("x")]);
        let r = call_nested(
            &wb.db,
            &provider,
            &wb.registry,
            &ctx(&wb),
            json!({"question": "查一下"}),
        );
        assert!(matches!(r, Err(ToolError::Exec(_))));
        assert!(provider.recorded().is_empty());
    }

    /// 正常嵌套：answer=末条 agent 消息；citations=嵌套段读类工具入参。
    /// 嵌套段事件/消息照常落库（记父 agent 头上）。
    #[test]
    fn nested_turn_collects_answer_and_citations() {
        let (wb, _d) = setup();
        // for_test 的 agent 默认 sleeping（schema 默认）——激活才可跑回合。
        crate::orchestra::write_agent_status(&wb.db, "p1", "a0", false).unwrap();
        std::fs::write(wb.repo_root.join("note.md"), "hello").unwrap();
        let provider = ScriptedProvider::new(vec![
            tool_response(vec![("t1", "fs_read", json!({"path": "note.md"}))]),
            tool_response(vec![(
                "t2",
                "fs_write",
                json!({"path": "x", "content": "y"}),
            )]),
            text_response("结论是42"),
        ]);
        let out = call_nested(
            &wb.db,
            &provider,
            &wb.registry,
            &ctx(&wb),
            json!({"question": "note.md 里写了什么"}),
        )
        .unwrap();
        let CallOutcome::Done(v) = out else {
            panic!("expected Done: {out:?}")
        };
        assert_eq!(v["answer"], "结论是42");
        let cites = v["citations"].as_array().unwrap();
        assert!(
            cites.iter().any(|c| c["ref"] == "note.md"),
            "fs_read 入参应进引用: {cites:?}"
        );
        // 只读注册表：fs_write 不在其中——写调用不产生引用/落盘
        assert!(!cites.iter().any(|c| c["ref"] == "x"));
        assert!(!wb.repo_root.join("x").exists(), "只读嵌套不许写盘");
    }
}

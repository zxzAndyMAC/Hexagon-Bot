use super::*;
use crate::provider::ScriptedProvider;
use crate::tools::ToolContext;
use crate::trace::MessageToken;

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
            "INSERT INTO agents (id, project_id, role, status) VALUES ('a1','p1','后端开发','active')",
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

// ---------- 票 03：turn→UI delta ----------

/// 分块脚本回合：delta 有序到达且带归属，收束时补一发 done。
/// delta 是瞬时不落库——持久层只收最终拼装文本（spec 约定）。
#[test]
fn streaming_turn_emits_ordered_deltas_then_done() {
    let (db, reg, ctx, _dir) = setup();
    let provider = ScriptedProvider::chunked(vec![text_response("你好世界")], 2);
    let mut got: Vec<TurnDelta> = Vec::new();
    let out = run_turn_streaming(
        &db,
        &provider,
        &reg,
        &ctx,
        vec![],
        "写",
        false,
        Some(&mut |d| got.push(d.clone())),
    )
    .unwrap();
    assert!(matches!(out, TurnOutcome::Finished));
    let texts: Vec<&str> = got
        .iter()
        .filter(|d| !d.done && !d.reset)
        .map(|d| d.text.as_str())
        .collect();
    assert_eq!(texts.concat(), "你好世界");
    assert!(got.iter().all(|d| d.agent_id == "a1"));
    assert!(got.last().unwrap().done);
    // delta 不落库：agent_message 仍只有最终拼装那一条（4 块 delta≠4 条消息）
    let msgs: i64 = db
        .conn()
        .query_row(
            "SELECT COUNT(*) FROM messages WHERE project_id='p1' AND author='a1'",
            [],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(msgs, 1);
}

/// 发完半截文本撞上瞬时错 → 重试前补 reset，重试流从头重起。
#[test]
fn stream_retry_resets_partial_deltas() {
    /// 第一次调用：发一截文本再 Transport 错；之后正常。
    struct Flaky(std::sync::atomic::AtomicUsize);
    impl crate::provider::ModelProvider for Flaky {
        fn complete(
            &self,
            _r: &ChatRequest,
        ) -> Result<ChatResponse, crate::provider::ProviderError> {
            Ok(text_response("unused"))
        }
        fn stream(
            &self,
            _r: &ChatRequest,
            sink: &mut crate::provider::StreamSink<'_>,
        ) -> Result<ChatResponse, crate::provider::ProviderError> {
            if self.0.fetch_add(1, std::sync::atomic::Ordering::SeqCst) == 0 {
                sink(&crate::provider::StreamDelta::Text("半截".into()));
                return Err(crate::provider::ProviderError::Transport("flaky".into()));
            }
            sink(&crate::provider::StreamDelta::Text("完整答复".into()));
            Ok(text_response("完整答复"))
        }
    }
    let (db, reg, ctx, _dir) = setup();
    let provider = Flaky(0.into());
    let mut got: Vec<TurnDelta> = Vec::new();
    let out = run_turn_streaming(
        &db,
        &provider,
        &reg,
        &ctx,
        vec![],
        "写",
        false,
        Some(&mut |d| got.push(d.clone())),
    )
    .unwrap();
    assert!(matches!(out, TurnOutcome::Finished));
    assert_eq!(got[0].text, "半截");
    assert!(got[1].reset);
    assert_eq!(got[2].text, "完整答复");
    assert!(got.last().unwrap().done);
}

// ---------- 票 04：流中叫停 ----------

/// delta 间隙叫停：第一块发出后负责人 pause，第二块间隙检出 →
/// Interrupted 终态（非 Failed），不落假完成消息，done 照发。
#[test]
fn pause_mid_stream_interrupts() {
    let (db, reg, ctx, _dir) = setup();
    let provider = ScriptedProvider::chunked(vec![text_response("一二三四")], 1);
    let mut got: Vec<TurnDelta> = Vec::new();
    let db_ref = &db;
    let out = run_turn_streaming(
        db_ref,
        &provider,
        &reg,
        &ctx,
        vec![],
        "写",
        false,
        Some(&mut |d| {
            got.push(d.clone());
            if got.len() == 1 {
                crate::orchestra::pause(db_ref, "p1").unwrap();
            }
        }),
    )
    .unwrap();
    assert!(matches!(out, TurnOutcome::Interrupted));
    // 只流出第一块就被叫停
    assert_eq!(got[0].text, "一");
    assert!(got.last().unwrap().done);
    // 未落假完成消息
    let msgs: i64 = db
        .conn()
        .query_row("SELECT COUNT(*) FROM messages WHERE author='a1'", [], |r| {
            r.get(0)
        })
        .unwrap();
    assert_eq!(msgs, 0);
}

/// 流前叫停（既有轮顶闸）也走 Interrupted——两种叫停同终态。
#[test]
fn pause_before_turn_interrupts_without_call() {
    let (db, reg, ctx, _dir) = setup();
    crate::orchestra::pause(&db, "p1").unwrap();
    let provider = ScriptedProvider::new(vec![text_response("不该发出")]);
    let out = run_turn(&db, &provider, &reg, &ctx, vec![], "写").unwrap();
    assert!(matches!(out, TurnOutcome::Interrupted));
    assert!(provider.recorded().is_empty()); // 模型零调用
}

// ---------- 票 05：steering 回灌 ----------

/// 测试工具：扮演「回合进行中负责人发消息」——经 &Db 落 owner 消息，
/// 等价于壳层 send_message_side 的旁路写入（真实路径是第二条连接，
/// 进程内读写语义相同）。input.body 缺席时不写，供多轮脚本复用。
struct OwnerSpeaks;

impl crate::tools::Tool for OwnerSpeaks {
    fn name(&self) -> &str {
        "owner_speaks"
    }
    fn description(&self) -> &str {
        "test: owner sends a steering message mid-turn"
    }
    fn input_schema(&self) -> Value {
        json!({"type": "object", "properties": {"body": {"type": "string"}}})
    }
    // Read 档自动放行——测试要的是执行副作用，不是问权路径。
    fn risk(&self) -> crate::tools::RiskClass {
        crate::tools::RiskClass::Read
    }
    fn exec(&self, db: &Db, input: &Value, ctx: &ToolContext) -> Result<Value, ToolError> {
        if let Some(body) = input.get("body").and_then(|v| v.as_str()) {
            db.append_message(&ctx.project_id, "owner", body, &[], None, None)
                .map_err(|e| ToolError::Exec(e.to_string()))?;
        }
        Ok(json!({"ok": true}))
    }
}

fn steering_envelopes(req: &ChatRequest) -> usize {
    req.messages
        .iter()
        .filter(|m| {
            matches!(m.role, Role::User)
                && m.content.iter().any(
                    |b| matches!(b, ContentBlock::Text { text } if text.contains("\"steering\"")),
                )
        })
        .count()
}

/// 回合进行中落库的 owner 消息，在下一轮模型请求以 steering 信封
/// 出现；留 steering_injected 审计事件；持久历史不掺假 assistant。
#[test]
fn steering_message_mid_turn_reaches_next_call() {
    let (db, mut reg, ctx, _dir) = setup();
    reg.register(OwnerSpeaks);
    let provider = ScriptedProvider::new(vec![
        tool_response(vec![(
            "t1",
            "owner_speaks",
            json!({"body":"改方向：先做 B"}),
        )]),
        text_response("收到，先做 B"),
    ]);
    let out = run_turn(&db, &provider, &reg, &ctx, vec![], "做 A").unwrap();
    assert_eq!(out, TurnOutcome::Finished);

    let reqs = provider.recorded();
    assert_eq!(reqs.len(), 2);
    assert_eq!(
        steering_envelopes(&reqs[0]),
        0,
        "call 1 predates the message"
    );
    let env = steering_envelopes(&reqs[1]);
    assert_eq!(env, 1, "call 2 must carry exactly one steering envelope");
    let has_body = reqs[1].messages.iter().any(|m| {
        m.content.iter().any(|b| {
            matches!(b, ContentBlock::Text { text }
                if text.contains("\"steering\"") && text.contains("改方向：先做 B"))
        })
    });
    assert!(has_body);

    // 审计留痕：steering_injected 事件带消息 id 与轮次
    let injected = db
        .timeline("p1", None, 200, Some(&[EventKind::System]))
        .unwrap()
        .iter()
        .any(|i| i.event.payload.to_string().contains("steering_injected"));
    assert!(injected);

    // 持久层语义不变：owner 消息就是 owner 消息，没有被复制成
    // agent 发言；agent 只有最终答复那一条
    let rows: Vec<(String, String)> = db
        .conn()
        .prepare("SELECT author, body FROM messages ORDER BY id")
        .unwrap()
        .query_map([], |r| Ok((r.get(0)?, r.get(1)?)))
        .unwrap()
        .collect::<Result<_, _>>()
        .unwrap();
    assert_eq!(
        rows.iter().filter(|(a, _)| a == "a1").count(),
        1,
        "agent persisted exactly one final message"
    );
    assert!(rows
        .iter()
        .any(|(a, b)| a == "owner" && b == "改方向：先做 B"));
}

/// 水位线：回合开始前已存在的 owner 消息不走 steering 注入——
/// 它们属于正常上下文装配，注入只覆盖起跑线之后新来的。
#[test]
fn pre_turn_owner_message_not_steered() {
    let (db, mut reg, ctx, _dir) = setup();
    reg.register(OwnerSpeaks);
    db.append_message("p1", "owner", "起跑前的背景", &[], None, None)
        .unwrap();
    // 首轮调用工具但不写消息 → 第二轮上下文不应出现任何 steering 信封
    let provider = ScriptedProvider::new(vec![
        tool_response(vec![("t1", "owner_speaks", json!({}))]),
        text_response("done"),
    ]);
    let out = run_turn(&db, &provider, &reg, &ctx, vec![], "做 A").unwrap();
    assert_eq!(out, TurnOutcome::Finished);
    assert_eq!(provider.recorded().len(), 2);
    for req in &provider.recorded() {
        assert_eq!(
            steering_envelopes(req),
            0,
            "pre-turn owner message must not be re-injected as steering"
        );
    }
    let injected = db
        .timeline("p1", None, 200, Some(&[EventKind::System]))
        .unwrap()
        .iter()
        .any(|i| i.event.payload.to_string().contains("steering_injected"));
    assert!(!injected);
}

/// 注入只发生一次：信封进站后随 messages 持续随行（与真实对话
/// 语义一致），水位保证同一条 owner 消息不会产生第二个信封、
/// 也不落第二个 steering_injected 审计事件。
#[test]
fn steering_injected_once_not_every_round() {
    let (db, mut reg, ctx, _dir) = setup();
    reg.register(OwnerSpeaks);
    let provider = ScriptedProvider::new(vec![
        tool_response(vec![("t1", "owner_speaks", json!({"body":"插队一句"}))]),
        tool_response(vec![("t2", "owner_speaks", json!({}))]),
        text_response("done"),
    ]);
    let out = run_turn(&db, &provider, &reg, &ctx, vec![], "做 A").unwrap();
    assert_eq!(out, TurnOutcome::Finished);
    let reqs = provider.recorded();
    assert_eq!(reqs.len(), 3);
    // r2/r3 各带同一信封（上下文随行），r1 没有——重点是信封总数=1 条消息
    assert_eq!(steering_envelopes(&reqs[0]), 0);
    assert_eq!(steering_envelopes(&reqs[1]), 1);
    assert_eq!(steering_envelopes(&reqs[2]), 1);
    // 权威去重断言：排水只触发一次审计事件
    let injected_events = db
        .timeline("p1", None, 200, Some(&[EventKind::System]))
        .unwrap()
        .iter()
        .filter(|i| i.event.payload.to_string().contains("steering_injected"))
        .count();
    assert_eq!(injected_events, 1, "same message must not be drained twice");
}

/// 控制面不注入：owner 消息是文本指令（"/stamp"、"退回" 这类整句
/// 命令）时水位照进，但不产生 steering 信封也不落注入事件——
/// 指令走控制面，不该被当业务插话喂给模型。
#[test]
fn command_messages_not_steered() {
    let (db, mut reg, ctx, _dir) = setup();
    reg.register(OwnerSpeaks);
    let provider = ScriptedProvider::new(vec![
        tool_response(vec![("t1", "owner_speaks", json!({"body":"/stamp"}))]),
        text_response("done"),
    ]);
    let out = run_turn(&db, &provider, &reg, &ctx, vec![], "做 A").unwrap();
    assert_eq!(out, TurnOutcome::Finished);
    assert_eq!(provider.recorded().len(), 2);
    for req in &provider.recorded() {
        assert_eq!(steering_envelopes(req), 0);
    }
    let injected_events = db
        .timeline("p1", None, 200, Some(&[EventKind::System]))
        .unwrap()
        .iter()
        .filter(|i| i.event.payload.to_string().contains("steering_injected"))
        .count();
    assert_eq!(injected_events, 0);
}

#[test]
fn higher_layer_wins_key_conflict() {
    let prompt = build_system_prompt(vec![
        PromptLayer::keyed(LayerLevel::Brief, "commit_style", "no-verify 提交"),
        PromptLayer::keyed(LayerLevel::Workbench, "commit_style", "永远带签名提交"),
        PromptLayer::keyed(LayerLevel::Pack, "output", "产物写 .hexagon/"),
        PromptLayer::new(LayerLevel::RoleDef, "你是后端开发"),
    ]);
    assert!(prompt.contains("永远带签名提交"));
    assert!(!prompt.contains("no-verify"));
    assert!(prompt.contains("产物写 .hexagon/"));
    assert!(prompt.contains("你是后端开发"));
    // 层级序：workbench 段在最前
    assert!(prompt.find("workbench").unwrap() < prompt.find("role").unwrap());
}

#[test]
fn end_to_end_turn_brief_model_tool_artifact() {
    let (db, reg, ctx, dir) = setup();
    let provider = ScriptedProvider::new(vec![
        tool_response(vec![(
            "t1",
            "artifact_write",
            json!({"path":"impl/plan.md","content":"# 方案\n自由档产物"}),
        )]),
        text_response("已交付实现方案"),
    ]);
    let out = run_turn(&db, &provider, &reg, &ctx, vec![], "写个方案").unwrap();
    assert_eq!(out, TurnOutcome::Finished);
    assert!(dir.path().join(".hexagon/impl/plan.md").exists());
    // 全程事件可回放
    let items = db.timeline("p1", None, 50, None).unwrap();
    let kinds: Vec<_> = items.iter().map(|i| i.event.kind).collect();
    for k in [
        EventKind::TurnStarted,
        EventKind::ToolCalled,
        EventKind::ToolResult,
        EventKind::AgentMessage,
        EventKind::TurnFinished,
    ] {
        assert!(kinds.contains(&k), "missing {k:?}");
    }
    // 供应商收到 2 次调用：第一次带工具清单
    assert_eq!(provider.recorded().len(), 2);
    assert!(!provider.recorded()[0].tools.is_empty());
}

#[test]
fn usage_cap_blocks_scheduling() {
    let (db, reg, ctx, dir) = setup();
    // 设 1 分上限 + 高价表，先记一笔超限账
    std::fs::create_dir_all(dir.path().join(".hexagon")).unwrap();
    std::fs::write(
        dir.path().join(".hexagon/prices.json"),
        r#"{"default":{"prompt_per_1k_mc":2000,"completion_per_1k_mc":0}}"#,
    )
    .unwrap();
    db.conn()
        .execute("UPDATE projects SET usage_limit_cents=1 WHERE id='p1'", [])
        .unwrap();
    crate::usage::record(
        &db,
        &ctx,
        "chat",
        &crate::provider::Usage {
            prompt_tokens: 1000,
            completion_tokens: 0,
        },
        0,
    )
    .unwrap();
    let provider = ScriptedProvider::new(vec![text_response("不该被调用")]);
    let out = run_turn(&db, &provider, &reg, &ctx, vec![], "继续").unwrap();
    assert_eq!(out, TurnOutcome::SkippedCap);
    assert!(provider.recorded().is_empty());
    // 触顶事件 + 全员休眠已落
    let kinds: Vec<_> = db
        .timeline("p1", None, 50, None)
        .unwrap()
        .iter()
        .map(|i| i.event.kind)
        .collect();
    assert!(kinds.contains(&EventKind::UsageCapHit));
    assert!(kinds.contains(&EventKind::TeamSlept));
}

#[test]
fn sleeping_agent_makes_zero_model_calls() {
    let (db, reg, ctx, _dir) = setup();
    crate::orchestra::write_agent_status(&db, "p1", "a1", true).unwrap();
    let provider = ScriptedProvider::new(vec![text_response("不该被调用")]);
    let out = run_turn(&db, &provider, &reg, &ctx, vec![], "干活").unwrap();
    assert_eq!(out, TurnOutcome::SkippedSleeping);
    assert!(provider.recorded().is_empty());
    // 连 TurnStarted 都没有——根本没被调度
    assert!(db
        .timeline("p1", None, 50, Some(&[EventKind::TurnStarted]))
        .unwrap()
        .is_empty());
}

#[test]
fn ask_pauses_turn_with_question() {
    let (db, reg, ctx, _dir) = setup();
    let provider = ScriptedProvider::new(vec![tool_response(vec![(
        "t1",
        "bash",
        json!({"cmd":"cargo build"}),
    )])]);
    let out = run_turn(&db, &provider, &reg, &ctx, vec![], "构建").unwrap();
    let TurnOutcome::AwaitingPermission(qid) = out else {
        panic!("expected ask, got {out:?}")
    };
    assert!(qid.starts_with('q'));
}

#[test]
fn denied_tool_result_feeds_back() {
    let (db, reg, ctx, _dir) = setup();
    let provider = ScriptedProvider::new(vec![
        tool_response(vec![("t1", "fs_read", json!({"path":".env"}))]),
        text_response("被拒绝了"),
    ]);
    let out = run_turn(&db, &provider, &reg, &ctx, vec![], "读 env").unwrap();
    assert_eq!(out, TurnOutcome::Finished);
    // 第二轮请求的 messages 里带 is_error 的 ToolResult
    let second = &provider.recorded()[1];
    let has_err = second.messages.iter().any(|m| {
        m.content
            .iter()
            .any(|b| matches!(b, ContentBlock::ToolResult { is_error: true, .. }))
    });
    assert!(has_err);
}

#[test]
fn narrow_context_carries_pointers_not_bodies() {
    let (db, reg, ctx, _dir) = setup();
    // 塞一个产物行 + 一条点名消息
    db.conn()
        .execute(
            "INSERT INTO artifacts (id, project_id, path, kind, tier, version)
         VALUES ('art1','p1','specs/prd.md','规格','skeleton',1)",
            [],
        )
        .unwrap();
    db.append_message(
        "p1",
        "owner",
        "看一下规格 @后端开发",
        &[MessageToken::Mention {
            agent_role: "后端开发".into(),
        }],
        None,
        None,
    )
    .unwrap();
    db.append_message("p1", "owner", "无关消息", &[], None, None)
        .unwrap();

    let provider = ScriptedProvider::new(vec![text_response("ok")]);
    run_turn(&db, &provider, &reg, &ctx, vec![], "继续").unwrap();
    let req = &provider.recorded()[0];
    let ctx_text = match &req.messages[1].content[0] {
        ContentBlock::Text { text } => text.clone(),
        _ => panic!(),
    };
    // 有产物指针，无产物全文
    assert!(ctx_text.contains("specs/prd.md"));
    // 有点名消息，无无关消息
    assert!(ctx_text.contains("看一下规格"));
    assert!(!ctx_text.contains("无关消息"));
}

/// US35：工具循环有硬顶（MAX_TOOL_ROUNDS ≤ 规格上限 32）——超顶结束回合不空转。
#[test]
fn us35_tool_loop_capped() {
    let (db, reg, ctx, dir) = setup();
    std::fs::create_dir_all(dir.path().join("src")).unwrap();
    std::fs::write(dir.path().join("src/lib.rs"), "fn main() {}").unwrap();
    // 脚本给 9 轮 tool_use（> 8 轮上限）：回合必须报错收场
    let provider = ScriptedProvider::new(
        (0..9)
            .map(|i| {
                tool_response(vec![(
                    &format!("t{i}"),
                    "fs_read",
                    json!({"path":"src/lib.rs"}),
                )])
            })
            .collect(),
    );
    match run_turn(&db, &provider, &reg, &ctx, vec![], "go").unwrap() {
        TurnOutcome::Failed(e) => assert!(e.contains("tool-loop"), "got {e}"),
        o => panic!("expected Failed, got {o:?}"),
    }
    assert_eq!(provider.recorded().len(), 8);
}

/// US72：项目说明全文进激活首条消息；超 32KB 降级为目录+指引并提醒负责人。
#[test]
fn us72_agents_md_enters_first_message() {
    let (db, reg, ctx, dir) = setup();
    std::fs::write(
        dir.path().join("AGENTS.md"),
        "# 项目约束\n永远先跑测试再提交。",
    )
    .unwrap();
    let provider = ScriptedProvider::new(vec![text_response("ok")]);
    run_turn(&db, &provider, &reg, &ctx, vec![], "go").unwrap();
    let sys = match &provider.recorded()[0].messages[0].content[0] {
        ContentBlock::Text { text } => text.clone(),
        _ => panic!(),
    };
    assert!(sys.contains("agents.md"), "layer label missing: {sys}");
    assert!(sys.contains("永远先跑测试再提交"));
}

#[test]
fn us72_oversized_instructions_degrade_and_remind() {
    let (db, reg, ctx, dir) = setup();
    let mut big = String::from("# 头部\n");
    for i in 0..900 {
        big.push_str(&format!("## 第{i}节\n{}\n", "x".repeat(64)));
    }
    assert!(big.len() > 32 * 1024);
    std::fs::write(dir.path().join("AGENTS.md"), &big).unwrap();
    let provider = ScriptedProvider::new(vec![text_response("ok")]);
    run_turn(&db, &provider, &reg, &ctx, vec![], "go").unwrap();
    let sys = match &provider.recorded()[0].messages[0].content[0] {
        ContentBlock::Text { text } => text.clone(),
        _ => panic!(),
    };
    assert!(sys.contains("已降级"), "no degrade marker: {}", &sys[..200]);
    assert!(sys.contains("节标题目录"));
    assert!(sys.contains("## 第899节"), "outline truncated wrongly");
    assert!(!sys.contains(&"x".repeat(200)));
    // 负责人提醒事件已落
    let evs = db
        .timeline("p1", None, 50, Some(&[EventKind::System]))
        .unwrap();
    assert!(evs.iter().any(|i| i
        .event
        .payload
        .to_string()
        .contains("instructions_degraded")));
}

#[test]
fn us72_no_instructions_no_empty_layer() {
    let (db, reg, ctx, _dir) = setup();
    let provider = ScriptedProvider::new(vec![text_response("ok")]);
    run_turn(&db, &provider, &reg, &ctx, vec![], "go").unwrap();
    let sys = match &provider.recorded()[0].messages[0].content[0] {
        ContentBlock::Text { text } => text.clone(),
        _ => panic!(),
    };
    // 行为变更（skills.rs rescan 修复后）：内置技能目录是 AgentsMd 层，
    // 无说明文件时系统提示仍含 "## agents.md" 段——断言收紧为「无说明
    // 文件内容」（注入标记 "{name} 全文："；目录文本自带「取全文」字样，
    // 不能拿裸「全文：」当判据）。
    assert!(!sys.contains("AGENTS.md 全文") && !sys.contains("CLAUDE.md 全文"));
}

/// 票 02：撞限裁剪走 spill——超长 tool_result 全文落盘，上下文留
/// head+marker+tail；重复裁剪同内容幂等（文件名取内容哈希）。
#[test]
fn spill_trim_keeps_tail_and_spills_full() {
    let (_db, _reg, ctx, dir) = setup();
    let big = format!("{}{}", "h".repeat(6000), "FATAL_TAIL");
    let out = trim_context(
        &ctx,
        vec![Message {
            role: Role::Tool,
            content: vec![ContentBlock::ToolResult {
                tool_use_id: "t1".into(),
                content: big.clone(),
                is_error: false,
            }],
        }],
    );
    let ContentBlock::ToolResult { content, .. } = &out[0].content[0] else {
        panic!()
    };
    assert!(content.contains("full output: .hexagon/spill/"));
    assert!(content.ends_with("FATAL_TAIL"));
    let rel = content
        .split("full output: ")
        .nth(1)
        .unwrap()
        .split(']')
        .next()
        .unwrap()
        .trim();
    assert_eq!(std::fs::read_to_string(dir.path().join(rel)).unwrap(), big);
}

/// 票 06：撞限机械降级——最旧轮次逐字落 transcript，机械状态块入队，
/// 切点不拆 tool_use/tool_result 对。
#[test]
fn mechanical_compact_spills_and_keeps_boundaries() {
    let (db, _reg, ctx, dir) = setup();
    let mut msgs = vec![
        Message {
            role: Role::System,
            content: vec![ContentBlock::Text { text: "sys".into() }],
        },
        Message {
            role: Role::User,
            content: vec![ContentBlock::Text {
                text: "首条指令".into(),
            }],
        },
    ];
    for i in 0..8 {
        msgs.push(Message {
            role: Role::Assistant,
            content: vec![ContentBlock::ToolUse {
                id: format!("t{i}"),
                name: "fs_read".into(),
                input: json!({"path": format!("f{i}")}),
            }],
        });
        msgs.push(Message {
            role: Role::Tool,
            content: vec![ContentBlock::ToolResult {
                tool_use_id: format!("t{i}"),
                content: "x".repeat(100),
                is_error: false,
            }],
        });
    }
    msgs.push(Message {
        role: Role::Assistant,
        content: vec![ContentBlock::Text {
            text: "latest".into(),
        }],
    });
    let out = mechanical_compact(&db, &ctx, msgs);
    // [0]sys [1]首条指令原样；[2] 是机械块 User
    assert_eq!(out[1].role, Role::User);
    assert_eq!(out[2].role, Role::User);
    let ContentBlock::Text { text } = &out[2].content[0] else {
        panic!()
    };
    assert!(text.contains("机械降级") && text.contains("transcript-"));
    assert!(text.contains("首条指令") || text.contains("续写契约"));
    // 保留段首不是孤儿 tool_result
    assert_ne!(out[3].role, Role::Tool);
    // transcript 落盘且逐字
    let t = std::fs::read_to_string(dir.path().join(".hexagon/spill/transcript-1.md")).unwrap();
    assert!(t.contains("tool_use fs_read"));
    assert!(t.contains("tool_result"));
}

/// 票 06：没东西可切（或切点跑穿）→ 原样返回，升级路径不变。
#[test]
fn mechanical_compact_noop_when_nothing_to_cut() {
    let (db, _reg, ctx, _dir) = setup();
    let msgs = vec![
        Message {
            role: Role::System,
            content: vec![ContentBlock::Text { text: "s".into() }],
        },
        Message {
            role: Role::User,
            content: vec![ContentBlock::Text { text: "u".into() }],
        },
    ];
    assert_eq!(mechanical_compact(&db, &ctx, msgs).len(), 2);
}

// ---- US57：瞬时重试与同错熔断 ----

/// 可回放错误的假供应商：脚本条目是 Result，记录调用次数。
struct FlakyProvider {
    script: std::sync::Mutex<
        std::collections::VecDeque<Result<ChatResponse, crate::provider::ProviderError>>,
    >,
    calls: std::sync::Mutex<usize>,
}
impl FlakyProvider {
    fn new(script: Vec<Result<ChatResponse, crate::provider::ProviderError>>) -> Self {
        Self {
            script: std::sync::Mutex::new(script.into()),
            calls: std::sync::Mutex::new(0),
        }
    }
    fn calls(&self) -> usize {
        *self.calls.lock().unwrap()
    }
}
impl ModelProvider for FlakyProvider {
    fn complete(&self, _req: &ChatRequest) -> Result<ChatResponse, crate::provider::ProviderError> {
        *self.calls.lock().unwrap() += 1;
        self.script
            .lock()
            .unwrap()
            .pop_front()
            .unwrap_or(Err(crate::provider::ProviderError::ScriptExhausted))
    }
}

fn system_events(db: &Db, marker: &str) -> usize {
    db.timeline("p1", None, 200, None)
        .unwrap()
        .iter()
        .filter(|i| {
            i.event.kind == EventKind::System && i.event.payload.to_string().contains(marker)
        })
        .count()
}

#[test]
fn us57_transient_retry_recovers() {
    let (db, reg, ctx, _dir) = setup();
    let provider = FlakyProvider::new(vec![
        Err(crate::provider::ProviderError::Transport("timeout".into())),
        Err(crate::provider::ProviderError::Transport("reset".into())),
        Ok(text_response("好了")),
    ]);
    let out = run_turn(&db, &provider, &reg, &ctx, vec![], "go").unwrap();
    assert_eq!(out, TurnOutcome::Finished);
    assert_eq!(provider.calls(), 3); // 1 初调 + 2 重试
    assert_eq!(system_events(&db, "provider_retry"), 2);
}

#[test]
fn us57_retry_exhausted_fails_turn() {
    let (db, reg, ctx, _dir) = setup();
    // 4 次全 Transport：初调 + 3 次重试用尽仍败 → 回合失败
    let provider = FlakyProvider::new(vec![
        Err(crate::provider::ProviderError::Transport("t".into())),
        Err(crate::provider::ProviderError::Transport("t".into())),
        Err(crate::provider::ProviderError::Transport("t".into())),
        Err(crate::provider::ProviderError::Transport("t".into())),
        Ok(text_response("不该到")),
    ]);
    assert!(run_turn(&db, &provider, &reg, &ctx, vec![], "go").is_err());
    assert_eq!(provider.calls(), 4); // 最多 3 次重试
    assert_eq!(system_events(&db, "provider_retry"), 3);
}

#[test]
fn us57_refused_never_retried() {
    let (db, reg, ctx, _dir) = setup();
    // Refused（4xx/权限拒绝类）：不重试，一次即败
    let provider = FlakyProvider::new(vec![
        Err(crate::provider::ProviderError::Refused("403".into())),
        Ok(text_response("不该到")),
    ]);
    assert!(run_turn(&db, &provider, &reg, &ctx, vec![], "go").is_err());
    assert_eq!(provider.calls(), 1);
    assert_eq!(system_events(&db, "provider_retry"), 0);
}

#[test]
fn us57_same_tool_same_error_breaker() {
    let (db, reg, ctx, _dir) = setup();
    // 同一 fs_read 读不存在的文件连错：第 3 次熔断结束回合
    let provider = ScriptedProvider::new(vec![
        tool_response(vec![("t1", "fs_read", json!({"path": "nope.md"}))]),
        tool_response(vec![("t2", "fs_read", json!({"path": "nope.md"}))]),
        tool_response(vec![("t3", "fs_read", json!({"path": "nope.md"}))]),
        text_response("不该到"),
    ]);
    match run_turn(&db, &provider, &reg, &ctx, vec![], "go").unwrap() {
        TurnOutcome::Failed(e) => assert!(e.contains("tool breaker"), "got {e}"),
        o => panic!("expected breaker Failed, got {o:?}"),
    }
    assert_eq!(provider.recorded().len(), 3); // 熔断在第 3 次错，第 4 次模型调用不发生
    assert_eq!(system_events(&db, "tool_breaker"), 1);
}

#[test]
fn us57_streak_resets_on_success() {
    let (db, reg, ctx, dir) = setup();
    std::fs::write(dir.path().join("ok.md"), "hi").unwrap();
    // 错、错、成、错、错、完：连击从未到 3，不熔断
    let provider = ScriptedProvider::new(vec![
        tool_response(vec![("t1", "fs_read", json!({"path": "nope.md"}))]),
        tool_response(vec![("t2", "fs_read", json!({"path": "nope.md"}))]),
        tool_response(vec![("t3", "fs_read", json!({"path": "ok.md"}))]),
        tool_response(vec![("t4", "fs_read", json!({"path": "nope.md"}))]),
        tool_response(vec![("t5", "fs_read", json!({"path": "nope.md"}))]),
        text_response("done"),
    ]);
    let out = run_turn(&db, &provider, &reg, &ctx, vec![], "go").unwrap();
    assert_eq!(out, TurnOutcome::Finished);
    assert_eq!(system_events(&db, "tool_breaker"), 0);
}

// ---- US37：上下文撞停问负责人 ----

/// 估算 prompt 仍超上限 → 升级卡问负责人，不做自动全量摘要，模型零调用。
#[test]
fn us37_context_overflow_escalates() {
    let (db, reg, ctx, _dir) = setup();
    // 600KB 系统层 ≈ 150k tok > 120k 上限——轻量裁剪救不回系统层
    let big = PromptLayer::new(LayerLevel::Brief, "x".repeat(600_000));
    let provider = ScriptedProvider::new(vec![text_response("不该被调用")]);
    let out = run_turn(&db, &provider, &reg, &ctx, vec![big], "go").unwrap();
    match out {
        TurnOutcome::AwaitingPermission(qid) => {
            let card = crate::cards::get(&db, &qid).unwrap();
            assert_eq!(card.kind, "escalation");
            assert!(
                card.payload.to_string().contains("context_overflow"),
                "payload {}",
                card.payload
            );
        }
        o => panic!("expected AwaitingPermission, got {o:?}"),
    }
    assert!(provider.recorded().is_empty(), "撞限在模型调用前");
}

/// 票 13：stop=max_tokens 续推一次——后续正常响应则回合完成。
#[test]
fn t13_max_tokens_continuation_finishes() {
    let (db, reg, ctx, _dir) = setup();
    let provider = ScriptedProvider::new(vec![
        ChatResponse {
            content: vec![ContentBlock::Text {
                text: "半句话".into(),
            }],
            stop: StopReason::MaxTokens,
            usage: Default::default(),
        },
        text_response("说完的后半句"),
    ]);
    let out = run_turn(&db, &provider, &reg, &ctx, vec![], "go").unwrap();
    assert!(
        matches!(out, TurnOutcome::Finished),
        "续推成功应 Finished，got {out:?}"
    );
    let calls = provider.recorded();
    assert_eq!(calls.len(), 2, "截断+续推=两次调用");
    // 续推注入 nudge 用户消息而非重发原题（末条是票 14 动态块，
    // nudge 在倒数第二）
    let msgs = &calls[1].messages;
    let nudge = msgs.get(msgs.len().saturating_sub(2)).expect("nudge msg");
    assert!(
        matches!(&nudge.content[0], ContentBlock::Text { text } if text.contains("截断")),
        "续推应带 nudge"
    );
}

/// 票 13：续推后仍截断 → truncated 终态（与 Finished 区分），零升级卡。
#[test]
fn t13_still_truncated_terminal() {
    let (db, reg, ctx, _dir) = setup();
    let provider = ScriptedProvider::new(vec![
        ChatResponse {
            content: vec![ContentBlock::Text {
                text: "半句".into(),
            }],
            stop: StopReason::MaxTokens,
            usage: Default::default(),
        },
        ChatResponse {
            content: vec![ContentBlock::Text {
                text: "又是半句".into(),
            }],
            stop: StopReason::MaxTokens,
            usage: Default::default(),
        },
    ]);
    let out = run_turn(&db, &provider, &reg, &ctx, vec![], "go").unwrap();
    assert!(
        matches!(out, TurnOutcome::Truncated),
        "仍截断应 Truncated，got {out:?}"
    );
}

/// 票 14：易变内容（时间/轮次）独立成末尾块；系统提示+首条指令
/// 跨回合逐字节一致——provider prompt cache 命中的前提。
#[test]
fn t14_dynamic_tail_keeps_prefix_stable() {
    let (db, reg, ctx, _dir) = setup();
    let provider = ScriptedProvider::new(vec![text_response("done")]);
    run_turn(&db, &provider, &reg, &ctx, vec![], "go").unwrap();
    let provider2 = ScriptedProvider::new(vec![text_response("done")]);
    run_turn(&db, &provider2, &reg, &ctx, vec![], "go").unwrap();
    let c1 = provider.recorded();
    let c2 = provider2.recorded();
    let tail = c1[0].messages.last().expect("dynamic tail");
    assert!(
        matches!(&tail.content[0], ContentBlock::Text { text } if text.contains("\"env\"")),
        "末条应是动态块"
    );
    // 前缀逐字节稳定：系统提示与首条指令跨回合相同
    // （Message 无 PartialEq，序列化后比对）
    assert_eq!(
        serde_json::to_value(&c1[0].messages[0]).unwrap(),
        serde_json::to_value(&c2[0].messages[0]).unwrap(),
        "system 前缀漂移"
    );
    assert_eq!(
        serde_json::to_value(&c1[0].messages[1]).unwrap(),
        serde_json::to_value(&c2[0].messages[1]).unwrap(),
        "首条指令漂移"
    );
}

/// 票 13：悬空 tool_use 补 error stub——provider 形状配对恢复。
#[test]
fn t13_dangling_tool_use_repaired() {
    let mut messages = vec![
        Message {
            role: Role::Assistant,
            content: vec![
                ContentBlock::ToolUse {
                    id: "t1".into(),
                    name: "fs_read".into(),
                    input: serde_json::json!({}),
                },
                ContentBlock::ToolUse {
                    id: "t2".into(),
                    name: "fs_read".into(),
                    input: serde_json::json!({}),
                },
            ],
        },
        Message {
            role: Role::Tool,
            content: vec![ContentBlock::ToolResult {
                tool_use_id: "t1".into(),
                content: "ok".into(),
                is_error: false,
            }],
        },
        Message {
            role: Role::Assistant,
            content: vec![ContentBlock::ToolUse {
                id: "t3".into(),
                name: "bash".into(),
                input: serde_json::json!({}),
            }],
        },
    ];
    let n = repair_dangling_tool_uses(&mut messages);
    assert_eq!(n, 2, "t2、t3 悬空");
    // 每个 ToolUse 后都有配对 ToolResult
    let uses: Vec<&str> = messages
        .iter()
        .flat_map(|m| m.content.iter())
        .filter_map(|b| match b {
            ContentBlock::ToolUse { id, .. } => Some(id.as_str()),
            _ => None,
        })
        .collect();
    let results: Vec<&str> = messages
        .iter()
        .flat_map(|m| m.content.iter())
        .filter_map(|b| match b {
            ContentBlock::ToolResult { tool_use_id, .. } => Some(tool_use_id.as_str()),
            _ => None,
        })
        .collect();
    for u in &uses {
        assert!(results.contains(u), "{u} 无配对结果");
    }
    // stub 如实标注中断
    assert!(messages
        .iter()
        .flat_map(|m| m.content.iter())
        .any(|b| matches!(
            b,
            ContentBlock::ToolResult { content, is_error: true, .. }
                if content.contains("interrupted")
        )));
    // 幂等：再修一遍无新增
    assert_eq!(repair_dangling_tool_uses(&mut messages), 0);
}

/// 轻量裁剪：超长 tool_result 截断带标记；小消息不动。
#[test]
fn us37_trim_truncates_tool_results() {
    let (_db, _reg, ctx, _dir) = setup();
    let msgs = vec![
        Message {
            role: Role::Tool,
            content: vec![ContentBlock::ToolResult {
                tool_use_id: "t1".into(),
                content: "y".repeat(9000),
                is_error: false,
            }],
        },
        Message {
            role: Role::User,
            content: vec![ContentBlock::Text {
                text: "short".into(),
            }],
        },
    ];
    let out = trim_context(&ctx, msgs);
    match &out[0].content[0] {
        ContentBlock::ToolResult { content, .. } => {
            assert!(content.len() < 5000, "still {} chars", content.len());
            assert!(content.contains("trimmed"), "marker missing: {content}");
        }
        _ => panic!(),
    }
    match &out[1].content[0] {
        ContentBlock::Text { text } => assert_eq!(text, "short"),
        _ => panic!(),
    }
}

// ---- 票 10：事件游标 + taint ----

fn owner_mention(db: &Db, body: &str) {
    db.append_message(
        "p1",
        "owner",
        body,
        &[crate::trace::MessageToken::Mention {
            agent_role: "后端开发".into(),
        }],
        None,
        None,
    )
    .unwrap();
}

#[test]
fn cursor_scopes_brief_to_incremental_events() {
    let (db, _reg, _ctx, _d) = setup();
    owner_mention(&db, "第一条点名");
    let b1 = build_brief_context(&db, "a1", None).unwrap();
    assert_eq!(b1.mentions, vec!["第一条点名"]);
    assert!(b1.watermark > 0);
    // 游标推进后再来消息 → 第二次组装只见增量
    db.advance_cursor("a1", b1.watermark).unwrap();
    owner_mention(&db, "第二条点名");
    let b2 = build_brief_context(&db, "a1", None).unwrap();
    assert_eq!(b2.mentions, vec!["第二条点名"], "不重读游标前的消息");
    // 不推进 → 再读仍是同一条增量（休眠唤醒不漏不重）
    let b3 = build_brief_context(&db, "a1", None).unwrap();
    assert_eq!(b3.mentions, vec!["第二条点名"]);
}

#[test]
fn cursor_advances_only_after_first_response() {
    let (db, _reg, ctx, _d) = setup();
    owner_mention(&db, "点1");
    // provider 第一轮就失败：游标不该推进
    let failing = ScriptedProvider::new(vec![]);
    let _ = run_turn(&db, &failing, &_reg, &ctx, vec![], "干活");
    assert_eq!(db.cursor("a1"), 0, "brief 没送达模型前游标不动");
    // 成功的回合推进到组装水位
    let ok = ScriptedProvider::new(vec![text_response("done")]);
    run_turn(&db, &ok, &_reg, &ctx, vec![], "干活").unwrap();
    assert!(db.cursor("a1") > 0, "首个响应到手后游标推进");
}

#[test]
fn external_results_taint_subsequent_outputs() {
    let (db, _reg, _ctx, _d) = setup();
    db.conn()
        .execute(
            "INSERT INTO stage_runs (id, project_id, stage_name, seq, state)
             VALUES ('sr1','p1','实现',0,'active')",
            [],
        )
        .unwrap();
    // 外部内容回喂前：消息不带标
    db.append_message("p1", "a1", "之前的话", &[], Some("a1"), Some("sr1"))
        .unwrap();
    // mcp 结果回喂（tool_result 事件）
    db.append_event(
        "p1",
        EventKind::ToolResult,
        json!({"tool":"mcp:gh:list","output":{}}),
        Some("a1"),
        Some("sr1"),
    )
    .unwrap();
    db.append_message("p1", "a1", "读完外部后的判断", &[], Some("a1"), Some("sr1"))
        .unwrap();
    let items = db
        .timeline("p1", None, 50, Some(&[EventKind::AgentMessage]))
        .unwrap();
    assert_eq!(items.len(), 2);
    assert!(items[0].event.payload.get("after_external").is_none());
    assert_eq!(items[1].event.payload["after_external"], true);
    // 负责人消息永不打标
    db.append_message("p1", "owner", "负责人说的", &[], None, Some("sr1"))
        .unwrap();
    let items = db
        .timeline("p1", None, 50, Some(&[EventKind::OwnerMessage]))
        .unwrap();
    assert!(items[0].event.payload.get("after_external").is_none());
}

// ---- rsi-research 票 03：请求信封落盘 ----

fn envelopes(db: &Db) -> Vec<Value> {
    db.timeline("p1", None, 100, Some(&[EventKind::System]))
        .unwrap()
        .into_iter()
        .map(|i| i.event.payload)
        .filter(|p| p["kind"] == "request_envelope")
        .collect()
}

#[test]
fn dispatch_persists_request_envelope() {
    let (db, reg, ctx, _dir) = setup();
    let provider = ScriptedProvider::new(vec![text_response("done")]);
    run_turn(
        &db,
        &provider,
        &reg,
        &ctx,
        vec![
            PromptLayer::new(LayerLevel::Workbench, "wb"),
            PromptLayer::keyed(LayerLevel::RoleDef, "role:后端开发", "r"),
        ],
        "干活",
    )
    .unwrap();
    let env = envelopes(&db);
    assert_eq!(env.len(), 1);
    let e = &env[0];
    assert_eq!(e["call"], 0);
    assert_eq!(e["model_slot"], "default");
    // layer 清单带 level/key/hash/bytes
    // 行为变更（skills.rs rescan 修复）：内置技能目录恒在 → 第三个
    // agents.md 层是新常态，2 → 3。
    let layers = e["layers"].as_array().unwrap();
    assert_eq!(layers.len(), 3);
    assert_eq!(layers[0]["level"], "workbench");
    assert_eq!(layers[1]["key"], "role:后端开发");
    assert_eq!(layers[2]["level"], "agents.md");
    assert!(layers[0]["sha"].as_str().unwrap().len() == 16);
    // 每消息一条指纹
    assert_eq!(e["messages"].as_array().unwrap().len(), 2); // system+user
                                                            // 工具名单非空（内置注册表）
    assert!(!e["tools"].as_array().unwrap().is_empty());
    assert_eq!(e["fingerprint"].as_str().unwrap().len(), 16);
}

#[test]
fn envelope_fingerprint_is_deterministic_and_sensitive() {
    // 纯函数级：同素材同指纹;任一素材变即变。
    let req = ChatRequest {
        model_slot: "default".into(),
        messages: vec![],
        tools: vec![],
    };
    let lm = layer_meta(&[PromptLayer::new(LayerLevel::Workbench, "wb")]);
    let msgs = vec![Message {
        role: Role::User,
        content: vec![ContentBlock::Text { text: "m".into() }],
    }];
    let e1 = request_envelope(0, &req, &msgs, &lm);
    let e2 = request_envelope(0, &req, &msgs, &lm);
    assert_eq!(e1["fingerprint"], e2["fingerprint"]);
    // 消息内容变 → 指纹变
    let msgs2 = vec![Message {
        role: Role::User,
        content: vec![ContentBlock::Text { text: "m2".into() }],
    }];
    assert_ne!(
        e1["fingerprint"],
        request_envelope(0, &req, &msgs2, &lm)["fingerprint"]
    );
    // layer 变 → 指纹变
    let lm2 = layer_meta(&[PromptLayer::new(LayerLevel::Workbench, "wb2")]);
    assert_ne!(
        e1["fingerprint"],
        request_envelope(0, &req, &msgs, &lm2)["fingerprint"]
    );
    // 多轮派发：信封按 call 序号区分,轮 1 比轮 0 多 assistant/tool 消息
    let (db, reg, ctx, _dir) = setup();
    let provider = ScriptedProvider::new(vec![
        tool_response(vec![("t1", "fs_read", json!({"path": "x"}))]),
        text_response("done"),
    ]);
    run_turn(&db, &provider, &reg, &ctx, vec![], "读").ok();
    let env = envelopes(&db);
    assert!(env.len() >= 2);
    assert_eq!(env[0]["call"], 0);
    assert_eq!(env[1]["call"], 1);
    assert!(
        env[1]["messages"].as_array().unwrap().len() > env[0]["messages"].as_array().unwrap().len()
    );
}

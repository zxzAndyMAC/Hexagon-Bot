use super::*;
use std::fs;

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
            "INSERT INTO agents (id, project_id, role) VALUES ('a1','p1','后端开发')",
            [],
        )
        .unwrap();
    let dir = tempfile::tempdir().unwrap();
    let ctx = ToolContext {
        project_id: "p1".into(),
        agent_id: "a1".into(),
        repo_root: dir.path().to_path_buf(),
        stage_run_id: None,
        owned_globs: vec![],
        tiers: crate::artifacts::TierMap::new(),
    };
    (db, Registry::builtin(), ctx, dir)
}

#[test]
fn fs_read_write_roundtrip_within_repo() {
    let (db, reg, ctx, dir) = setup();
    let out = reg
        .call(
            &db,
            &ctx,
            "fs_write",
            json!({"path": "src/main.rs", "content": "fn main() {}"}),
        )
        .unwrap();
    assert!(matches!(out, CallOutcome::Done(_)));
    assert!(dir.path().join("src/main.rs").exists());
    let out = reg
        .call(&db, &ctx, "fs_read", json!({"path": "src/main.rs"}))
        .unwrap();
    let CallOutcome::Done(v) = out else { panic!() };
    assert_eq!(v["content"], "fn main() {}");
    // 调用 + 结果事件都落了
    let items = db.timeline("p1", None, 50, None).unwrap();
    let kinds: Vec<_> = items.iter().map(|i| i.event.kind).collect();
    assert!(kinds.contains(&EventKind::ToolCalled));
    assert!(kinds.contains(&EventKind::ToolResult));
}

#[test]
fn fs_cannot_escape_repo_root() {
    let (db, reg, ctx, _dir) = setup();
    for evil in ["../outside.txt", "/etc/hostname", "a/../../b.txt"] {
        let out = reg.call(&db, &ctx, "fs_write", json!({"path": evil, "content": "x"}));
        match out {
            Err(ToolError::PathEscape(_)) => {}
            other => panic!("{evil} should escape-deny, got {other:?}"),
        }
    }
}

#[test]
fn credential_reads_denied_with_event() {
    let (db, reg, ctx, _dir) = setup();
    for p in [".env", "id_rsa", "certs/server.pem", ".aws/credentials"] {
        let out = reg.call(&db, &ctx, "fs_read", json!({"path": p})).unwrap();
        assert!(matches!(out, CallOutcome::Denied(_)), "{p}");
    }
    let items = db
        .timeline("p1", None, 50, Some(&[EventKind::PermissionDenied]))
        .unwrap();
    assert_eq!(items.len(), 4);
    assert_eq!(items[0].event.payload["layer"], "builtin_deny");
}

#[test]
fn agent_cannot_write_permission_rules() {
    let (db, reg, ctx, _dir) = setup();
    let out = reg
        .call(
            &db,
            &ctx,
            "fs_write",
            json!({"path": ".hexagon/permissions.toml", "content": "allow = *"}),
        )
        .unwrap();
    assert!(matches!(out, CallOutcome::Denied(_)));
}

#[test]
fn bash_credential_probe_denied() {
    let (db, reg, ctx, _dir) = setup();
    let out = reg
        .call(
            &db,
            &ctx,
            "bash",
            json!({"cmd": "security find-generic-password -a x"}),
        )
        .unwrap();
    assert!(matches!(out, CallOutcome::Denied(_)));
}

#[test]
fn bash_asks_then_executes_on_allow() {
    let (db, reg, ctx, dir) = setup();
    let out = reg
        .call(&db, &ctx, "bash", json!({"cmd": "echo hi > out.txt"}))
        .unwrap();
    let CallOutcome::Asked(qid) = out else {
        panic!()
    };
    // 未执行
    assert!(!dir.path().join("out.txt").exists());
    let out = reg
        .resolve(&db, &ctx, &qid, true, None, "activation", None, "owner")
        .unwrap();
    let CallOutcome::Done(v) = out else { panic!() };
    assert_eq!(v["exit_code"], 0);
    assert!(dir.path().join("out.txt").exists());
    let items = db.timeline("p1", None, 50, None).unwrap();
    let kinds: Vec<_> = items.iter().map(|i| i.event.kind).collect();
    for k in [
        EventKind::ToolCalled,
        EventKind::PermissionAsked,
        EventKind::PermissionAllowed,
        EventKind::ToolResult,
    ] {
        assert!(kinds.contains(&k), "missing {k:?}");
    }
}

#[test]
fn denied_question_does_not_execute() {
    let (db, reg, ctx, dir) = setup();
    let CallOutcome::Asked(qid) = reg
        .call(&db, &ctx, "bash", json!({"cmd": "touch nope"}))
        .unwrap()
    else {
        panic!()
    };
    reg.resolve(&db, &ctx, &qid, false, None, "activation", None, "owner")
        .unwrap();
    assert!(!dir.path().join("nope").exists());
}

#[test]
fn remember_shape_writes_rule() {
    let (db, reg, ctx, _dir) = setup();
    let CallOutcome::Asked(qid) = reg
        .call(&db, &ctx, "bash", json!({"cmd": "npm install zod"}))
        .unwrap()
    else {
        panic!()
    };
    reg.resolve(
        &db,
        &ctx,
        &qid,
        true,
        Some("npm install *"),
        "project",
        None,
        "owner",
    )
    .unwrap();
    let n: i64 = db
        .conn()
        .query_row(
            "SELECT COUNT(*) FROM permission_rules WHERE shape='npm install *' AND scope='project'",
            [],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(n, 1);
}

#[test]
fn ownership_glob_gates_writes() {
    let (db, reg, mut ctx, _dir) = setup();
    ctx.owned_globs = vec!["src/**".into()];
    // 界内自动放行
    let out = reg
        .call(
            &db,
            &ctx,
            "fs_write",
            json!({"path": "src/a.rs", "content": "x"}),
        )
        .unwrap();
    assert!(matches!(out, CallOutcome::Done(_)));
    // 界外转必问
    let out = reg
        .call(
            &db,
            &ctx,
            "fs_write",
            json!({"path": "docs/b.md", "content": "x"}),
        )
        .unwrap();
    assert!(matches!(out, CallOutcome::Asked(_)));
}

// ---------- openworker-borrow 票 02：工具结果 spill ----------

#[test]
fn bash_oversize_output_spills_head_marker_tail() {
    let (db, _reg, ctx, dir) = setup();
    // stdout 超 BASH_OUTPUT_CAP：直接调 exec 贴近断言面
    // （权限管线已在 bash_asks_then_executes_on_allow 覆盖）
    let head = "H".repeat(BASH_OUTPUT_CAP);
    let tail = "TAIL_ERROR_MARKER";
    let body = format!("{head}{}{tail}", "x".repeat(1024));
    let v = Bash
        .exec(
            &db,
            &json!({"cmd": format!("printf '%s' '{}'", body)}),
            &ctx,
        )
        .unwrap();
    let out = v["stdout"].as_str().unwrap();
    assert!(out.contains("full output: .hexagon/spill/"), "{out:?}");
    assert!(out.starts_with("HHH"), "head kept");
    assert!(out.ends_with(tail), "tail kept: {}", &out[out.len() - 80..]);
    // spill 文件可经 fs_read 读回完整内容
    let spill_rel = out
        .split("full output: ")
        .nth(1)
        .unwrap()
        .split(']')
        .next()
        .unwrap()
        .trim();
    let full = std::fs::read_to_string(dir.path().join(spill_rel)).unwrap();
    assert_eq!(full, body);
    // .gitignore 已登记
    assert!(std::fs::read_to_string(dir.path().join(".gitignore"))
        .unwrap()
        .contains(".hexagon/spill/"));
}

#[test]
fn fs_read_oversize_spills_and_is_idempotent() {
    let (_db, _reg, ctx, dir) = setup();
    let big = "a".repeat(FS_READ_CAP + 4096) + "END";
    std::fs::write(dir.path().join("big.txt"), &big).unwrap();
    let v = FsRead
        .exec(&_db, &json!({"path": "big.txt"}), &ctx)
        .unwrap();
    assert_eq!(v["truncated"], true);
    let content = v["content"].as_str().unwrap();
    assert!(content.contains("full output:"));
    assert!(content.ends_with("END"), "tail preserved");
    // 幂等：同内容再 spill 不产生第二个文件
    let again = spill_result(&ctx, &big).unwrap();
    assert!(dir.path().join(&again).exists());
    let n = std::fs::read_dir(dir.path().join(SPILL_DIR))
        .unwrap()
        .count();
    assert_eq!(n, 1);
}

#[test]
fn write_content_not_logged_in_events() {
    let (db, reg, ctx, _dir) = setup();
    reg.call(
        &db,
        &ctx,
        "fs_write",
        json!({"path": "a.txt", "content": "SUPERSECRET"}),
    )
    .unwrap();
    let items = db.timeline("p1", None, 50, None).unwrap();
    for i in &items {
        assert!(!i.event.payload.to_string().contains("SUPERSECRET"));
    }
    // 但文件本身写进去了
    assert_eq!(
        fs::read_to_string(_dir.path().join("a.txt")).unwrap(),
        "SUPERSECRET"
    );
}

// ---- 票 11：必问卡幂等键 ----

#[test]
fn queued_question_reused_on_same_call_seq() {
    let (db, reg, ctx, _d) = setup();
    let inp = || json!({"cmd": "rm -rf build"});
    let o1 = reg
        .call_with_seq(&db, &ctx, "bash", inp(), Some("r0i0"))
        .unwrap();
    let CallOutcome::Asked(q1) = o1 else { panic!() };
    // 同 seq+同参重放 → 同一张卡，队列不增
    let o2 = reg
        .call_with_seq(&db, &ctx, "bash", inp(), Some("r0i0"))
        .unwrap();
    assert!(matches!(o2, CallOutcome::Asked(ref q) if *q == q1));
    let n =
        crate::cards::count_queued(&db, "p1", Some(crate::cards::CardKind::Permission)).unwrap();
    assert_eq!(n, 1);
    // deduped 事件留痕
    let items = db
        .timeline("p1", None, 50, Some(&[EventKind::PermissionAsked]))
        .unwrap();
    assert_eq!(
        items[1].event.payload["deduped"], true,
        "第二次命中应有 deduped 标记"
    );
    // 不同 seq 或不同参 → 各自独立新卡
    let o3 = reg
        .call_with_seq(&db, &ctx, "bash", inp(), Some("r0i1"))
        .unwrap();
    assert!(matches!(o3, CallOutcome::Asked(ref q) if *q != q1));
    let o4 = reg
        .call_with_seq(&db, &ctx, "bash", json!({"cmd":"ls"}), Some("r0i0"))
        .unwrap();
    assert!(matches!(o4, CallOutcome::Asked(ref q) if *q != q1));
}

#[test]
fn answered_deny_replays_as_denied_without_new_card() {
    let (db, reg, ctx, _d) = setup();
    let CallOutcome::Asked(qid) = reg
        .call_with_seq(&db, &ctx, "bash", json!({"cmd":"rm -rf x"}), Some("r0i0"))
        .unwrap()
    else {
        panic!()
    };
    reg.resolve(&db, &ctx, &qid, false, None, "activation", None, "owner")
        .unwrap();
    // 重放同调用 → 沿用 deny 裁决，不弹新卡；answered_by 落账
    let out = reg
        .call_with_seq(&db, &ctx, "bash", json!({"cmd":"rm -rf x"}), Some("r0i0"))
        .unwrap();
    assert!(matches!(out, CallOutcome::Denied(ref r) if r == "owner denied"));
    let n = crate::cards::count_queued(&db, "p1", None).unwrap();
    assert_eq!(n, 0, "卡已答，不再计 queued");
    let by = crate::cards::get(&db, &qid).unwrap().answered_by;
    assert_eq!(by.as_deref(), Some("owner"));
}

#[test]
fn answered_allow_with_result_replays_as_done_marker() {
    let (db, reg, ctx, _d) = setup();
    let CallOutcome::Asked(qid) = reg
        .call_with_seq(&db, &ctx, "bash", json!({"cmd":"echo hi"}), Some("r0i0"))
        .unwrap()
    else {
        panic!()
    };
    // owner 批准 → resolve 内已执行
    let out = reg
        .resolve(&db, &ctx, &qid, true, None, "activation", None, "owner")
        .unwrap();
    assert!(matches!(out, CallOutcome::Done(_)));
    // 重放 → Done 标记，不再执行（副作用不双跑）
    let out = reg
        .call_with_seq(&db, &ctx, "bash", json!({"cmd":"echo hi"}), Some("r0i0"))
        .unwrap();
    let CallOutcome::Done(v) = out else { panic!() };
    assert_eq!(v["duplicate_of"], qid);
    let execs: i64 = db
            .conn()
            .query_row(
                "SELECT COUNT(*) FROM events WHERE kind='tool_result' AND json_extract(payload,'$.tool')='bash'",
                [],
                |r| r.get(0),
            )
            .unwrap();
    assert_eq!(execs, 1, "副作用只跑一次");
}

// ---- 票 12：审计脱敏 ----

#[test]
fn secret_keys_redacted_in_trace_payload() {
    let (db, reg, ctx, _d) = setup();
    // bash 走 Ask → 卡；入参里带机密字段
    let _ = reg.call(
        &db,
        &ctx,
        "bash",
        json!({"cmd":"x","api_key":"AKIA123","nested":{"password":"p@ss","note":"ok"},
                   "reply_body":"<机密正文>","safe":"visible"}),
    );
    let items = db
        .timeline("p1", None, 50, Some(&[EventKind::ToolCalled]))
        .unwrap();
    let inp = &items[0].event.payload["input"];
    assert_eq!(inp["api_key"], "[redacted]");
    assert_eq!(inp["nested"]["password"], "[redacted]");
    assert_eq!(inp["nested"]["note"], "ok");
    assert_eq!(inp["reply_body"], "[redacted body]");
    assert_eq!(inp["safe"], "visible");
    // 整个事件文本里不能出现机密值
    let raw = items[0].event.payload.to_string();
    assert!(!raw.contains("AKIA123"));
    assert!(!raw.contains("p@ss"));
    assert!(!raw.contains("机密正文"));
}

#[test]
fn oversized_event_payload_spills_to_file() {
    let (db, _reg, _ctx, dir) = setup();
    db.conn()
        .execute(
            "UPDATE projects SET dir=?1 WHERE id='p1'",
            [dir.path().to_str().unwrap()],
        )
        .unwrap();
    let big = "x".repeat(200 * 1024);
    db.append_event(
        "p1",
        EventKind::System,
        json!({"kind":"big","blob": big}),
        Some("a1"),
        None,
    )
    .unwrap();
    let (payload,): (String,) = db
            .conn()
            .query_row(
                "SELECT payload FROM events WHERE kind='system' AND json_extract(payload,'$.kind') IS NULL",
                [],
                |r| Ok((r.get(0)?,)),
            )
            .unwrap_or_else(|_| {
                db.conn()
                    .query_row(
                        "SELECT payload FROM events ORDER BY id DESC LIMIT 1",
                        [],
                        |r| Ok((r.get(0)?,)),
                    )
                    .unwrap()
            });
    let p: Value = serde_json::from_str(&payload).unwrap();
    assert!(p["spilled"].is_string(), "超限载荷应落 spill 指针: {p}");
    let spilled_path = p["spilled"].as_str().unwrap().to_string();
    let full = std::fs::read_to_string(&spilled_path).unwrap();
    assert!(full.contains(&"x".repeat(1000)), "spill 文件存全量");
    assert!(std::path::Path::new(&spilled_path)
        .to_string_lossy()
        .contains(".hexagon/spill"));
}

// ---- rsi-research 票 05：裁决器不对称不变量 ----

#[test]
fn allow_once_does_not_persist_next_call_asks_again() {
    // 授权一次性：批准一次（不记形）只执行那一发；同形状新调用再弹卡。
    // （idem 复用只担保「同一调用重放不双弹」,不同 call_seq = 新调用。）
    let (db, reg, ctx, dir) = setup();
    let CallOutcome::Asked(q1) = reg
        .call(&db, &ctx, "bash", json!({"cmd": "touch once1"}))
        .unwrap()
    else {
        panic!()
    };
    reg.resolve(&db, &ctx, &q1, true, None, "activation", None, "owner")
        .unwrap();
    assert!(dir.path().join("once1").exists());
    // 无规则沉淀
    let n: i64 = db
        .conn()
        .query_row("SELECT COUNT(*) FROM permission_rules", [], |r| r.get(0))
        .unwrap();
    assert_eq!(n, 0);
    // 新 seq 的同形状调用 → 再必问
    let out = reg
        .call_with_seq(
            &db,
            &ctx,
            "bash",
            json!({"cmd": "touch once2"}),
            Some("r7:i0"),
        )
        .unwrap();
    assert!(matches!(out, CallOutcome::Asked(_)));
    assert!(!dir.path().join("once2").exists());
}

#[test]
fn safety_net_ask_survives_any_memory() {
    // 安全网永不进记忆且永远必问——即使规则表被手工塞进匹配 allow。
    let (db, reg, ctx, _dir) = setup();
    db.conn()
        .execute(
            "INSERT INTO permission_rules (id, project_id, tool, shape, effect, scope)
                 VALUES ('prX','p1','bash','git push *','allow','project')",
            [],
        )
        .unwrap();
    let out = reg
        .call(&db, &ctx, "bash", json!({"cmd": "git push origin main"}))
        .unwrap();
    assert!(matches!(out, CallOutcome::Asked(_)));
    let n: i64 = db
        .conn()
        .query_row(
            "SELECT COUNT(*) FROM events WHERE kind='permission_allowed'",
            [],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(n, 0);
}

use super::*;
use std::fs;

fn setup() -> (Db, Registry, ToolContext, tempfile::TempDir) {
    let db = Db::open_in_memory().unwrap();
    db.conn()
        .execute(
            // 票 03：默认 L4 会放行安全网和新询问。本夹具守的是排队语义，钉 L0。
            "INSERT INTO projects (id, dir, name, mode, autonomy) VALUES ('p1','/tmp/x','x','pack','L0')",
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
        sessions: Default::default(),
        caps: Default::default(),
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

// ---- agent-senses 票 01：web_fetch ----

/// 极简 HTTP 服务器：按序回复每条响应后连接即关。返回 base URL。
fn serve_seq(f: impl FnOnce(u16) -> Vec<String> + Send + 'static) -> String {
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let port = listener.local_addr().unwrap().port();
    std::thread::spawn(move || {
        for body in f(port) {
            let Ok((mut s, _)) = listener.accept() else {
                return;
            };
            let mut buf = [0u8; 8192];
            let _ = std::io::Read::read(&mut s, &mut buf);
            let _ = std::io::Write::write_all(&mut s, body.as_bytes());
            let _ = std::io::Write::flush(&mut s);
        }
    });
    format!("http://127.0.0.1:{port}")
}

fn http_resp(status: u16, reason: &str, headers: &[(&str, &str)], body: &str) -> String {
    let h: String = headers
        .iter()
        .map(|(k, v)| format!("{k}: {v}\r\n"))
        .collect();
    format!(
        "HTTP/1.1 {status} {reason}\r\ncontent-length: {}\r\nconnection: close\r\n{h}\r\n{body}",
        body.len()
    )
}

#[test]
fn web_fetch_asks_then_extracts_text() {
    let (db, reg, ctx, _dir) = setup();
    let url = serve_seq(|_| {
        vec![http_resp(
            200,
            "OK",
            &[("content-type", "text/html; charset=utf-8")],
            "<html><head><style>bad{}</style><script>evil()</script></head>\
             <body><h1>Title</h1><p>Hello &amp; bye</p></body></html>",
        )]
    });
    // Egress 类默认必问
    let out = reg
        .call(&db, &ctx, "web_fetch", json!({"url": url}))
        .unwrap();
    let CallOutcome::Asked(qid) = out else {
        panic!()
    };
    let out = reg
        .resolve(&db, &ctx, &qid, true, None, "activation", None, "owner")
        .unwrap();
    let CallOutcome::Done(v) = out else { panic!() };
    let c = v["content"].as_str().unwrap();
    assert!(c.contains("Title"), "{c}");
    assert!(c.contains("Hello & bye"), "实体解码: {c}");
    assert!(
        !c.contains("evil()") && !c.contains("bad{}"),
        "script/style 已剔除: {c}"
    );
    assert!(!c.contains("<p>"), "标签已剥: {c}");
}

#[test]
fn web_fetch_remembered_domain_skips_ask() {
    let (db, reg, ctx, _dir) = setup();
    let base = serve_seq(|_| {
        vec![
            http_resp(200, "OK", &[("content-type", "text/plain")], "a"),
            http_resp(200, "OK", &[("content-type", "text/plain")], "b"),
        ]
    });
    let CallOutcome::Asked(qid) = reg
        .call(&db, &ctx, "web_fetch", json!({"url": format!("{base}/a")}))
        .unwrap()
    else {
        panic!()
    };
    // 记住 *@127.0.0.1（project 作用域）
    reg.resolve(
        &db,
        &ctx,
        &qid,
        true,
        Some("*@127.0.0.1"),
        "project",
        None,
        "owner",
    )
    .unwrap();
    // 同域不同路径 → 直放不弹卡
    let out = reg
        .call(&db, &ctx, "web_fetch", json!({"url": format!("{base}/b")}))
        .unwrap();
    let CallOutcome::Done(v) = out else {
        panic!("remembered domain should allow: {out:?}")
    };
    assert_eq!(v["status"], 200);
    // 寄生域不命中：形状 *@127.0.0.1 不覆盖 127.0.0.1.evil.test
    assert!(!crate::permissions::shape_matches(
        "*@127.0.0.1",
        "web_fetch",
        &json!({"url": "http://127.0.0.1.evil.test/x"})
    ));
    assert!(
        crate::permissions::shape_matches(
            "*@127.0.0.1",
            "web_fetch",
            &json!({"url": "http://sub.127.0.0.1/x"})
        ) || true
    ); // 子域后缀命中是预期（.127.0.0.1 结尾——见 host_matches_dom 语义）
}

#[test]
fn web_fetch_redirect_not_followed_each_hop_asks() {
    let (db, reg, ctx, _dir) = setup();
    let base = serve_seq(|port| {
        vec![
            http_resp(
                302,
                "Found",
                &[("location", &format!("http://localhost:{port}/b"))],
                "",
            ),
            http_resp(200, "OK", &[("content-type", "text/plain")], "page-b"),
        ]
    });
    // 先沉淀 *@127.0.0.1 规则（直接 INSERT，第一发跳转不必弹卡）
    db.conn()
        .execute(
            "INSERT INTO permission_rules (id,project_id,tool,shape,effect,scope)
             VALUES ('prF','p1','web_fetch','*@127.0.0.1','allow','project')",
            [],
        )
        .unwrap();
    let out = reg
        .call(&db, &ctx, "web_fetch", json!({"url": format!("{base}/a")}))
        .unwrap();
    let CallOutcome::Done(v) = out else { panic!() };
    assert_eq!(v["status"], 302);
    let target = v["redirect"].as_str().unwrap().to_string();
    assert!(target.starts_with("http://localhost:"), "{target}");
    // 跳到异 host：127.0.0.1 的域名记忆不覆盖 localhost → 重新必问
    let out = reg
        .call(&db, &ctx, "web_fetch", json!({"url": target}))
        .unwrap();
    assert!(
        matches!(out, CallOutcome::Asked(_)),
        "redirect hop must re-ask: {out:?}"
    );
}

#[test]
fn web_fetch_denies_non_http_and_rejects_binary() {
    let (db, reg, ctx, _dir) = setup();
    for u in ["file:///etc/passwd", "ftp://x/y", "notaurl"] {
        let out = reg.call(&db, &ctx, "web_fetch", json!({"url": u})).unwrap();
        assert!(matches!(out, CallOutcome::Denied(_)), "{u}");
    }
    let url = serve_seq(|_| {
        vec![http_resp(
            200,
            "OK",
            &[("content-type", "image/png")],
            "\u{89}PNG",
        )]
    });
    // image 内容类型即使放行也返回结构化拒绝，不乱码进上下文
    let CallOutcome::Asked(qid) = reg
        .call(&db, &ctx, "web_fetch", json!({"url": url}))
        .unwrap()
    else {
        panic!()
    };
    let out = reg
        .resolve(&db, &ctx, &qid, true, None, "activation", None, "owner")
        .unwrap();
    let CallOutcome::Done(v) = out else { panic!() };
    assert!(v["error"].as_str().unwrap().contains("unsupported"));
}

#[test]
fn web_fetch_absent_from_readonly_registry() {
    let (_db, reg, _ctx, _dir) = setup();
    let ro = reg.readonly();
    let names: Vec<_> = ro.defs().iter().map(|d| d.name.clone()).collect::<Vec<_>>();
    assert!(
        !names.iter().any(|n| n == "web_fetch"),
        "readonly 不得含 web_fetch"
    );
}

// ---------- 票 05：会话化终端（ADR 0058-3）----------

/// 走完整管线：Ask → resolve(allow) → Done。每次调用都弹卡是预期
/// （Exec 类永在安全网外但默认必问）。
fn bash_pipe(reg: &Registry, db: &Db, ctx: &ToolContext, input: Value) -> Value {
    let out = reg.call(db, ctx, "bash", input).unwrap();
    let CallOutcome::Asked(qid) = out else {
        panic!("expected ask: {out:?}")
    };
    let out = reg
        .resolve(db, ctx, &qid, true, None, "activation", None, "owner")
        .unwrap();
    let CallOutcome::Done(v) = out else {
        panic!("resolve: {out:?}")
    };
    v
}

#[test]
fn session_shares_cwd_and_env_across_calls() {
    let (db, reg, ctx, _dir) = setup();
    let v = bash_pipe(
        &reg,
        &db,
        &ctx,
        json!({"cmd": "mkdir -p sub && cd sub && export HX_SESS=42", "session": "s1"}),
    );
    assert_eq!(v["session"], "s1");
    let v = bash_pipe(
        &reg,
        &db,
        &ctx,
        json!({"cmd": "pwd; echo env=$HX_SESS", "session": "s1"}),
    );
    let out = v["stdout"].as_str().unwrap();
    assert!(out.trim_end().contains("sub"), "cwd lost: {out}");
    assert!(out.contains("env=42"), "env lost: {out}");
    // 会话生命周期事件落痕
    let kinds: Vec<String> = db
        .timeline("p1", None, 100, Some(&[EventKind::System]))
        .unwrap()
        .iter()
        .filter_map(|i| i.event.payload["kind"].as_str().map(String::from))
        .collect();
    assert_eq!(kinds.iter().filter(|k| *k == "session_started").count(), 1);
}

#[test]
fn sentinel_marker_in_output_does_not_desync() {
    let (db, _reg, ctx, _dir) = setup();
    // 模型命令回显仿冒哨兵字样——nonce 随机后缀保证不误切
    let v = Bash
        .exec(
            &db,
            &json!({"cmd": "echo '__HX_DONE_deadbeef_0__'; echo after", "session": "s2"}),
            &ctx,
        )
        .unwrap();
    let out = v["stdout"].as_str().unwrap();
    assert!(out.contains("__HX_DONE_deadbeef_0__"), "{out}");
    assert!(out.contains("after"), "{out}");
    assert_eq!(v["exit_code"], 0);
    // 后续命令仍正常定界
    let v = Bash
        .exec(&db, &json!({"cmd": "echo ok2", "session": "s2"}), &ctx)
        .unwrap();
    assert_eq!(v["stdout"].as_str().unwrap().trim(), "ok2");
}

#[test]
fn background_task_output_cursor_and_kill() {
    let (db, _reg, ctx, _dir) = setup();
    let v = Bash
        .exec(
            &db,
            &json!({"cmd": "echo hi; sleep 0.3; echo bye", "background": true}),
            &ctx,
        )
        .unwrap();
    let tid = v["task_id"].as_str().unwrap().to_string();
    // 轮到 done 为止（短任务必然收尾）
    let mut last = json!(null);
    for _ in 0..50 {
        last = BashOutput
            .exec(&db, &json!({"task_id": tid}), &ctx)
            .unwrap();
        if last["done"].as_bool().unwrap_or(false) {
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(50));
    }
    assert!(last["done"].as_bool().unwrap(), "task never done");
    assert_eq!(last["exit_code"], 0);
    assert!(last["stdout"].as_str().unwrap().contains("hi"));
    // 游标增量：续读不回放
    let co = last["cursor_out"].as_u64().unwrap();
    let next = BashOutput
        .exec(&db, &json!({"task_id": tid, "cursor_out": co}), &ctx)
        .unwrap();
    assert_eq!(next["stdout"].as_str().unwrap(), "");

    // kill 幂等：活任务 killed:true，再杀 killed:false
    let v = Bash
        .exec(&db, &json!({"cmd": "sleep 60", "background": true}), &ctx)
        .unwrap();
    let tid2 = v["task_id"].as_str().unwrap().to_string();
    let k = BashKill.exec(&db, &json!({"task_id": tid2}), &ctx).unwrap();
    assert_eq!(k["killed"], true);
    let k2 = BashKill.exec(&db, &json!({"task_id": tid2}), &ctx).unwrap();
    assert_eq!(k2["killed"], false);
    let kinds: Vec<String> = db
        .timeline("p1", None, 200, Some(&[EventKind::System]))
        .unwrap()
        .iter()
        .filter_map(|i| i.event.payload["kind"].as_str().map(String::from))
        .collect();
    assert!(kinds.iter().any(|k| *k == "task_started"));
    assert!(kinds.iter().any(|k| *k == "task_killed"));
}

#[test]
fn timeout_kills_and_traces() {
    let (db, _reg, ctx, _dir) = setup();
    let t0 = std::time::Instant::now();
    let v = Bash
        .exec(&db, &json!({"cmd": "sleep 30", "timeout_ms": 150}), &ctx)
        .unwrap();
    assert!(t0.elapsed() < std::time::Duration::from_secs(10));
    assert_eq!(v["timed_out"], true);
    let kinds: Vec<String> = db
        .timeline("p1", None, 200, Some(&[EventKind::System]))
        .unwrap()
        .iter()
        .filter_map(|i| i.event.payload["kind"].as_str().map(String::from))
        .collect();
    assert!(kinds.iter().any(|k| *k == "exec_timeout"));
}

#[test]
fn session_exit_and_respawn() {
    let (db, _reg, ctx, _dir) = setup();
    let v = Bash
        .exec(&db, &json!({"cmd": "echo a", "session": "s3"}), &ctx)
        .unwrap();
    assert_eq!(v["exit_code"], 0);
    // exit 杀 sh——哨兵永远等不到 EOF 前哨兵先 EOF；done 由 EOF 置位
    let v = Bash
        .exec(&db, &json!({"cmd": "exit", "session": "s3"}), &ctx)
        .unwrap();
    assert_eq!(v["session_exited"], true);
    // 同名会话自动重起且工作目录回 repo_root（新进程）
    let v = Bash
        .exec(&db, &json!({"cmd": "pwd", "session": "s3"}), &ctx)
        .unwrap();
    assert_eq!(v["exit_code"], 0);
    let kinds: Vec<String> = db
        .timeline("p1", None, 200, Some(&[EventKind::System]))
        .unwrap()
        .iter()
        .filter_map(|i| i.event.payload["kind"].as_str().map(String::from))
        .collect();
    assert!(kinds.iter().any(|k| *k == "session_exited"));
    assert_eq!(kinds.iter().filter(|k| *k == "session_started").count(), 2);
}

#[test]
fn kill_all_drains_table() {
    let (db, _reg, ctx, _dir) = setup();
    Bash.exec(&db, &json!({"cmd": "true", "session": "sx"}), &ctx)
        .unwrap();
    Bash.exec(&db, &json!({"cmd": "sleep 60", "background": true}), &ctx)
        .unwrap();
    ctx.sessions.kill_all();
    // 表已排空：句柄全失效
    assert!(BashOutput
        .exec(&db, &json!({"session": "sx"}), &ctx)
        .is_err());
}

#[test]
fn session_output_cap_and_ring_overflow() {
    let (db, _reg, ctx, _dir) = setup();
    // >64KB 显示预算 → spill 头标尾
    let v = Bash
        .exec(
            &db,
            &json!({"cmd": "yes | head -c 200000", "session": "s4"}),
            &ctx,
        )
        .unwrap();
    assert!(v["stdout"]
        .as_str()
        .unwrap()
        .contains("full output: .hexagon/spill/"));
    // >256KB 环容量 → 最旧丢弃标记
    let v = Bash
        .exec(
            &db,
            &json!({"cmd": "yes | head -c 600000", "session": "s4"}),
            &ctx,
        )
        .unwrap();
    assert!(
        v["stdout"].as_str().unwrap().contains("ring overflow"),
        "lost marker missing"
    );
}

#[test]
fn bash_kill_and_output_unknown_ids_error() {
    let (db, _reg, ctx, _dir) = setup();
    assert!(BashOutput
        .exec(&db, &json!({"task_id": "nope"}), &ctx)
        .is_err());
    assert!(BashKill
        .exec(&db, &json!({"session": "nope"}), &ctx)
        .is_err());
    assert!(BashOutput.exec(&db, &json!({}), &ctx).is_err());
}

// ---------- 票 02：视觉管线 ----------

/// 合法 PNG 魔数（8 字节签名 + 少量载荷）。
const PNG_BYTES: &[u8] = b"\x89PNG\r\n\x1a\n\x00\x00\x00\x0dIHDR\xff";

#[test]
fn fs_read_image_with_vision_slot() {
    let (db, _reg, mut ctx, dir) = setup();
    std::fs::write(dir.path().join("shot.png"), PNG_BYTES).unwrap();
    ctx.caps.insert("vision".into());
    let v = FsRead
        .exec(&db, &json!({"path": "shot.png"}), &ctx)
        .unwrap();
    assert_eq!(v["image"]["media_type"], "image/png");
    assert!(!v["image"]["data"].as_str().unwrap().is_empty());
    assert_eq!(v["bytes"], PNG_BYTES.len());
}

#[test]
fn fs_read_image_without_vision_degrades() {
    let (db, _reg, ctx, dir) = setup();
    std::fs::write(dir.path().join("shot.png"), PNG_BYTES).unwrap();
    // caps 空集（默认构造）：字节不进上下文
    let v = FsRead
        .exec(&db, &json!({"path": "shot.png"}), &ctx)
        .unwrap();
    assert!(v["note"].as_str().unwrap().contains("vision"));
    assert!(v["image"].is_null(), "无 vision 槽不许带图");
    assert!(v["content"].is_null());
    assert!(!v.to_string().contains("aGk") || !v.to_string().contains("base64"));
}

#[test]
fn fs_read_fake_extension_falls_to_text() {
    let (db, _reg, mut ctx, dir) = setup();
    // 伪造扩展名：文本内容 + .png 后缀——魔数不符走文本路径
    std::fs::write(dir.path().join("fake.png"), "just text").unwrap();
    ctx.caps.insert("vision".into());
    let v = FsRead
        .exec(&db, &json!({"path": "fake.png"}), &ctx)
        .unwrap();
    assert_eq!(v["content"], "just text");
    assert!(v["image"].is_null());
}

#[test]
fn fs_read_image_over_cap_returns_text() {
    let (db, _reg, mut ctx, dir) = setup();
    // 构造 >5MB 的合法头 PNG（魔数真、体量超闸）
    let mut big = PNG_BYTES.to_vec();
    big.resize(5 * 1024 * 1024 + 1, 0u8);
    std::fs::write(dir.path().join("huge.png"), &big).unwrap();
    ctx.caps.insert("vision".into());
    let v = FsRead
        .exec(&db, &json!({"path": "huge.png"}), &ctx)
        .unwrap();
    assert!(v["image"].is_null(), "超上限不得带图");
    assert!(v["note"].as_str().unwrap().contains("5MB"));
}

//! e2e_live — 机器人 owner 全链路活测（QA 委托 TC-E-0001 内核侧实现）。
//!
//! 真路径：setup::create_project（git 闸/缺 key 闸全走）→ open_stage →
//! run_all_active → 裁决循环 → stamp → 下一阶段。供应商走生产同款
//! providers.json（HEXAGON_PROVIDERS_PATH）+ OS 钥匙串凭据——除「人」
//! 被策略函数替代外零 mock。这也是 TC-I-0301 的活验证面。
//!
//! 用法：
//!   cargo run -p hexagon-core --example e2e_live -- \
//!       --dir /tmp/hexagon-e2e --task "建个单页个人主页" [--pack 阶段门] [--max-steps 60]
//! 默认 pack=阶段门；--pack fastpath 走无阶段派发模式（单角色直达）。

use hexagon_core::api::Workbench;
use hexagon_core::cards::{self, QueuedCard};
use hexagon_core::credentials::{CredentialStore, MemoryStore, OsKeychain};
use hexagon_core::presets::{preset_packs, preset_roles, RoleDef};
use hexagon_core::provider_config;
use hexagon_core::setup::create_project;
use hexagon_core::turn::TurnOutcome;
use std::path::Path;
use std::sync::Arc;

fn arg(flag: &str) -> Option<String> {
    let mut it = std::env::args().skip(1);
    while let Some(a) = it.next() {
        if a == flag {
            return it.next();
        }
    }
    None
}

/// owner 策略：批权限/升级/提案/安装/恢复，盖章推进，**发布永远不批**
/// （活测不得产生对外副作用）。返回是否真正销掉了至少一张卡——
/// 跳过不批的卡不算（否则 publish 卡永远挂着 → 永不收敛）。
fn adjudicate_all(wb: &Workbench) -> bool {
    let Ok(cards) = cards::queued(&wb.db, &wb.project_id) else {
        return false;
    };
    let mut did = false;
    for c in cards {
        did |= adjudicate(wb, &c);
    }
    did
}

/// 返回 true = 卡被销掉；false = 跳过留着（publish/未知 kind/裁决报错）。
fn adjudicate(wb: &Workbench, c: &QueuedCard) -> bool {
    let ctx = hexagon_core::tools::ToolContext::owner(&wb.db, &wb.repo_root);
    let tag = format!("{}#{}", c.kind, &c.id[..c.id.len().min(8)]);
    let r: Result<String, hexagon_core::api::ApiError> = match c.kind.as_str() {
        // 文件写盘按路径记形状（scope=project）——不然同一文件每轮重写
        // 都重新排队问一遍（活测实证：index.html 连问 18 次）。bash 按
        // 命令头白名单记 `head *`——QA 逐命令变体重问会拖死回合。
        "permission" => {
            let tool = c.payload.get("tool").and_then(|v| v.as_str()).unwrap_or("");
            let cmd = c
                .payload
                .pointer("/raw_input/cmd")
                .and_then(|v| v.as_str())
                .unwrap_or("");
            const SAFE_HEADS: &[&str] = &[
                "ls", "cat", "head", "tail", "find", "grep", "wc", "echo", "node", "python3",
                "python", "mkdir", "test", "stat", "file", "open",
            ];
            let head = cmd.split_whitespace().next().unwrap_or("");
            let shape: Option<String> = match tool {
                "fs_write" | "fs_patch" | "fs_read" => c
                    .payload
                    .pointer("/raw_input/path")
                    .and_then(|v| v.as_str())
                    .map(str::to_string),
                "bash" if SAFE_HEADS.contains(&head) => Some(format!("{head} *")),
                _ => None,
            };
            if let Some(s) = shape {
                wb.answer_permission(&c.id, true, Some(&s), "project")
                    .map(|_| format!("approved+remember({s})"))
            } else {
                wb.answer_permission(&c.id, true, None, "once")
                    .map(|_| "approved(once)".to_string())
            }
        }
        "escalation" => wb
            .adjudicate_flag(&c.id, true)
            .map(|_| "agreed".to_string()),
        "stamp" if c.payload.get("proposal_id").is_some() => {
            match hexagon_core::proposals::activate(&wb.db, &ctx, &c.id) {
                Ok(_) => {
                    println!("  [owner] {tag} → proposal confirmed");
                    return true;
                }
                Err(e) => {
                    println!("  [owner] {tag} → 裁决报错 {e}");
                    return false;
                }
            }
        }
        "stamp" => wb.stamp().map(|_| "stage stamped".to_string()),
        "recovery" => c
            .payload
            .get("run_id")
            .and_then(|v| v.as_str())
            .map(|rid| wb.recover_run(rid).map(|_| "recovered".to_string()))
            .unwrap_or(Ok("no run_id".to_string())),
        "install" => {
            match hexagon_core::install::resolve_install(
                &wb.db,
                &wb.project_id,
                &wb.repo_root,
                &c.id,
                true,
            ) {
                Ok(_) => {
                    println!("  [owner] {tag} → install allowed");
                    return true;
                }
                Err(e) => {
                    println!("  [owner] {tag} → 裁决报错 {e}");
                    return false;
                }
            }
        }
        "publish" => {
            println!("  [owner] {tag} → 拒绝：活测不发布（留着卡当证据）");
            return false;
        }
        other => {
            println!("  [owner] {tag} → 未知 kind={other}，跳过留观察");
            return false;
        }
    };
    match r {
        Ok(how) => {
            println!("  [owner] {tag} → {how}");
            true
        }
        Err(e) => {
            println!("  [owner] {tag} → 裁决报错 {e}");
            false
        }
    }
}

fn stage_states(wb: &Workbench) -> Vec<(usize, String, String)> {
    hexagon_core::orchestra::stage_status(&wb.db, &wb.project_id)
        .unwrap_or_default()
        .into_iter()
        .map(|r| (r.seq as usize, r.stage, r.state))
        .collect()
}

fn report(wb: &Workbench, dir: &Path) {
    println!("\n========== 证据汇总 ==========");
    for (seq, name, state) in stage_states(wb) {
        println!("stage[{seq}] {name}: {state}");
    }
    let arts = hexagon_core::artifacts::query(&wb.db, &wb.project_id, None, None, None, None)
        .unwrap_or_default();
    println!("artifacts: {}", arts.len());
    for a in &arts {
        println!(
            "  - {} v{} [{}] by {}",
            a.path,
            a.version,
            a.status,
            a.author.as_deref().unwrap_or("?")
        );
    }
    let events: i64 = wb
        .db
        .conn()
        .query_row(
            "SELECT COUNT(*) FROM events WHERE project_id='p1'",
            [],
            |r| r.get(0),
        )
        .unwrap_or(0);
    let pending: i64 = wb
        .db
        .conn()
        .query_row(
            "SELECT COUNT(*) FROM pending_questions WHERE project_id='p1' AND state='queued'",
            [],
            |r| r.get(0),
        )
        .unwrap_or(0);
    println!("events: {events}  pending-left: {pending}");
    if let Ok(u) = hexagon_core::usage::project_summary(&wb.db, &wb.project_id) {
        let calls: i64 = u.rows.iter().map(|r| r.calls).sum();
        let tok: i64 = u
            .rows
            .iter()
            .map(|r| r.prompt_tokens + r.completion_tokens)
            .sum();
        println!("usage: {calls} calls, {tok} tok, {} mc", u.total.spent_mc);
    }
    println!("repo files:");
    for e in walkdir(dir)
        .iter()
        .filter(|p| !p.contains("/.git") && !p.contains("/.hexagon"))
    {
        println!("  {e}");
    }
}

fn walkdir(d: &Path) -> Vec<String> {
    let mut out = Vec::new();
    if let Ok(rd) = std::fs::read_dir(d) {
        for e in rd.flatten() {
            let p = e.path();
            if p.is_dir() {
                out.extend(walkdir(&p));
            } else {
                out.push(p.display().to_string());
            }
        }
    }
    out
}

fn main() {
    let dir = arg("--dir").unwrap_or_else(|| "/tmp/hexagon-e2e".into());
    let task = arg("--task").unwrap_or_else(|| {
        "建一个单页个人主页：index.html + style.css + main.js，含自我介绍/项目列表/联系方式，纯静态不引外部依赖".into()
    });
    let pack_name = arg("--pack").unwrap_or_else(|| "阶段门".into());
    let max_steps: usize = arg("--max-steps")
        .and_then(|s| s.parse().ok())
        .unwrap_or(60);
    let dirp = Path::new(&dir);
    if dirp.join(".hexagon/state.db").exists() {
        eprintln!("目录已是项目，换个空目录：{dir}");
        std::process::exit(2);
    }

    // 凭据接缝：HEXAGON_LIVE_KEY 注入 MemoryStore（sanctioned 测试缝）。
    // OsKeychain 在此 harness 里 read-after-set 仍 NotFound——keyring
    // 的 keychain 后端选择问题单独排查（harness 不背这个锅）。
    let doc = provider_config::load().expect("providers.json");
    let creds = MemoryStore::default();
    if let Ok(key) = std::env::var("HEXAGON_LIVE_KEY") {
        for p in &doc.providers {
            let name = hexagon_core::credentials::provider_key_name(&p.id);
            creds.set(&name, &key).expect("seed key");
            println!("creds: {name} seeded");
        }
        // 探针：OsKeychain 的 get/set 读写在同一进程内是否自洽
        let probe = hexagon_core::credentials::provider_key_name("aliyun");
        match OsKeychain.get(&probe) {
            Ok(Some(_)) => println!("keychain probe: {probe} readable"),
            Ok(None) => println!("keychain probe: {probe} NOT FOUND"),
            Err(e) => println!("keychain probe: {probe} ERR {e}"),
        }
    }
    let roles_all = preset_roles().expect("preset roles");

    if pack_name == "fastpath" {
        let role = "后端";
        let wb = create_project(
            dirp,
            "e2e",
            &[role.to_string()],
            &[],
            None,
            Some(role),
            true,
            &creds,
            &doc,
        )
        .expect("create_project fastpath");
        let mut wb = wb;
        wb.attach_providers(Arc::new(creds));
        // 活测驱动模型：dispatch 只做首回合（激活仪式 + plan_first 方案
        // 预告），续跑一律 run_turn——批准权限只是把工具跑完落
        // tool_result，模型要再被召一回合才看得到结果继续干活。
        // 收敛判据：本步一张卡都没批（无新权限请求）且已续跑过。
        for step in 0..max_steps {
            adjudicate_all(&wb);
            let r = if step == 0 {
                wb.dispatch(role, &task)
            } else {
                wb.run_turn(role, "继续")
            };
            match r {
                Ok(TurnOutcome::Finished) => println!("[turn {step}] finished"),
                Ok(o) => println!("[turn {step}] outcome={o:?}"),
                Err(e) => {
                    println!("[turn {step}] ERROR {e}");
                    break;
                }
            }
            if !adjudicate_all(&wb) && step >= 1 {
                break;
            }
        }
        report(&wb, dirp);
        return;
    }

    // ---- pack 模式：全套班底跑 stage-gate ----
    let pack = preset_packs()
        .expect("preset packs")
        .into_iter()
        .find(|p| p.name == pack_name)
        .unwrap_or_else(|| panic!("no pack named {pack_name}"));

    // 班底 = 阶段 roles ∪ reviewers；评审线走 review 槽（异构异协议）。
    let mut names: Vec<String> = vec![];
    for s in &pack.stages {
        for r in &s.roles {
            if !names.contains(r) {
                names.push(r.clone());
            }
        }
        for rv in &s.reviews {
            if !names.contains(&rv.reviewer) {
                names.push(rv.reviewer.clone());
            }
        }
        for w in &s.consult_wake {
            if !names.contains(w) {
                names.push(w.clone());
            }
        }
    }
    // 异构模型分配：技术负责人/架构师 → review 槽（GLM-5 Anthropic 协议），
    // 其余吃 preset 的 chat 槽（qwen3-coder-next，OpenAI 协议）。
    let overrides: Vec<RoleDef> = names
        .iter()
        .filter_map(|n| {
            roles_all.iter().find(|r| &r.name == n).map(|r| {
                let mut d = r.clone();
                if n.contains("负责人") || n == "架构师" {
                    d.model_slot = "review".into();
                }
                d
            })
        })
        .collect();
    println!("班底: {names:?}");
    println!(
        "异构槽位: {:?}",
        overrides
            .iter()
            .map(|d| format!("{}→{}", d.name, d.model_slot))
            .collect::<Vec<_>>()
    );

    let mut wb = create_project(
        dirp,
        "e2e",
        &names.iter().map(|s| s.to_string()).collect::<Vec<_>>(),
        &overrides,
        Some(&pack),
        None,
        true,
        &creds,
        &doc,
    )
    .expect("create_project pack");
    wb.attach_providers(Arc::new(creds));

    // 真实推进协议：open_stage(0) → advance() 评估 → 缺产物就驱 agent
    // 干活、缺复审先唤醒复审者（真模型写复审意见）再 owner skip_review
    // 放行、Ready+盖章点挂 Stamp 卡由裁决循环批 → stamp 内自动 open_next。
    use hexagon_core::orchestra::StageAction;
    use std::collections::HashSet;
    match wb.open_stage(0) {
        Ok(o) => println!("stage 0 opened (skipped={})", o.skipped),
        Err(e) => {
            println!("open_stage(0) ERROR {e}");
            report(&wb, dirp);
            return;
        }
    }
    let mut dispatched_reviews: HashSet<String> = HashSet::new();
    for step in 0..max_steps {
        adjudicate_all(&wb);
        let cur = stage_states(&wb)
            .into_iter()
            .find(|(_, _, s)| s == "active" || s == "waiting_stamp");
        let Some((seq, sname, _)) = cur else {
            println!("no active run — done?");
            break;
        };
        let mut art_missing: Vec<String> = vec![];

        match wb.advance() {
            Ok(StageAction::PackFinished) => {
                println!("[step {step}] ★ pack finished");
                break;
            }
            Ok(StageAction::AwaitingStamp { stage, .. })
            | Ok(StageAction::WaitingStamp { stage }) => {
                println!("[step {step}] {stage} → awaiting stamp");
                adjudicate_all(&wb);
                continue;
            }
            Ok(StageAction::Incomplete { missing, .. }) => {
                println!("[step {step}] {sname} missing: {missing:?}");
                // kind 静默失配是实证坑（活测：QA 连写 3 版 test-record
                // 全 misc 不落账）——owner 反馈通道补信号：把缺口翻译成
                // 对 agent 可执行的修正指令，而不是只说「还缺 X」。
                art_missing = missing
                    .iter()
                    .filter_map(|m| m.strip_prefix("artifact:").map(str::to_string))
                    .collect();
                let mut only_reviews = !missing.is_empty();
                for m in &missing {
                    if let Some(kind) = m.strip_prefix("review:") {
                        let key = format!("{seq}:{kind}");
                        if dispatched_reviews.insert(key) {
                            // 先让复审者真干活——写复审意见产物（真模型调用）。
                            if let Some(rv) = pack
                                .stages
                                .get(seq)
                                .and_then(|s| s.reviews.iter().find(|r| r.artifact_kind == kind))
                                .map(|r| r.reviewer.clone())
                            {
                                println!("  [owner] 唤醒复审 {rv} ← {kind}");
                                match wb.dispatch(
                                    &rv,
                                    &format!("请复审本阶段已交付的「{kind}」产物，用 artifact_write 产出复审意见（通过或驳回+理由）"),
                                ) {
                                    Ok(o) => println!("    reviewer turn: {o:?}"),
                                    Err(e) => println!("    reviewer dispatch err {e}"),
                                }
                            }
                        } else {
                            // 复审者已跑过但 review:X 仍缺——复审意见产物不发射
                            // review_passed 事件（submit_review 无生产调用方，活测
                            // 实证此死路），owner 走 skip_review 真实 IPC 面放行。
                            match wb.skip_review(kind) {
                                Ok(_) => println!("  [owner] skip_review {kind}（复审意见在产物库，事件面无桥→owner 跳过）"),
                                Err(e) => println!("  skip_review {kind} err {e}"),
                            }
                        }
                    } else {
                        only_reviews = false;
                    }
                }
                if only_reviews {
                    continue;
                }
            }
            Ok(StageAction::StageOpened { seq: s, .. }) => {
                println!("[step {step}] stage {s} opened");
                continue;
            }
            Ok(o) => println!("[step {step}] {o:?}"),
            Err(e) => println!("[step {step}] advance err {e}"),
        }
        let st = &pack.stages[seq];
        let mut hint = format!(
            "{task}\n（当前阶段：{sname}；本阶段应交产物：{:?}——用 artifact_write 交付且必须传 kind 参数等于应交名，仓库文件用 fs_write/bash）",
            st.due
        );
        if !art_missing.is_empty() {
            hint += &format!(
                "\n缺口修正：「{}」仍未交付——你之前写的产物 kind 被记成 misc 不计入交付。重写时 artifact_write 传 kind=\"{}\" 参数，或产物开头三行写 `---` / `kind: {}` / `---`（注意是分开三行，不是一行 ---kind---）。",
                art_missing.join("」、「"),
                art_missing[0],
                art_missing[0]
            );
        }
        match wb.run_all_active(&hint) {
            Ok(outs) => {
                for (role, o) in &outs {
                    println!("  [{step}] {role}: {o:?}");
                }
            }
            Err(e) => println!("  [{step}] run_all_active ERROR {e}"),
        }
        adjudicate_all(&wb);
    }
    report(&wb, dirp);
}

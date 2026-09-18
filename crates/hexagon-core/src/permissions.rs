//! 权限管线：五层求值（严格顺序）。
//!
//! 1. 内置 deny（最高优先，不可覆盖）—— 在各 Tool::builtin_deny；
//! 2. 安全网必问 —— 基线合入/远程发布/.git 内部改写/删仓根等不可逆高危，
//!    永远必问且**永不进入记忆**；
//! 3. 项目级 deny —— permission_rules effect='deny'，压过一切记忆 allow；
//! 4. 形状化记忆 allow —— tool + 命令/路径/域形 + 作用域（activation 绑授予时的
//!    stage_run / project 长效）；网络规则绑域名；
//! 5. 默认 ask 负责人 —— 必问卡通道（批准一次/拒绝/记住形状）。
//!
//! 负责人离开时请求挂起排队，不自动拒绝。
//! 确认的检验命令沉淀为 Bash 形状授权（两本账合一）。

use crate::db::Db;
use crate::orchestra::PackDef;
use crate::tools::{glob_match, Tool, ToolContext};
use serde_json::Value;

#[derive(Debug, PartialEq)]
pub enum Decision {
    /// 放行（含记忆命中）
    Allow { via: AllowVia },
    /// 转必问
    Ask { reason: String, safety_net: bool },
    /// 拒绝
    Deny { reason: String, layer: &'static str },
}

#[derive(Debug, PartialEq)]
pub enum AllowVia {
    /// 无规则命中、非安全网、非必问类工具（如仓内读）
    Default,
    /// 形状化记忆命中
    Remembered { shape: String, scope: String },
}

/// 安全网：命令/路径命中清单即必问。清单是常量——不进记忆、不可降级。
pub fn is_safety_net(tool: &str, input: &Value) -> Option<&'static str> {
    match tool {
        "bash" => {
            let cmd = input["cmd"].as_str().unwrap_or("").to_lowercase();
            const PATTERNS: &[(&str, &str)] = &[
                ("git push", "remote publish"),
                ("git merge", "baseline merge"),
                ("git rebase", "baseline rewrite"),
                ("git reset --hard", "worktree rewrite"),
                ("git update-ref", "git internals"),
                ("git filter-branch", "history rewrite"),
                ("git checkout --", "worktree rewrite"),
                ("git clean", "worktree rewrite"),
                ("rm -rf", "destructive delete"),
            ];
            PATTERNS
                .iter()
                .find(|(p, _)| cmd.contains(p))
                .map(|(_, label)| *label)
        }
        "fs_write" | "fs_patch" | "artifact_write" => {
            let p = input["path"].as_str().unwrap_or("");
            if p == "." || p == "/" || p.starts_with(".git/") || p == ".git" {
                Some("git internals / repo root")
            } else {
                None
            }
        }
        // 未来工具名的占位：基线合入与远程发布工具天生安全网
        "git_baseline_merge" => Some("baseline merge"),
        "remote_publish" => Some("remote publish"),
        _ => None,
    }
}

/// 形状匹配：`npm install *` / `npm install *@registry.npmjs.org` / 路径 glob。
/// `@domain` 后缀把规则绑到域名：命令必须实际引用该域。
pub fn shape_matches(shape: &str, tool: &str, input: &Value) -> bool {
    let target = match tool {
        "bash" => input["cmd"].as_str().unwrap_or(""),
        "fs_read" | "fs_write" | "fs_patch" | "artifact_write" | "artifact_read" => {
            input["path"].as_str().unwrap_or("")
        }
        _ => return false,
    };
    // @域名 绑定：shape 里 `*@dom` → 目标必须含该域
    if let Some((head, dom)) = shape.split_once('@') {
        if !target.contains(dom) {
            return false;
        }
        return token_match(head.trim_end_matches('*').trim_end(), target, true);
    }
    match tool {
        "bash" => token_match(shape, target, false),
        _ => glob_match(shape, target),
    }
}

/// 命令形匹配：按空白分词，`*` 匹配任意单段（含空）。
fn token_match(shape: &str, target: &str, shape_is_prefix: bool) -> bool {
    let sp: Vec<&str> = shape.split_whitespace().collect();
    let tp: Vec<&str> = target.split_whitespace().collect();
    fn m(s: &[&str], t: &[&str]) -> bool {
        if s.is_empty() {
            return t.is_empty();
        }
        if s[0] == "*" {
            // `*` 至少吃一段：「cargo *」不匹配裸「cargo」（宽松面是安全隐患）
            return (1..=t.len()).any(|i| m(&s[1..], &t[i..]));
        }
        !t.is_empty() && s[0] == t[0] && m(&s[1..], &t[1..])
    }
    if shape_is_prefix {
        // 已剥掉 *@dom 的头部：只要目标是 shape 前缀的延续即可
        sp.len() <= tp.len() && m(&sp, &tp[..sp.len()])
    } else {
        m(&sp, &tp)
    }
}

/// 五层求值。`tool` 用于第 1 层内置 deny。
pub fn evaluate(
    db: &Db,
    ctx: &ToolContext,
    tool: &dyn Tool,
    tool_name: &str,
    input: &Value,
) -> Result<Decision, crate::tools::ToolError> {
    // L0 授权闸门：mcp:<service>:<tool> 调用方必须在 grants 表里有该服务授权，
    // 缺席即硬拒（授权注册表是边界，不走规则、不可记忆）。
    if let Some(service) = tool_name
        .strip_prefix("mcp:")
        .and_then(|s| s.split(':').next())
    {
        let granted: bool = db
            .conn()
            .query_row(
                "SELECT COUNT(*) FROM grants g JOIN agents a ON a.id = g.agent_id
                 WHERE a.project_id=?1 AND g.agent_id=?2 AND g.kind='mcp' AND g.name=?3",
                rusqlite::params![ctx.project_id, ctx.agent_id, service],
                |r| r.get::<_, i64>(0),
            )
            .map(|n| n > 0)
            .unwrap_or(false);
        if !granted {
            return Ok(Decision::Deny {
                reason: format!("no grant for mcp service: {service}"),
                layer: "grant",
            });
        }
    }
    // L1 内置 deny
    if let Some(reason) = tool.builtin_deny(input, ctx) {
        return Ok(Decision::Deny {
            reason,
            layer: "builtin_deny",
        });
    }
    // L2 安全网必问（永不进记忆）
    if let Some(label) = is_safety_net(tool_name, input) {
        return Ok(Decision::Ask {
            reason: format!("safety net: {label}"),
            safety_net: true,
        });
    }
    // L3 项目级 deny（压过记忆 allow）
    if let Some(shape) = matching_rule(db, ctx, tool_name, input, "deny")? {
        return Ok(Decision::Deny {
            reason: format!("project deny rule: {shape}"),
            layer: "project_deny",
        });
    }
    // L4 形状化记忆 allow
    if let Some((shape, scope)) = matching_rule_scoped(db, ctx, tool_name, input, "allow")? {
        return Ok(Decision::Allow {
            via: AllowVia::Remembered { shape, scope },
        });
    }
    // 路径归属：写入越界 → 必问
    if violates_ownership(tool_name, input, ctx) {
        return Ok(Decision::Ask {
            reason: "path outside ownership".into(),
            safety_net: false,
        });
    }
    // L5：必问类工具（bash 等）默认问负责人
    if tool.needs_ask(input, ctx) {
        return Ok(Decision::Ask {
            reason: "default ask".into(),
            safety_net: false,
        });
    }
    Ok(Decision::Allow {
        via: AllowVia::Default,
    })
}

/// 带日志的求值入口包装：管线内部分支自带 return，这里统一记结论。
pub fn evaluate_logged(
    db: &Db,
    ctx: &ToolContext,
    tool: &dyn Tool,
    tool_name: &str,
    input: &Value,
) -> Result<Decision, crate::tools::ToolError> {
    let d = evaluate(db, ctx, tool, tool_name, input)?;
    match &d {
        Decision::Deny { layer, reason } => {
            log::info!("perm deny [{layer}] {tool_name}: {reason}")
        }
        Decision::Ask { reason, safety_net } => log::info!(
            "perm ask{} {tool_name}: {reason}",
            if *safety_net { " (safety-net)" } else { "" }
        ),
        Decision::Allow { via } => log::debug!("perm allow {tool_name} via {via:?}"),
    }
    Ok(d)
}

/// Agent 的路径归属 glob 列表（agent_globs 表；空 = 不做归属检查）。
pub fn agent_globs(
    db: &crate::db::Db,
    agent_id: &str,
) -> Result<Vec<String>, crate::tools::ToolError> {
    let mut st = db
        .conn()
        .prepare("SELECT glob FROM agent_globs WHERE agent_id=?1")?;
    let rows = st
        .query_map([agent_id], |r| r.get(0))?
        .collect::<Result<Vec<String>, _>>()?;
    Ok(rows)
}

fn violates_ownership(tool: &str, input: &Value, ctx: &ToolContext) -> bool {
    if !matches!(tool, "fs_write" | "fs_patch" | "artifact_write") || ctx.owned_globs.is_empty() {
        return false;
    }
    let path = input["path"].as_str().unwrap_or("");
    !ctx.owned_globs.iter().any(|g| glob_match(g, path))
}

/// 命中的 deny/allow 规则形（activation 作用域要求同一 stage_run）。
fn matching_rule(
    db: &Db,
    ctx: &ToolContext,
    tool: &str,
    input: &Value,
    effect: &str,
) -> Result<Option<String>, crate::tools::ToolError> {
    Ok(matching_rule_scoped(db, ctx, tool, input, effect)?.map(|(s, _)| s))
}

fn matching_rule_scoped(
    db: &Db,
    ctx: &ToolContext,
    tool: &str,
    input: &Value,
    effect: &str,
) -> Result<Option<(String, String)>, crate::tools::ToolError> {
    let mut st = db.conn().prepare(
        "SELECT shape, scope, stage_run_id, domain FROM permission_rules
         WHERE project_id=?1 AND tool=?2 AND effect=?3",
    )?;
    let rules: Vec<(String, String, Option<String>, Option<String>)> = st
        .query_map(rusqlite::params![ctx.project_id, tool, effect], |r| {
            Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?))
        })?
        .collect::<Result<_, _>>()?;
    for (shape, scope, srid, _domain) in rules {
        let scope_ok = match scope.as_str() {
            "project" => true,
            "activation" => srid.as_deref() == ctx.stage_run_id.as_deref(),
            _ => false,
        };
        if scope_ok && shape_matches(&shape, tool, input) {
            return Ok(Some((shape, scope)));
        }
    }
    Ok(None)
}

/// 记形许可写入（resolve 裁决时调用）。安全网调用永不进记忆——返回 false。
/// 若批准的是包内声明的检验命令，自动沉淀为 project 级 Bash 形（两本账合一）。
pub fn persist_rule(
    db: &Db,
    ctx: &ToolContext,
    tool: &str,
    input: &Value,
    shape: Option<&str>,
    scope: &str,
    pack: Option<&PackDef>,
) -> Result<bool, crate::tools::ToolError> {
    if is_safety_net(tool, input).is_some() {
        return Ok(false);
    }
    let (shape, scope) = if let Some(s) = shape {
        (s.to_string(), scope.to_string())
    } else if tool == "bash" {
        // 检验命令自动沉淀：cmd 命中包声明 checks → project 级精确形
        let cmd = input["cmd"].as_str().unwrap_or("");
        let is_check = pack
            .map(|p| p.stages.iter().any(|st| st.checks.iter().any(|c| c == cmd)))
            .unwrap_or(false);
        if !is_check {
            return Ok(false);
        }
        (cmd.to_string(), "project".to_string())
    } else {
        return Ok(false);
    };
    let srid = if scope == "activation" {
        ctx.stage_run_id.clone()
    } else {
        None
    };
    let domain = shape.split_once('@').map(|(_, d)| d.to_string());
    db.conn().execute(
        "INSERT INTO permission_rules (id, project_id, tool, shape, domain, effect, scope, stage_run_id)
         VALUES (?1,?2,?3,?4,?5,'allow',?6,?7)",
        rusqlite::params![
            format!("pr{}", db.next_id("pr")?),
            ctx.project_id,
            tool,
            shape,
            domain,
            scope,
            srid
        ],
    )?;
    Ok(true)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::orchestra::PackDef;
    use crate::tools::Bash;
    use serde_json::json;

    fn setup() -> (Db, ToolContext, tempfile::TempDir) {
        let db = Db::open_in_memory().unwrap();
        db.conn()
            .execute(
                "INSERT INTO projects (id, dir, name, mode) VALUES ('p1','/tmp/x','x','pack')",
                [],
            )
            .unwrap();
        db.conn()
            .execute(
                "INSERT INTO agents (id, project_id, role) VALUES ('a1','p1','后端')",
                [],
            )
            .unwrap();
        let dir = tempfile::tempdir().unwrap();
        (
            db,
            ToolContext {
                project_id: "p1".into(),
                agent_id: "a1".into(),
                repo_root: dir.path().to_path_buf(),
                stage_run_id: Some("sr1".into()),
                owned_globs: vec![],
                tiers: Default::default(),
            },
            dir,
        )
    }

    fn bash_ctx(db: &Db, ctx: &ToolContext, cmd: &str) -> Decision {
        evaluate(db, ctx, &Bash, "bash", &json!({"cmd": cmd})).unwrap()
    }

    #[test]
    fn l1_builtin_deny_wins_over_allow_memory() {
        let (db, ctx, _d) = setup();
        // 即使有 project allow *，凭据读仍被内置 deny
        db.conn()
            .execute(
                "INSERT INTO permission_rules (id,project_id,tool,shape,effect,scope)
             VALUES ('r','p1','fs_read','**','allow','project')",
                [],
            )
            .unwrap();
        let d = evaluate(
            &db,
            &ctx,
            &crate::tools::FsRead,
            "fs_read",
            &json!({"path":".env"}),
        )
        .unwrap();
        assert!(matches!(
            d,
            Decision::Deny {
                layer: "builtin_deny",
                ..
            }
        ));
    }

    #[test]
    fn l2_safety_net_always_asks_and_never_memorizes() {
        let (db, ctx, _d) = setup();
        // 有 allow 记忆也必问
        db.conn()
            .execute(
                "INSERT INTO permission_rules (id,project_id,tool,shape,effect,scope)
             VALUES ('r','p1','bash','git *','allow','project')",
                [],
            )
            .unwrap();
        for cmd in [
            "git push origin main",
            "git merge hexagon/work",
            "rm -rf /tmp/x",
        ] {
            let d = bash_ctx(&db, &ctx, cmd);
            assert!(
                matches!(
                    d,
                    Decision::Ask {
                        safety_net: true,
                        ..
                    }
                ),
                "{cmd}"
            );
        }
        // 且永不进记忆
        let ok = persist_rule(
            &db,
            &ctx,
            "bash",
            &json!({"cmd":"git push"}),
            Some("git push *"),
            "project",
            None,
        )
        .unwrap();
        assert!(!ok);
        let n: i64 = db
            .conn()
            .query_row("SELECT COUNT(*) FROM permission_rules", [], |r| r.get(0))
            .unwrap();
        assert_eq!(n, 1); // 只有测试预置那条
    }

    #[test]
    fn l3_project_deny_beats_remembered_allow() {
        let (db, ctx, _d) = setup();
        db.conn()
            .execute(
                "INSERT INTO permission_rules (id,project_id,tool,shape,effect,scope) VALUES
             ('a','p1','bash','npm install *','allow','project'),
             ('d','p1','bash','npm install *','deny','project')",
                [],
            )
            .unwrap();
        let d = bash_ctx(&db, &ctx, "npm install zod");
        assert!(matches!(
            d,
            Decision::Deny {
                layer: "project_deny",
                ..
            }
        ));
    }

    #[test]
    fn l4_remembered_allow_hits() {
        let (db, ctx, _d) = setup();
        db.conn()
            .execute(
                "INSERT INTO permission_rules (id,project_id,tool,shape,effect,scope)
             VALUES ('a','p1','bash','npm install *','allow','project')",
                [],
            )
            .unwrap();
        let d = bash_ctx(&db, &ctx, "npm install zod");
        assert!(matches!(
            d,
            Decision::Allow {
                via: AllowVia::Remembered { .. }
            }
        ));
        // 不命中 → 默认问
        let d2 = bash_ctx(&db, &ctx, "cargo build");
        assert!(matches!(
            d2,
            Decision::Ask {
                safety_net: false,
                ..
            }
        ));
    }

    #[test]
    fn l4_activation_scope_bound_to_stage_run() {
        let (db, mut ctx, _d) = setup();
        db.conn()
            .execute(
                "INSERT INTO permission_rules (id,project_id,tool,shape,effect,scope,stage_run_id)
             VALUES ('a','p1','bash','cargo *','allow','activation','sr1')",
                [],
            )
            .unwrap();
        assert!(matches!(
            bash_ctx(&db, &ctx, "cargo test"),
            Decision::Allow { .. }
        ));
        ctx.stage_run_id = Some("sr2".into()); // 换了激活期
        assert!(matches!(
            bash_ctx(&db, &ctx, "cargo test"),
            Decision::Ask { .. }
        ));
    }

    #[test]
    fn l4_domain_bound_rule() {
        let (db, ctx, _d) = setup();
        db.conn().execute(
            "INSERT INTO permission_rules (id,project_id,tool,shape,domain,effect,scope)
             VALUES ('a','p1','bash','npm install *@registry.npmjs.org','registry.npmjs.org','allow','project')",
            []).unwrap();
        // 引用该域 → 放行
        let d = bash_ctx(
            &db,
            &ctx,
            "npm install zod --registry https://registry.npmjs.org",
        );
        assert!(matches!(d, Decision::Allow { .. }));
        // 不引用 → 默认问（域不匹配）
        let d2 = bash_ctx(&db, &ctx, "npm install zod");
        assert!(matches!(d2, Decision::Ask { .. }));
    }

    #[test]
    fn l5_ownership_violation_asks() {
        let (db, mut ctx, _d) = setup();
        ctx.owned_globs = vec!["src/**".into()];
        let d = evaluate(
            &db,
            &ctx,
            &crate::tools::FsWrite,
            "fs_write",
            &json!({"path":"etc/x","content":"c"}),
        )
        .unwrap();
        assert!(matches!(d, Decision::Ask { .. }));
        // 界内放行
        let d2 = evaluate(
            &db,
            &ctx,
            &crate::tools::FsWrite,
            "fs_write",
            &json!({"path":"src/x","content":"c"}),
        )
        .unwrap();
        assert!(matches!(
            d2,
            Decision::Allow {
                via: AllowVia::Default
            }
        ));
    }

    #[test]
    fn confirmed_check_command_becomes_bash_shape() {
        let (db, ctx, _d) = setup();
        let pack: PackDef = serde_json::from_value(json!({
            "name":"t","version":1,"stages":[{"name":"实现","roles":["后端"],"due":[],
             "checks":["cargo test --quiet"]}]}))
        .unwrap();
        let ok = persist_rule(
            &db,
            &ctx,
            "bash",
            &json!({"cmd":"cargo test --quiet"}),
            None,
            "activation",
            Some(&pack),
        )
        .unwrap();
        assert!(ok);
        // 之后同命令直接放行（project 级沉淀）
        let d = bash_ctx(&db, &ctx, "cargo test --quiet");
        assert!(matches!(
            d,
            Decision::Allow {
                via: AllowVia::Remembered { .. }
            }
        ));
        // 非检验命令不沉淀
        let ok2 = persist_rule(
            &db,
            &ctx,
            "bash",
            &json!({"cmd":"ls"}),
            None,
            "activation",
            Some(&pack),
        )
        .unwrap();
        assert!(!ok2);
    }

    #[test]
    fn shape_token_matching() {
        assert!(shape_matches(
            "npm install *",
            "bash",
            &json!({"cmd":"npm install zod"})
        ));
        assert!(!shape_matches(
            "npm install *",
            "bash",
            &json!({"cmd":"npm ci"})
        ));
        assert!(shape_matches(
            "cargo *",
            "bash",
            &json!({"cmd":"cargo build --release"})
        ));
        assert!(!shape_matches("cargo *", "bash", &json!({"cmd":"cargo"})));
        assert!(shape_matches(
            "src/**",
            "fs_write",
            &json!({"path":"src/a/b.rs"})
        ));
    }

    // ---------- 票 28：权限不变量属性测试 ----------
    //
    // 规格 Testing Decisions：proptest 生成规则组合，断言三条不变量
    // 在任意 allow/deny/作用域组合下恒成立。

    mod prop_tests {
        use super::tests::setup;
        use super::*;
        use crate::tools::{Bash, FsRead, FsWrite};
        use proptest::prelude::*;
        use proptest::{collection, sample};
        use serde_json::json;

        /// 规则 (tool, shape, effect, scope) 生成器——形状取自真实惯用形。
        fn rule() -> impl Strategy<Value = (&'static str, &'static str, &'static str, &'static str)>
        {
            (
                sample::select(vec!["bash", "fs_read", "fs_write"]),
                sample::select(vec![
                    "npm *",
                    "cargo *",
                    "git *",
                    "src/**",
                    "**",
                    "npm install *",
                ]),
                sample::select(vec!["allow", "deny"]),
                sample::select(vec!["project", "activation"]),
            )
        }

        fn insert_rule(db: &Db, i: usize, tool: &str, shape: &str, effect: &str, scope: &str) {
            let srid = if scope == "activation" {
                Some("sr1")
            } else {
                None
            };
            db.conn()
                .execute(
                    "INSERT INTO permission_rules (id,project_id,tool,shape,effect,scope,stage_run_id)
                     VALUES (?1,'p1',?2,?3,?4,?5,?6)",
                    rusqlite::params![format!("r{i}"), tool, shape, effect, scope, srid],
                )
                .unwrap();
        }

        /// 安全网命令：不触凭据词（否则 L1 先拦，测不到 L2 语义）。
        const NET: &[&str] = &[
            "rm -rf build/",
            "git push origin main",
            "git merge feature",
            "git reset --hard HEAD~1",
            "git clean -fd",
        ];

        /// deny/allow 双向命中对：非安全网、非凭据、仓内路径。
        fn pairs() -> Vec<(&'static str, &'static str, serde_json::Value)> {
            vec![
                ("bash", "npm *", json!({"cmd":"npm install zod"})),
                ("bash", "cargo *", json!({"cmd":"cargo test"})),
                ("bash", "echo *", json!({"cmd":"echo hi"})),
                (
                    "fs_write",
                    "src/**",
                    json!({"path":"src/a.rs","content":"x"}),
                ),
                ("fs_read", "src/**", json!({"path":"src/lib.rs"})),
            ]
        }

        proptest! {
            /// 不变量①：任意规则组合下安全网永远必问（记忆 allow 不能豁免）。
            #[test]
            fn safety_net_always_asks(
                rules in collection::vec(rule(), 0..8),
                idx in 0..NET.len(),
            ) {
                let (db, ctx, _d) = setup();
                for (i, (t, s, e, sc)) in rules.iter().enumerate() {
                    insert_rule(&db, i, t, s, e, sc);
                }
                let d = evaluate(&db, &ctx, &Bash, "bash", &json!({"cmd": NET[idx]})).unwrap();
                prop_assert!(
                    matches!(d, Decision::Ask { safety_net: true, .. }),
                    "expected safety-net ask, got {d:?}"
                );
            }

            /// 不变量②：内置 deny 永不被任何规则覆盖。
            #[test]
            fn builtin_deny_uncoverable(
                rules in collection::vec(rule(), 0..8),
                pick in 0..3usize,
            ) {
                let (db, ctx, _d) = setup();
                for (i, (t, s, e, sc)) in rules.iter().enumerate() {
                    insert_rule(&db, i, t, s, e, sc);
                }
                let d = match pick {
                    0 => evaluate(&db, &ctx, &FsRead, "fs_read", &json!({"path":".env"})).unwrap(),
                    1 => evaluate(
                        &db,
                        &ctx,
                        &FsWrite,
                        "fs_write",
                        &json!({"path":"keys/id_rsa","content":"x"}),
                    )
                    .unwrap(),
                    _ => evaluate(&db, &ctx, &Bash, "bash", &json!({"cmd":"cat .env"})).unwrap(),
                };
                prop_assert!(
                    matches!(d, Decision::Deny { layer: "builtin_deny", .. }),
                    "expected builtin deny, got {d:?}"
                );
            }

            /// 不变量③：同一输入同中 allow+deny 时 deny 恒胜（与插入顺序无关）。
            #[test]
            fn deny_beats_allow(order in proptest::bool::ANY, idx in 0..5usize) {
                let (db, ctx, _d) = setup();
                let pairs = pairs();
                let (tool, shape, input) = &pairs[idx];
                let (first, second) = if order {
                    ("allow", "deny")
                } else {
                    ("deny", "allow")
                };
                insert_rule(&db, 0, tool, shape, first, "project");
                insert_rule(&db, 1, tool, shape, second, "project");
                let d = match *tool {
                    "bash" => evaluate(&db, &ctx, &Bash, "bash", input).unwrap(),
                    "fs_write" => evaluate(&db, &ctx, &FsWrite, "fs_write", input).unwrap(),
                    _ => evaluate(&db, &ctx, &FsRead, "fs_read", input).unwrap(),
                };
                prop_assert!(
                    matches!(d, Decision::Deny { layer: "project_deny", .. }),
                    "expected project deny, got {d:?}"
                );
            }
        }
    }
}

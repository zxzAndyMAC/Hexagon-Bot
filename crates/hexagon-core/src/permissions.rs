//! 权限管线：五层求值（严格顺序）。
//!
//! 1. 内置 deny（最高优先，不可覆盖）—— 在各 Tool::builtin_deny；
//! 2. 安全网必问 —— 基线合入/远程发布/.git 内部改写/删仓根等不可逆高危，
//!    **永不进入记忆**。自治 L3+ 放行这一次（见 `release_at_high_autonomy`），
//!    工具名 `remote_publish` 除外；
//! 3. 项目级 deny —— permission_rules effect='deny'，压过一切记忆 allow，
//!    也压过 L3+ 的放行；
//! 4. 形状化记忆 allow —— tool + 命令/路径/域形 + 作用域（activation 绑授予时的
//!    stage_run / project 长效）；网络规则绑域名；
//! 5. 类级默认 —— 按 Tool::risk() 分：Read/WriteLocal 放行，
//!    Egress/Exec/External 必问（必问卡通道：批准一次/拒绝/记住形状）。
//!    自治 L3+ 把这次询问放行，不写规则。
//!    External（mcp:*）焊死地板：记忆层 allow 不生效、规则写不进。
//!    这里的「L4」是记忆层，不是自治档 L4。
//!
//! 负责人离开时，L0–L2 的请求挂起排队，不自动拒绝。
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

/// 守卫判定闭集（rsi-research 票 05 裁决器不对称）：
/// 守卫只允许产出三值——拒止、转人工、无意见。**类型层不存在
/// force-allow**：放行只能来自记忆命中/类级默认（机械层）或负责人
/// 裁决（人），守卫想说「没问题」只能 Pass 把判定上推。
/// 代价模型：守卫错放 = 一次未审副作用；多转人工 = 多一张卡。
/// 实现偏向后者——所有存疑路径归 ReferHuman 或 Deny。
enum GuardVerdict {
    /// 终局拒止
    Deny { reason: String, layer: &'static str },
    /// 不担保,转必问卡
    ReferHuman { reason: String, safety_net: bool },
    /// 本守卫无意见（不等于放行）
    Pass,
}

impl GuardVerdict {
    fn into_decision(self) -> Option<Decision> {
        match self {
            Self::Pass => None,
            Self::Deny { reason, layer } => Some(Decision::Deny { reason, layer }),
            Self::ReferHuman { reason, safety_net } => Some(Decision::Ask { reason, safety_net }),
        }
    }
}

#[derive(Debug, PartialEq)]
pub enum AllowVia {
    /// 无规则命中、非安全网、非必问类工具（如仓内读）
    Default,
    /// 形状化记忆命中
    Remembered { shape: String, scope: String },
    /// 自治 L3+ 放行安全网或新的权限询问。不写 permission_rules。
    /// `level` 是存储档 3 或 4。`safety_net` 与若排队时的卡标记一致，
    /// 时间线用它区分危险操作和普通新询问。
    Autonomy {
        level: u8,
        safety_net: bool,
        reason: String,
    },
}

/// 安全网：命令/路径命中清单即必问（自治 L3+ 改为放行这一次，仍不进记忆）。
/// 清单是常量。`remote_publish` 不随高档放行，见 `release_at_high_autonomy`。
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
        "web_fetch" => input["url"].as_str().unwrap_or(""),
        "fs_read" | "fs_write" | "fs_patch" | "artifact_write" | "artifact_read" => {
            input["path"].as_str().unwrap_or("")
        }
        _ => return false,
    };
    match tool {
        "bash" => bash_shape_matches(shape, target),
        "web_fetch" => web_fetch_shape_matches(shape, target),
        _ => glob_match(shape, target),
    }
}

/// web_fetch 的 URL 形状：`https://ex.com/*` 全串 glob，或 `*@dom` /
/// `前缀@dom` 绑域名（host 边界复用 bash 的 @域语义——寄生域不命中）。
/// 出处：ADR 0058-1——出网记忆沿「形状 × 域名」既有轴，不另造词汇。
fn web_fetch_shape_matches(shape: &str, url: &str) -> bool {
    let (head, dom) = match shape.split_once('@') {
        Some((h, d)) => (h.trim_end_matches('*').trim_end(), Some(d.to_lowercase())),
        None => (shape, None),
    };
    match dom {
        Some(d) => {
            host_matches_dom(url, &d) && (head.is_empty() || glob_match(&format!("{head}*"), url))
        }
        None => glob_match(shape, url),
    }
}

// ---------- bash 复合命令拆段（openworker-borrow 票 01）----------
//
// 出处：OpenWorker coworker/permissions.py::_command_allowed/_split_commands。
// 记忆授权形状只担保它匹配到的词——旧实现把整条命令当一个 token 序列，
// `cargo *` 会放行 `cargo test && rm -rf x`（`*` 吃掉 `&&` 后的全部段），
// 复合命令是任意命令后门。修复：每段独立匹配，全中才放行。
// 不对称代价：判错方向是「多一次人工问」与「一次未审副作用」——本模块所有
// 存疑路径（词法失败、空段、不透明构造）都返回 false 即转必问，fail closed。

/// 前缀形状无法担保其内容的构造：命令/进程替换、重定向（写向授权没审过的地方）、
/// 变量展开（值在授权视野外）。出现即整命令失格。
const CMD_OPAQUE: &[&str] = &["`", "$(", "$", ">", "<", "("];
/// 参数里点名另一个程序来执行的程序：外层前缀担保不了内层。
const ARG_EXECUTORS: &[&str] = &[
    "xargs", "env", "nohup", "nice", "stdbuf", "timeout", "watch", "sudo", "doas", "ssh", "docker",
    "podman", "kubectl", "npx", "pnpx", "bunx", "uvx",
];
/// 可带内联代码的解释器（`python -c`、`node -e`、`powershell -Command`）。
const INTERPRETERS: &[&str] = &[
    "sh",
    "bash",
    "zsh",
    "dash",
    "ksh",
    "fish",
    "powershell",
    "pwsh",
    "cmd",
    "python",
    "python3",
    "node",
    "deno",
    "bun",
    "ruby",
    "perl",
    "php",
];
const INLINE_CODE_FLAGS: &[&str] = &[
    "-c",
    "-e",
    "--eval",
    "--command",
    "-Command",
    "-EncodedCommand",
];
/// 把搜索/列举工具变成执行或删除工具的标志（`find . -exec rm {} +`）。
const DANGEROUS_FLAGS: &[&str] = &["-exec", "-execdir", "-delete", "-ok", "-okdir", "-fprintf"];

/// 复合命令按分隔符拆段。引号内的分隔符不切——词法与 shell_words 同一套规则；
/// 词法失败的段不落段表，由调用处统一 fail closed。
pub(crate) fn split_commands(command: &str) -> Vec<String> {
    let mut parts = vec![String::new()];
    let mut quote: Option<char> = None;
    let mut escaped = false;
    let mut chars = command.chars().peekable();
    while let Some(c) = chars.next() {
        if escaped {
            parts.last_mut().unwrap().push('\\');
            parts.last_mut().unwrap().push(c);
            escaped = false;
            continue;
        }
        match quote {
            Some('\'') => {
                parts.last_mut().unwrap().push(c);
                if c == '\'' {
                    quote = None;
                }
            }
            Some('"') => {
                parts.last_mut().unwrap().push(c);
                if c == '"' {
                    quote = None;
                } else if c == '\\' {
                    escaped = true;
                }
            }
            _ => match c {
                '\'' | '"' => {
                    quote = Some(c);
                    parts.last_mut().unwrap().push(c);
                }
                '\\' => escaped = true,
                '&' | '|' | ';' => {
                    let next = chars.peek().copied();
                    // 双字符分隔符整体消费：&& || |&
                    if (c == '&' || c == '|') && next == Some(c) || (c == '|' && next == Some('&'))
                    {
                        chars.next();
                    }
                    parts.push(String::new());
                }
                '\n' | '\r' => parts.push(String::new()),
                c => parts.last_mut().unwrap().push(c),
            },
        }
    }
    parts
        .into_iter()
        .map(|p| p.trim().to_string())
        .filter(|p| !p.is_empty())
        .collect()
}

/// 极简 shell 词法：空白分词 + 单双引号 + 反斜杠转义。
/// 引号不闭合/悬挂转义 → Err——调用处 fail closed 转必问。
pub(crate) fn shell_words(s: &str) -> Result<Vec<String>, ()> {
    let mut out = Vec::new();
    let mut cur = String::new();
    let mut quote: Option<char> = None;
    let mut escaped = false;
    let mut has = false; // 当前 token 已开始（区分空引号 token）
    for c in s.chars() {
        if escaped {
            cur.push(c);
            escaped = false;
            has = true;
            continue;
        }
        match quote {
            Some('\'') => {
                if c == '\'' {
                    quote = None;
                } else {
                    cur.push(c);
                }
                has = true;
            }
            Some('"') => {
                match c {
                    '"' => quote = None,
                    '\\' => escaped = true,
                    c => cur.push(c),
                }
                has = true;
            }
            _ => match c {
                '\'' | '"' => {
                    quote = Some(c);
                    has = true;
                }
                '\\' => escaped = true,
                c if c.is_whitespace() => {
                    if has {
                        out.push(std::mem::take(&mut cur));
                        has = false;
                    }
                }
                c => {
                    cur.push(c);
                    has = true;
                }
            },
        }
    }
    if escaped || quote.is_some() {
        return Err(());
    }
    if has {
        out.push(cur);
    }
    Ok(out)
}

/// 该段是否可被前缀形状担保。false = 段执行的代码规则没见过：
/// 参数里点名的程序、解释器内联代码、执行/删除标志。
fn prefix_eligible(argv: &[String]) -> bool {
    let prog = argv[0]
        .rsplit(['/', '\\'])
        .next()
        .unwrap_or(&argv[0])
        .to_lowercase();
    let prog = prog.strip_suffix(".exe").unwrap_or(&prog);
    if ARG_EXECUTORS.contains(&prog) {
        return false;
    }
    if INTERPRETERS.contains(&prog)
        && argv[1..]
            .iter()
            .any(|a| INLINE_CODE_FLAGS.contains(&a.as_str()))
    {
        return false;
    }
    if argv[1..]
        .iter()
        .any(|a| DANGEROUS_FLAGS.contains(&a.to_lowercase().as_str()))
    {
        return false;
    }
    true
}

/// 从 token 提取 host：URL 取 netloc（去 userinfo/端口），裸 token 原样小写。
/// 出处：openworker `permissions._host_of`——`https://docs.python.org/x` 与
/// `docs.python.org` 两种形态都收；认不出的形态返回空串（永不命中 → fail closed）。
fn host_of(token: &str) -> String {
    let t = token.trim().to_lowercase();
    let rest = t.split_once("://").map(|(_, r)| r).unwrap_or(&t);
    let rest = rest.rsplit('@').next().unwrap_or(rest);
    let netloc = rest.split('/').next().unwrap_or("");
    netloc.split(':').next().unwrap_or("").to_string()
}

/// host 边界匹配：`host == dom` 或 `*.dom`——`registry.npmjs.org.evil.com`
/// 与 `evil-registry.npmjs.org` 都不命中（旧实现 contains 两条都放）。
fn host_matches_dom(token: &str, dom: &str) -> bool {
    let host = host_of(token);
    !host.is_empty() && (host == dom || host.ends_with(&format!(".{dom}")))
}

/// 段 token 序列与形状 token 序列匹配：`*` 至少吃一段
/// （「cargo *」不匹配裸「cargo」——宽松面是安全隐患）。
///
/// 回溯加 memo：朴素递归对 k 个星 + 长 argv 是 O(n^k)（实测 k=8/n=40
/// 跑 2.3s——arch-review 附录 B1 证实；形状来自 owner 批准的
/// remember_shape，半可信输入不该能冻结评估循环）。memo 后 O(S·T)。
fn seg_tokens_match(shape: &[&str], argv: &[String]) -> bool {
    let (sn, tn) = (shape.len(), argv.len());
    let mut memo = vec![vec![None; tn + 1]; sn + 1];
    fn m(s: &[&str], t: &[String], memo: &mut [Vec<Option<bool>>]) -> bool {
        let (si, ti) = (s.len(), t.len());
        if let Some(v) = memo[si][ti] {
            return v;
        }
        let v = if s.is_empty() {
            t.is_empty()
        } else if s[0] == "*" {
            (1..=t.len()).any(|i| m(&s[1..], &t[i..], memo))
        } else {
            !t.is_empty() && s[0] == t[0] && m(&s[1..], &t[1..], memo)
        };
        memo[si][ti] = Some(v);
        v
    }
    m(shape, argv, &mut memo)
}

/// bash 目标的形状匹配：复合命令每段独立判定，全部命中才放行。
/// 任何一段词法失败/不合格/不命中 → 整条不命中（转必问，不转 deny——
/// deny 语义是 L3 的事，形状匹配只负责「这条授权不覆盖它」）。
fn bash_shape_matches(shape: &str, cmd: &str) -> bool {
    if cmd.trim().is_empty() || CMD_OPAQUE.iter().any(|t| cmd.contains(t)) {
        return false;
    }
    let parts = split_commands(cmd);
    if parts.is_empty() {
        return false;
    }
    // @域名绑定：头部前缀 + 同段内含 host 命中该域的 token。
    // 域必须在同一段——跨段引用（`npm i x && curl https://dom`）不算数。
    let (head, dom) = match shape.split_once('@') {
        Some((h, d)) => (h.trim_end_matches('*').trim_end(), Some(d.to_lowercase())),
        None => (shape, None),
    };
    let shape_toks: Vec<&str> = head.split_whitespace().collect();
    for part in &parts {
        let Ok(argv) = shell_words(part) else {
            return false;
        };
        if argv.is_empty() || !prefix_eligible(&argv) {
            return false;
        }
        let seg_ok = match &dom {
            Some(d) => {
                argv.len() >= shape_toks.len()
                    && shape_toks
                        .iter()
                        .zip(argv.iter())
                        .all(|(s, a)| *s == "*" || s == a)
                    && argv.iter().any(|t| host_matches_dom(t, d))
            }
            None => seg_tokens_match(&shape_toks, &argv),
        };
        if !seg_ok {
            return false;
        }
    }
    true
}

/// bash 命令里 path 形态的操作数是否越出 repo_root（票 07）。
/// 参照 `coworker/readonly.py::read_targets` 但提取更粗：不做按命令的
/// 操作数表，凡 path 形态 token 都判——少一张表换的是更保守的覆盖。
fn bash_operand_escapes(input: &Value, ctx: &ToolContext) -> bool {
    let cmd = input["cmd"].as_str().unwrap_or("");
    for part in split_commands(cmd) {
        // 词法失败按越界处理：宁可多问一次
        let Ok(argv) = shell_words(&part) else {
            return true;
        };
        for tok in &argv {
            // `--flag=/path`、`ENV=/path`：`=` 右侧同样是操作数候选
            let mut candidates = vec![tok.as_str()];
            if let Some((_, v)) = tok.split_once('=') {
                candidates.push(v);
            }
            for cand in candidates {
                if looks_like_operand_path(cand) && operand_escapes(cand, ctx) {
                    return true;
                }
            }
        }
    }
    false
}

/// token 是否 path 形态：含分隔符、家目录、相对游走，或绝对路径。
/// `-` 开头的是 flag 不是操作数。
fn looks_like_operand_path(tok: &str) -> bool {
    if tok.is_empty() || tok.starts_with('-') {
        return false;
    }
    tok.starts_with('~')
        || tok == "."
        || tok == ".."
        || tok.contains('/')
        || tok.contains('\\')
        || std::path::Path::new(tok).is_absolute()
}

fn operand_escapes(tok: &str, ctx: &ToolContext) -> bool {
    // `~` 词法上是相对路径但实际指家目录——一定在 repo 外
    if tok.starts_with('~') {
        return true;
    }
    crate::tools::repo_path(&ctx.repo_root, tok).is_err()
}

/// 记忆层之前的守卫序列（严格顺序，首个非 Pass 即终局）：
/// L0 授权闸门 → L1 内置 deny → L2 安全网必问 → L3 项目级 deny。
/// 全部只能输出 GuardVerdict——没有任何一个能替下层「放行」。
fn pre_memory_guards(
    db: &Db,
    ctx: &ToolContext,
    tool: &dyn Tool,
    tool_name: &str,
    input: &Value,
) -> Result<GuardVerdict, crate::tools::ToolError> {
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
            return Ok(GuardVerdict::Deny {
                reason: format!("no grant for mcp service: {service}"),
                layer: "grant",
            });
        }
    }
    // L1 内置 deny
    if let Some(reason) = tool.builtin_deny(input, ctx) {
        return Ok(GuardVerdict::Deny {
            reason,
            layer: "builtin_deny",
        });
    }
    // L2 安全网必问（永不进记忆）
    if let Some(label) = is_safety_net(tool_name, input) {
        return Ok(GuardVerdict::ReferHuman {
            reason: format!("safety net: {label}"),
            safety_net: true,
        });
    }
    // L3 项目级 deny（压过记忆 allow）
    if let Some(shape) = matching_rule(db, ctx, tool_name, input, "deny")? {
        return Ok(GuardVerdict::Deny {
            reason: format!("project deny rule: {shape}"),
            layer: "project_deny",
        });
    }
    Ok(GuardVerdict::Pass)
}

/// 自治 L3+ 把安全网和新的权限询问换成放行。
///
/// 出处：hands-free 票 03 / ADR 0059。选 L3 就是离开后过程不再被询问，
/// 危险操作的代价由选档承担。被否决：读 `execution_rank`（封顶 2）——
/// 这张票就永远放行不了；也被否决：把封顶抬到 4，盖章和提案的 `>= 3`
/// 会在票 02/04 之前生效。
///
/// 到这里时内置 deny 已经是 `Deny`（在 `pre_memory_guards`，先于本函数）。
/// 项目否定要再查一次：安全网在 L0–L2 先于 deny 返回，低档仍是必问而不是拒绝
/// （顺序不变）。高档位若直接把 Ask 换成 Allow，否定规则会被跳过。
/// 被否决：全局把 deny 挪到安全网之前——L0–L2 上「安全网 + 否定规则」会从
/// 必问变成拒绝。
///
/// 远程发布不搭这趟车：工具名 `remote_publish` 任何档都留下必问。
/// `git push` 是另一条命令（CONTEXT「远程发布」），随安全网放行。
/// 真正的发布入口是 `publish::request`，不进本函数，也不会在这里被放行。
/// 基线合入同样不搭：`git_baseline_merge` 和 bash 里的 `git merge` 是最终验收
///（票 02 / ADR 0059），L3/L4 也不自动合入。
///
/// 读档失败按 0。false negative 多一张卡；false positive 是未审副作用。
/// 放行不写 permission_rules：安全网永不记忆；新询问只是这一档的放行，不是记住。
fn release_at_high_autonomy(
    db: &Db,
    ctx: &ToolContext,
    tool_name: &str,
    input: &Value,
    decision: Decision,
) -> Result<Decision, crate::tools::ToolError> {
    let Decision::Ask { reason, safety_net } = decision else {
        return Ok(decision);
    };
    if tool_name == "remote_publish"
        || tool_name == "git_baseline_merge"
        || is_safety_net(tool_name, input) == Some("baseline merge")
    {
        return Ok(Decision::Ask { reason, safety_net });
    }
    let level = crate::autonomy::rank(db, &ctx.project_id).unwrap_or(0);
    if level < 3 {
        return Ok(Decision::Ask { reason, safety_net });
    }
    if let Some(shape) = matching_rule(db, ctx, tool_name, input, "deny")? {
        return Ok(Decision::Deny {
            reason: format!("project deny rule: {shape}"),
            layer: "project_deny",
        });
    }
    Ok(Decision::Allow {
        via: AllowVia::Autonomy {
            level,
            safety_net,
            reason,
        },
    })
}

/// 五层求值。`tool` 用于第 1 层内置 deny。
/// 封印约定（票 05）：本函数与执行共用同一 `&input` 借用——裁决看到的
/// 字节就是执行的字节；必问卡路径更硬：resolve 从持久化的 pending_question
/// 载荷取 raw_input 执行,调用方根本没有再传参的入口。
/// 自治 L3+ 的放行在返回前做（`release_at_high_autonomy`），不改这五层的顺序。
pub fn evaluate(
    db: &Db,
    ctx: &ToolContext,
    tool: &dyn Tool,
    tool_name: &str,
    input: &Value,
) -> Result<Decision, crate::tools::ToolError> {
    let decision = evaluate_layers(db, ctx, tool, tool_name, input)?;
    release_at_high_autonomy(db, ctx, tool_name, input, decision)
}

fn evaluate_layers(
    db: &Db,
    ctx: &ToolContext,
    tool: &dyn Tool,
    tool_name: &str,
    input: &Value,
) -> Result<Decision, crate::tools::ToolError> {
    if let Some(d) = pre_memory_guards(db, ctx, tool, tool_name, input)?.into_decision() {
        return Ok(d);
    }
    // L4 形状化记忆 allow
    if let Some((shape, scope)) = matching_rule_scoped(db, ctx, tool_name, input, "allow")? {
        // External 地板（票 08）：mcp:* 语义由第三方服务器自定，名字只是
        // 标签不是证据——记忆 allow 对本类永不生效，每次调用都是一次外发
        // 判定。（persist_rule 侧同样拒写 mcp 规则，双保险。）
        if tool.risk() == crate::tools::RiskClass::External {
            return Ok(Decision::Ask {
                reason: "external tool: per-call approval".into(),
                safety_net: false,
            });
        }
        // 票 07（OPE-130 同类）：形状只担保匹配到的词——`cat *` 的授权意图是
        // 「项目文件随便读」，不该覆盖 `cat ~/.aws/credentials`。凡 path 形态
        // 操作数越出 repo_root 的转必问。不对称代价：多报一个操作数 = 多一次
        // 人工；漏报 = 一次越权读——提取宁可偏多（fail closed）。
        if tool_name == "bash" && bash_operand_escapes(input, ctx) {
            return Ok(Decision::Ask {
                reason: "bash operand outside repo root".into(),
                safety_net: false,
            });
        }
        return Ok(Decision::Allow {
            via: AllowVia::Remembered { shape, scope },
        });
    }
    // 记忆层之后的守卫：路径归属
    if violates_ownership(tool_name, input, ctx) {
        return Ok(Decision::Ask {
            reason: "path outside ownership".into(),
            safety_net: false,
        });
    }
    // L5 类级默认（票 08 数据驱动）：读/本地写默认放行，
    // 执行/外发/外部服务默认必问。
    match tool.risk() {
        crate::tools::RiskClass::Read | crate::tools::RiskClass::WriteLocal => {
            Ok(Decision::Allow {
                via: AllowVia::Default,
            })
        }
        crate::tools::RiskClass::Egress
        | crate::tools::RiskClass::Exec
        | crate::tools::RiskClass::External => Ok(Decision::Ask {
            reason: "default ask".into(),
            safety_net: false,
        }),
    }
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
    // External 地板另一半（票 08）：mcp:* 规则根本写不进记忆层——
    // 名字是服务器自己的话，持久豁免等于把判定外包给标签。
    if tool.starts_with("mcp:") {
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

/// 已记权限规则行（设置-权限分区审计面，ui-audit-2 票 03）。
/// effect 一并下发：allow 与项目级 deny 规则同表陈列。
#[derive(Debug, Clone, serde::Serialize, ts_rs::TS)]
#[ts(export, export_to = "../../../ui/src/gen/")]
pub struct PermissionRuleRow {
    pub id: String,
    pub tool: String,
    pub shape: String,
    pub domain: Option<String>,
    pub effect: String,
    pub scope: String,
    pub created_at: String,
}

/// 审计面读路径：只读 permission_rules，属主仍是本模块。
pub fn list_rules(db: &Db, project_id: &str) -> Result<Vec<PermissionRuleRow>, rusqlite::Error> {
    db.conn()
        .prepare(
            "SELECT id, tool, shape, domain, effect, scope, created_at
             FROM permission_rules WHERE project_id=?1
             ORDER BY created_at DESC, id DESC",
        )?
        .query_map([project_id], |r| {
            Ok(PermissionRuleRow {
                id: r.get(0)?,
                tool: r.get(1)?,
                shape: r.get(2)?,
                domain: r.get(3)?,
                effect: r.get(4)?,
                scope: r.get(5)?,
                created_at: r.get(6)?,
            })
        })?
        .collect()
}

/// 撤销已记规则（ui-audit-2 票 03）：删行 + 落 permission_rule_revoked 事件。
/// 「记住」可撤销是 persist_rule 的存在前提；撤销本身也进审计轨——
/// 否则审计面自己就是盲区。返回 false = 规则不存在或不属本项目
/// （不冒充删了；调用方据此报错而非静默成功）。
pub fn revoke_rule(
    db: &Db,
    project_id: &str,
    rule_id: &str,
) -> Result<bool, crate::trace::TraceError> {
    use rusqlite::OptionalExtension;
    let row: Option<(String, String, String)> = db
        .conn()
        .query_row(
            "SELECT tool, shape, scope FROM permission_rules WHERE id=?1 AND project_id=?2",
            rusqlite::params![rule_id, project_id],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
        )
        .optional()?;
    let Some((tool, shape, scope)) = row else {
        return Ok(false);
    };
    db.conn().execute(
        "DELETE FROM permission_rules WHERE id=?1 AND project_id=?2",
        rusqlite::params![rule_id, project_id],
    )?;
    db.append_event(
        project_id,
        crate::trace::EventKind::PermissionRuleRevoked,
        serde_json::json!({
            "rule_id": rule_id,
            "tool": tool,
            "shape": shape,
            "scope": scope,
        }),
        None,
        None,
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
                // 票 03：省略 autonomy 的新行默认 L4，安全网和新询问会放行。
                // 本夹具测的是 L0 排队 / 记忆地板，显式钉 L0。高档见 prop 与 api 测试。
                "INSERT INTO projects (id, dir, name, mode, autonomy) VALUES ('p1','/tmp/x','x','pack','L0')",
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
                sessions: Default::default(),
                caps: Default::default(),
            },
            dir,
        )
    }

    fn bash_ctx(db: &Db, ctx: &ToolContext, cmd: &str) -> Decision {
        evaluate(db, ctx, &Bash, "bash", &json!({"cmd": cmd})).unwrap()
    }

    /// ui-audit-2 票 03：审计面读+撤销。撤销留 permission_rule_revoked 事件；
    /// 撤销后同形状请求回落必问（记忆层不再命中）。
    #[test]
    fn revoke_rule_roundtrip() {
        let (db, ctx, _d) = setup();
        db.conn()
            .execute(
                "INSERT INTO permission_rules (id,project_id,tool,shape,effect,scope)
                 VALUES ('pr1','p1','bash','npm *','allow','project')",
                [],
            )
            .unwrap();
        let rules = list_rules(&db, "p1").unwrap();
        assert_eq!(rules.len(), 1);
        assert_eq!(rules[0].shape, "npm *");
        assert_eq!(rules[0].effect, "allow");

        // 撤销前命中记忆 allow；撤销后回必问
        assert!(matches!(
            bash_ctx(&db, &ctx, "npm test"),
            Decision::Allow { .. }
        ));
        assert!(revoke_rule(&db, "p1", "pr1").unwrap());
        assert!(matches!(
            bash_ctx(&db, &ctx, "npm test"),
            Decision::Ask { .. }
        ));
        // 事件留痕
        let kind: String = db
            .conn()
            .query_row(
                "SELECT kind FROM events WHERE kind='permission_rule_revoked'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(kind, "permission_rule_revoked");
        // 不存在的规则：false，不冒充删除
        assert!(!revoke_rule(&db, "p1", "prX").unwrap());
        // 别人的项目：不可越界删
        assert!(!revoke_rule(&db, "p2", "pr1").unwrap());
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

    // ---- 票 08：RiskClass ----

    /// 无法注册的 McpTool 替身：evaluate 只看 risk()，用 stub 声明 External。
    struct ExternalStub;
    impl Tool for ExternalStub {
        fn name(&self) -> &str {
            "mcp:svc:t"
        }
        fn description(&self) -> &str {
            "stub"
        }
        fn input_schema(&self) -> Value {
            json!({})
        }
        fn risk(&self) -> crate::tools::RiskClass {
            crate::tools::RiskClass::External
        }
        fn exec(
            &self,
            _db: &Db,
            _i: &Value,
            _c: &ToolContext,
        ) -> Result<Value, crate::tools::ToolError> {
            Ok(json!({}))
        }
    }

    #[test]
    fn builtin_tools_declare_expected_risk() {
        use crate::tools::RiskClass::*;
        assert_eq!(crate::tools::FsRead.risk(), Read);
        assert_eq!(crate::tools::FsWrite.risk(), WriteLocal);
        assert_eq!(crate::tools::FsPatch.risk(), WriteLocal);
        assert_eq!(crate::tools::ArtifactWrite.risk(), WriteLocal);
        assert_eq!(crate::tools::ArtifactRead.risk(), Read);
        assert_eq!(crate::tools::Research.risk(), Read);
        assert_eq!(crate::tools::Bash.risk(), Exec);
    }

    #[test]
    fn external_floor_beats_remembered_allow() {
        let (db, ctx, _d) = setup();
        // 先喂 grants 让 L0 闸门放行（External 地板测的是 L4，不是 L0）
        db.conn()
            .execute(
                "INSERT INTO grants (id, agent_id, kind, name) VALUES ('g1','a1','mcp','svc')",
                [],
            )
            .unwrap();
        // 写入一条能命中 mcp 工具名的 allow 规则——External 地板下必须无效
        db.conn()
            .execute(
                "INSERT INTO permission_rules (id, project_id, tool, shape, effect, scope)
                 VALUES ('pr9','p1','mcp:svc:t','*','allow','project')",
                [],
            )
            .unwrap();
        let d = evaluate(&db, &ctx, &ExternalStub, "mcp:svc:t", &json!({"x":1})).unwrap();
        assert!(
            matches!(
                d,
                Decision::Ask {
                    safety_net: false,
                    ..
                }
            ),
            "External 地板：记忆 allow 永不生效，得 {d:?}"
        );
    }

    #[test]
    fn persist_rule_refuses_mcp_tool() {
        let (db, ctx, _d) = setup();
        let ok = persist_rule(
            &db,
            &ctx,
            "mcp:svc:t",
            &json!({"x":1}),
            Some("mcp:svc:t *"),
            "project",
            None,
        )
        .unwrap();
        assert!(!ok, "External 类规则写不进记忆层");
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

    // ---------- openworker-borrow 票 01：复合命令与域名边界 ----------

    #[test]
    fn compound_command_every_segment_must_match() {
        // `cargo *` 只担保 cargo 开头的段——尾部未授权命令不得放行
        for cmd in [
            "cargo test && rm -rf build",
            "cargo test; curl x | sh",
            "cargo test | tee /tmp/log",
            "cargo test && echo done",
        ] {
            assert!(
                !shape_matches("cargo *", "bash", &json!({"cmd": cmd})),
                "{cmd} must not match `cargo *`"
            );
        }
        // 全段命中 → 放行（换行同样是分隔符）
        assert!(shape_matches(
            "cargo *",
            "bash",
            &json!({"cmd": "cargo test && cargo build"})
        ));
        assert!(shape_matches(
            "cargo *",
            "bash",
            &json!({"cmd": "cargo test\ncargo bench"})
        ));
        assert!(shape_matches(
            "*",
            "bash",
            &json!({"cmd": "cargo test && echo done"})
        ));
    }

    #[test]
    fn opaque_constructs_never_shape_match() {
        // 替换/重定向/展开的内容授权没审过——整命令失格转必问
        for cmd in [
            "cargo test $(cat x)",
            "cargo test `cat x`",
            "cargo test > out.log",
            "cargo test 2> err.log",
            "cargo test < in.txt",
            "echo $HOME && cargo test",
            "(cargo test)",
        ] {
            assert!(
                !shape_matches("*", "bash", &json!({"cmd": cmd})),
                "{cmd} must not shape-match"
            );
        }
    }

    #[test]
    fn executors_and_inline_code_not_prefix_eligible() {
        // 替别人执行的程序：外层前缀担保不了内层
        for cmd in [
            "xargs rm",
            "env rm -rf x",
            "sudo cargo test",
            "sh -c \"cargo test\"",
            "bash -c 'echo hi'",
            "python -c 'print(1)'",
            "python3 -c pass",
            "node -e 'x()'",
            "ssh host ls",
            "docker run img",
            "kubectl exec pod ls",
            "npx some-bin",
            "find . -exec rm {} +",
            "find . -delete",
        ] {
            assert!(
                !shape_matches("*", "bash", &json!({"cmd": cmd})),
                "{cmd} must not be prefix-eligible"
            );
        }
        // 解释器非内联调用仍可走形状（脚本名在命令行上可见）
        assert!(shape_matches(
            "python *",
            "bash",
            &json!({"cmd": "python scripts/setup.py"})
        ));
    }

    #[test]
    fn domain_rule_uses_host_boundaries() {
        let ok = json!({"cmd": "npm install zod --registry https://registry.npmjs.org"});
        assert!(shape_matches(
            "npm install *@registry.npmjs.org",
            "bash",
            &ok
        ));
        // 伪造 host：后缀寄生/前缀寄生/不同域都不命中
        for host in [
            "registry.npmjs.org.evil.com",
            "evil-registry.npmjs.org",
            "registry.npmjs.orgx",
            "notnpmjs.org",
        ] {
            let bad = json!({"cmd": format!("npm install zod --registry https://{host}")});
            assert!(
                !shape_matches("npm install *@registry.npmjs.org", "bash", &bad),
                "{host} must not satisfy @registry.npmjs.org"
            );
        }
        // 域出现在另一段不算数：npm 段没引用该域
        let cross = json!({"cmd": "npm install zod && curl https://registry.npmjs.org/x"});
        assert!(!shape_matches(
            "npm install *@registry.npmjs.org",
            "bash",
            &cross
        ));
        // 裸域（无 scheme）token 也按 host 判
        let bare = json!({"cmd": "npm install zod --registry registry.npmjs.org"});
        assert!(shape_matches(
            "npm install *@registry.npmjs.org",
            "bash",
            &bare
        ));
    }

    #[test]
    fn unbalanced_quotes_fail_closed() {
        // 词法失败的段不猜——失格转必问
        assert!(!shape_matches(
            "git *",
            "bash",
            &json!({"cmd": "git log --format=\"%an|%s"})
        ));
        // 引号内的分隔符不是分隔符：整段一个词法单元
        assert!(shape_matches(
            "git *",
            "bash",
            &json!({"cmd": "git log --format=\"%an | %s\""})
        ));
    }

    #[test]
    fn l4_bash_operand_outside_repo_asks() {
        // 票 07（OPE-130 同类）：`cat *` 授权覆盖仓内读，越界操作数转必问
        let (db, ctx, _d) = setup();
        db.conn()
            .execute(
                "INSERT INTO permission_rules (id,project_id,tool,shape,effect,scope)
             VALUES ('a','p1','bash','cat *','allow','project'),
                    ('b','p1','bash','git *','allow','project')",
                [],
            )
            .unwrap();
        // 仓内 → 放行
        assert!(matches!(
            bash_ctx(&db, &ctx, "cat src/a.rs"),
            Decision::Allow { .. }
        ));
        assert!(matches!(
            bash_ctx(&db, &ctx, "cat a.md && cat b.md"),
            Decision::Allow { .. }
        ));
        // 越界 → 必问（绝对/游走/家目录/-C/内嵌 flag 值各一例）
        for cmd in [
            "cat /etc/hostname",
            "cat ../sibling/x",
            "cat ~/.zshrc",
            "cat a.md && cat /var/log/syslog",
            "git -C /tmp status",
            "git log --output=/tmp/x",
        ] {
            assert!(
                matches!(bash_ctx(&db, &ctx, cmd), Decision::Ask { .. }),
                "{cmd} must ask (operand escapes repo)"
            );
        }
        // deny 语义不变：deny `cat *` 对越界读仍是 Deny 不是 Ask
        db.conn()
            .execute(
                "INSERT INTO permission_rules (id,project_id,tool,shape,effect,scope)
             VALUES ('d','p1','bash','cat *','deny','project')",
                [],
            )
            .unwrap();
        assert!(matches!(
            bash_ctx(&db, &ctx, "cat /etc/hostname"),
            Decision::Deny {
                layer: "project_deny",
                ..
            }
        ));
    }

    #[test]
    fn l4_compound_bypass_now_asks() {
        // 端到端：记忆 `cargo *` 后复合命令仍必问（票 01 的洞）
        let (db, ctx, _d) = setup();
        db.conn()
            .execute(
                "INSERT INTO permission_rules (id,project_id,tool,shape,effect,scope)
             VALUES ('a','p1','bash','cargo *','allow','project')",
                [],
            )
            .unwrap();
        assert!(matches!(
            bash_ctx(&db, &ctx, "cargo test && echo hi"),
            Decision::Ask { .. }
        ));
        assert!(matches!(
            bash_ctx(&db, &ctx, "cargo test && cargo build"),
            Decision::Allow { .. }
        ));
    }

    // ---------- 票 28：权限不变量属性测试 ----------
    //
    // 规格 Testing Decisions：proptest 生成规则组合，断言三条不变量
    // 在任意 allow/deny/作用域组合下恒成立。

    /// B1 核验探针（arch-review 附录 B1）：`*` 回溯最坏 O(n^k) 无 memo，
    /// 病态形状（k 个星 + 尾段不匹配字面量强迫全回溯）必须在有界时间返回。
    /// 打印实测耗时供报告取证；阈值取宽限值只抓指数爆炸。
    #[test]
    fn star_match_pathological_bounded() {
        for k in [4usize, 6, 8] {
            let mut shape = vec!["*"; k];
            // 尾字面量 "z" 对全 x argv 永不命中——每个星的切分都要走完才败。
            shape.push("z");
            let argv: Vec<String> = (0..40).map(|_| "x".to_string()).collect();
            let t0 = std::time::Instant::now();
            let _ = seg_tokens_match(&shape, &argv);
            let el = t0.elapsed();
            eprintln!("seg_tokens_match k={k}+z n=40: {el:?}");
            assert!(
                el < std::time::Duration::from_secs(5),
                "k={k} 病态形状 {el:?} 超界——需迭代+memo 修复"
            );
        }
    }

    mod prop_tests {
        use super::tests::setup;
        use super::*;
        use crate::tools::{Bash, FsRead, FsWrite, WebFetch};
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

            /// 不变量④（openworker-borrow 票 01）：含分隔符的复合命令
            /// 永不被 `prefix *` 形状放行——`cargo *` 不得覆盖 `cargo test && rm`。
            #[test]
            fn compound_never_allowed_by_prefix_shape(
                sep in sample::select(vec!["&&", "||", ";", "|", "&", "\n"]),
                tail in sample::select(vec!["echo hi", "ls -la", "cat x.md"]),
            ) {
                let (db, ctx, _d) = setup();
                insert_rule(&db, 0, "bash", "cargo *", "allow", "project");
                let cmd = format!("cargo test {sep} {tail}");
                let d = evaluate(&db, &ctx, &Bash, "bash", &json!({"cmd": cmd})).unwrap();
                prop_assert!(
                    !matches!(d, Decision::Allow { .. }),
                    "compound {cmd} must not be shape-allowed, got {d:?}"
                );
            }

            /// 不变量⑤（票 01）：@域规则只认 host 边界——前缀/后缀寄生域不命中。
            #[test]
            fn domain_rules_require_host_boundary(
                host in sample::select(vec![
                    "registry.npmjs.org.evil.com",
                    "evil-registry.npmjs.org",
                    "registry.npmjs.orgx",
                ]),
            ) {
                let (db, ctx, _d) = setup();
                insert_rule(
                    &db, 0, "bash",
                    "npm install *@registry.npmjs.org", "allow", "project",
                );
                let cmd = format!("npm install zod --registry https://{host}");
                let d = evaluate(&db, &ctx, &Bash, "bash", &json!({"cmd": cmd})).unwrap();
                prop_assert!(
                    matches!(d, Decision::Ask { .. }),
                    "{host} must not satisfy the domain rule, got {d:?}"
                );
            }
        }

        /// 票 03 判定面。高档放行安全网和新询问；任何档拒绝内置永不和远程发布。
        /// 低档安全网/新询问仍是 Ask。夹具 setup() 钉 L0，这里再 set_level。
        #[derive(Clone, Debug)]
        enum Boundary {
            SafetyBash(&'static str),
            SafetyWrite(&'static str),
            NewAsk(&'static str),
            NewDomain(&'static str),
            BuiltinRead(&'static str),
            BuiltinWrite(&'static str),
            BuiltinBash(&'static str),
            RemotePublish,
        }

        fn boundary_action() -> impl Strategy<Value = Boundary> {
            sample::select(vec![
                Boundary::SafetyBash("rm -rf build"),
                Boundary::SafetyBash("git push origin main"),
                Boundary::SafetyBash("git update-ref HEAD"),
                Boundary::SafetyWrite(".git/config"),
                Boundary::NewAsk("echo hi"),
                Boundary::NewDomain("https://new.example/a"),
                Boundary::BuiltinRead(".env"),
                Boundary::BuiltinWrite(".hexagon/permissions.toml"),
                Boundary::BuiltinBash("cat .env"),
                Boundary::RemotePublish,
            ])
        }

        fn decide(db: &Db, ctx: &ToolContext, action: &Boundary) -> Decision {
            match action {
                Boundary::SafetyBash(cmd) | Boundary::NewAsk(cmd) | Boundary::BuiltinBash(cmd) => {
                    evaluate(db, ctx, &Bash, "bash", &json!({"cmd": cmd})).unwrap()
                }
                Boundary::SafetyWrite(path) | Boundary::BuiltinWrite(path) => evaluate(
                    db,
                    ctx,
                    &FsWrite,
                    "fs_write",
                    &json!({"path": path, "content": "x"}),
                )
                .unwrap(),
                Boundary::BuiltinRead(path) => {
                    evaluate(db, ctx, &FsRead, "fs_read", &json!({"path": path})).unwrap()
                }
                Boundary::NewDomain(url) => {
                    evaluate(db, ctx, &WebFetch, "web_fetch", &json!({"url": url})).unwrap()
                }
                // 工具名占位，不在注册表里。git push 不走这条。
                Boundary::RemotePublish => {
                    evaluate(db, ctx, &Bash, "remote_publish", &json!({})).unwrap()
                }
            }
        }

        proptest! {
            #[test]
            fn high_levels_pass_safety_net_and_reject_never_and_publish(
                level in sample::select(vec!["L0", "L1", "L2", "L3", "L4"]),
                action in boundary_action(),
            ) {
                let (db, ctx, _d) = setup();
                crate::autonomy::set_level(&db, "p1", level).unwrap();
                let high = matches!(level, "L3" | "L4");
                let d = decide(&db, &ctx, &action);
                match &action {
                    Boundary::BuiltinRead(_)
                    | Boundary::BuiltinWrite(_)
                    | Boundary::BuiltinBash(_) => {
                        prop_assert!(
                            matches!(d, Decision::Deny { layer: "builtin_deny", .. }),
                            "{level} {action:?} => {d:?}"
                        );
                    }
                    Boundary::RemotePublish => {
                        prop_assert!(
                            matches!(d, Decision::Ask { safety_net: true, .. }),
                            "{level} remote publish => {d:?}"
                        );
                    }
                    other => {
                        let expect_net = matches!(
                            other,
                            Boundary::SafetyBash(_) | Boundary::SafetyWrite(_)
                        );
                        if high {
                            match &d {
                                Decision::Allow {
                                    via: AllowVia::Autonomy {
                                        level: got,
                                        safety_net,
                                        ..
                                    },
                                } => {
                                    let want = crate::autonomy::parse_level(level).unwrap();
                                    prop_assert_eq!(*got, want, "{:?}", action);
                                    prop_assert_eq!(*safety_net, expect_net, "{:?}", action);
                                }
                                _ => prop_assert!(false, "{level} {action:?} => {d:?}"),
                            }
                        } else {
                            prop_assert!(
                                matches!(d, Decision::Ask { safety_net, .. } if safety_net == expect_net),
                                "{level} {action:?} => {d:?}"
                            );
                        }
                    }
                }
            }

            /// 项目否定在放行之前。安全网在 L0–L2 仍先必问（顺序不改）；
            /// L3+ 不能把否定规则一起放掉。普通询问任何档都是 deny。
            #[test]
            fn project_deny_survives_high_autonomy(
                level in sample::select(vec!["L0", "L1", "L2", "L3", "L4"]),
                net in proptest::bool::ANY,
            ) {
                let (db, ctx, _d) = setup();
                crate::autonomy::set_level(&db, "p1", level).unwrap();
                let (shape, cmd) = if net {
                    ("rm -rf *", "rm -rf build")
                } else {
                    ("npm *", "npm test")
                };
                insert_rule(&db, 0, "bash", shape, "deny", "project");
                let d = evaluate(&db, &ctx, &Bash, "bash", &json!({"cmd": cmd})).unwrap();
                let high = matches!(level, "L3" | "L4");
                if net && !high {
                    prop_assert!(
                        matches!(d, Decision::Ask { safety_net: true, .. }),
                        "{level} safety net still asks before deny, got {d:?}"
                    );
                } else {
                    prop_assert!(
                        matches!(d, Decision::Deny { layer: "project_deny", .. }),
                        "{level} net={net} => {d:?}"
                    );
                }
            }
        }
    }
}

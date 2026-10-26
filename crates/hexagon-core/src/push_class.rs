//! 推送 / 强推分类器（owner-followups-1015 #13，ADR 0093 后续裁决 2026-10-17）。
//!
//! Agent 终端里的 `git push` 分三档：不是推送 / 普通推送（走推送卡，可「允许本项目」，
//! 记住键 = 远程 + 分支）/ 强推（安全网：必问、不记、托管不代批）。
//!
//! **代价模型（fail closed）**：误判为强推只多问负责人一次；误判为普通推送是一次
//! 未经审阅的远端历史改写（还可能被「允许本项目」记住、被托管代批）。所以实现只在
//! 能完整读懂整条命令时才给「普通推送」——白名单解析，不是黑名单找 `--force`：
//! 未知选项、隐式目标、多个 refspec、远程写成 URL/路径、任何 shell 结构（复合、
//! 重定向、替换、通配）一律判强推。被否决的替代方案：在命令里搜 `--force` / `+`
//! ——`-uf`、`--force-with-lease=…`、`git -c alias…`、`origin :main`（删除远端分支）
//! 都能绕过黑名单。
//!
//! [`classify`] 是纯函数，无 IO：隐式目标（`git push` 不带参数时推到哪取决于 push.default / 上游 /
//! pushRemote）一律 `Force("implicit_target")`。ticket 33 P1（#13 负责人 2026-10-25 裁决选项 A）起，
//! 判定入口用 [`classify_in`] 带上仓库：隐式目标能确定解出时（`crate::push_resolve`）改判普通推送，
//! 解不出照旧安全网。原「读配置的判定与真正执行之间可被改写」的顾虑见 `push_resolve` 残余风险一节。
//! 每次判定只分类一次、结果往下传（ticket 33 S8：原先一次判定里安全网 / 记忆匹配 / 推送卡 / 诊断各自重分类）。

use crate::permissions::shell_words;

/// 普通推送的目标——「允许本项目」的记住键。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PushTarget {
    pub remote: String,
    pub branch: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PushClass {
    /// 命令里没有 git push。
    NotPush,
    /// 能完整读懂的普通推送：`git push [-u|-q|-v…] <命名远程> <分支|源:分支>`。
    Ordinary(PushTarget),
    /// 强推或读不懂的推送（原因码进诊断记录与卡片理由）。
    Force(&'static str),
}

/// 普通推送允许的选项（不改写远端历史、不扩大推送范围）。
const SAFE_OPTIONS: &[&str] = &["-u", "--set-upstream", "-q", "--quiet", "-v", "--verbose"];

/// shell 结构字符：出现即无法保证执行的就是读到的这一条 git push。
const SHELL_CONSTRUCTS: &[char] = &[
    ';', '&', '|', '<', '>', '$', '`', '\\', '(', ')', '{', '}', '*', '?', '[', ']', '!', '~',
    '\n', '\r', '#', '=',
];

pub fn classify(cmd: &str) -> PushClass {
    if !push_like(cmd) {
        return PushClass::NotPush;
    }
    match strict(cmd) {
        Ok(target) => PushClass::Ordinary(target),
        Err(code) => PushClass::Force(code),
    }
}

/// 带仓库上下文的分类（ticket 33 P1）：只有 [`classify`] 判成 `Force("implicit_target")` 的命令才去仓库里解目标，
/// 其余结论一字不改（强推、读不懂的命令不会因为有仓库而放宽）。`repo = None` 等同 [`classify`]。
/// 代价模型同模块顶：解析任何一步不确定都留在 `implicit_target`（偏多问一次）。
pub fn classify_in(cmd: &str, repo: Option<&std::path::Path>) -> PushClass {
    let class = classify(cmd);
    let (PushClass::Force("implicit_target"), Some(repo)) = (&class, repo) else {
        return class;
    };
    // implicit_target 只在整条命令已过 shell 结构 / 引号 / 选项白名单之后才给出，这里重读的词与 strict 一致。
    let Ok(words) = shell_words(cmd) else {
        return class;
    };
    let positional: Vec<&str> = words
        .get(2..)
        .unwrap_or_default()
        .iter()
        .map(String::as_str)
        .filter(|w| !w.starts_with('-'))
        .collect();
    match crate::push_resolve::read_facts(repo)
        .and_then(|facts| crate::push_resolve::resolve(&positional, &facts))
    {
        Some(target) => PushClass::Ordinary(target),
        None => class,
    }
}

/// 待决卡上的推送分类（托管、记住范围建议这些拿不到仓库的地方用）：从持久化的 raw_input 重新分类；
/// 唯一放宽的是 `implicit_target`——入卡时宿主已带仓库解过（[`card_facts`] 写进载荷的 `push`），
/// 载荷说解出了普通目标就用那个目标。载荷只由宿主写（`tools::mod` 入卡），模型碰不到；
/// 其余结论（强推、读不懂）仍只看执行的字节。旧卡（P1 之前入卡）载荷是 `force: true`，照旧安全网。
pub fn from_card(cmd: &str, facts: &serde_json::Value) -> PushClass {
    let class = classify(cmd);
    if class != PushClass::Force("implicit_target") || facts["force"] != false {
        return class;
    }
    match (facts["remote"].as_str(), facts["branch"].as_str()) {
        (Some(remote), Some(branch)) if named_remote(remote) && plain_branch(branch) => {
            PushClass::Ordinary(PushTarget {
                remote: remote.into(),
                branch: branch.into(),
            })
        }
        _ => class,
    }
}

/// 权限卡载荷里的推送事实（UI 据此画推送卡形）：普通推送给远程 + 分支，强推给原因码，
/// 不是推送为 null。判定从 raw_input 重新分类；只有隐式目标的解析结果经 [`from_card`] 被托管 / 记住建议采信
/// （ticket 33 P1：那两处拿不到仓库）。`repo` = 发起调用的仓库（`ToolContext::repo_root`）。
pub fn card_facts(
    tool: &str,
    input: &serde_json::Value,
    repo: Option<&std::path::Path>,
) -> serde_json::Value {
    if tool != "bash" {
        return serde_json::Value::Null;
    }
    match classify_in(input["cmd"].as_str().unwrap_or(""), repo) {
        PushClass::NotPush => serde_json::Value::Null,
        PushClass::Ordinary(t) => {
            serde_json::json!({"force": false, "remote": t.remote, "branch": t.branch})
        }
        PushClass::Force(code) => serde_json::json!({"force": true, "code": code}),
    }
}

/// 宽松识别「这条命令可能在推送」：宁可多认（多认只会走到严格解析再判强推）。
/// 覆盖面不小于改动前的安全网条件（任一词 basename 为 git 且任一词为 push）。
/// （ticket 33 / 2026-10-25 批次审查 S9：这两行曾错挂在 `card_facts` 上。）
fn push_like(cmd: &str) -> bool {
    let lower = cmd.to_lowercase();
    if lower.contains("git push") || lower.contains("git-push") {
        return true;
    }
    let words: Vec<String> = match shell_words(&lower) {
        Ok(w) => w,
        // 读不懂的命令只要字面带 push 就当推送（之后严格解析判强推）。
        Err(()) => return lower.contains("push"),
    };
    let base = |w: &str| w.rsplit('/').next().unwrap_or(w).to_string();
    let has_git = words
        .iter()
        .any(|w| matches!(base(w).as_str(), "git" | "git-push"));
    has_git && words.iter().any(|w| w == "push" || base(w) == "git-push")
}

fn strict(cmd: &str) -> Result<PushTarget, &'static str> {
    let words = plain_git_push(cmd)?;
    let (remote, refspec) = safe_positional(&words[2..])?;
    if !valid_remote(remote) {
        return Err("remote_not_named");
    }
    Ok(PushTarget {
        remote: remote.to_string(),
        branch: refspec_branch(refspec)?.to_string(),
    })
}

/// 读回字面上的 `git push …`：没有 shell 结构、能解析、读回的词与原文逐字一致、前两词正是 `git push`。
fn plain_git_push(cmd: &str) -> Result<Vec<String>, &'static str> {
    if cmd.contains(SHELL_CONSTRUCTS) {
        return Err("shell_construct");
    }
    let words = shell_words(cmd).map_err(|()| "unparsable")?;
    // 引号能把空白、分隔符藏进一个词；读回的词和原文逐字不同就不担保。
    if words.join(" ") != cmd.split_whitespace().collect::<Vec<_>>().join(" ") {
        return Err("quoted_words");
    }
    // `git -C dir push` / `git -c k=v push` 换了仓库或配置（alias、pushurl），不担保。
    match (
        words.first().map(String::as_str),
        words.get(1).map(String::as_str),
    ) {
        (Some("git"), Some("push")) => Ok(words),
        (Some("git"), _) => Err("git_global_option"),
        _ => Err("not_plain_git"),
    }
}

/// `git push` 之后的词：选项只许 [`SAFE_OPTIONS`]，非选项词必须恰好两个（远端 + refspec）。
fn safe_positional(args: &[String]) -> Result<(&str, &str), &'static str> {
    let mut positional = Vec::new();
    for w in args {
        if w.starts_with('-') {
            if !SAFE_OPTIONS.contains(&w.as_str()) {
                return Err(option_code(w));
            }
        } else {
            positional.push(w.as_str());
        }
    }
    match positional[..] {
        [remote, refspec] => Ok((remote, refspec)),
        _ if positional.len() < 2 => Err("implicit_target"),
        _ => Err("multiple_refspecs"),
    }
}

/// refspec 推到的目标分支：只认普通分支（可带 `refs/heads/`），源是 HEAD 或普通分支。
fn refspec_branch(refspec: &str) -> Result<&str, &'static str> {
    if refspec.starts_with('+') {
        return Err("plus_refspec");
    }
    let (src, dst) = match refspec.split_once(':') {
        // `origin :main` 删除远端分支。
        Some(("", _)) => return Err("delete_refspec"),
        Some((src, dst)) => (src, dst),
        None => (refspec, refspec),
    };
    let dst = dst.strip_prefix("refs/heads/").unwrap_or(dst);
    let src = src.strip_prefix("refs/heads/").unwrap_or(src);
    if dst.starts_with("refs/") || src.starts_with("refs/") {
        return Err("non_branch_ref");
    }
    // 只写 HEAD 时目标分支由当前检出决定，不是命令里读得到的事实。
    if dst == "HEAD" {
        return Err("implicit_target");
    }
    if !(valid_branch(dst) && (src == "HEAD" || valid_branch(src))) {
        return Err("branch_not_plain");
    }
    Ok(dst)
}

fn option_code(opt: &str) -> &'static str {
    let long = opt.starts_with("--");
    if opt.contains("force") || (!long && opt.contains('f')) {
        "force_flag"
    } else if matches!(opt, "--delete" | "-d") || (!long && opt.contains('d')) {
        "delete_flag"
    } else if matches!(
        opt,
        "--mirror" | "--all" | "--tags" | "--prune" | "--branches"
    ) {
        "multi_ref_flag"
    } else {
        "unknown_option"
    }
}

/// 命名远程（不是 URL / 路径）。
pub(crate) fn named_remote(s: &str) -> bool {
    valid_remote(s)
}

/// 普通分支名，且不是字面 `HEAD`（`HEAD` 作目标会推到远端 `refs/heads/HEAD`，见 `push_resolve`）。
pub(crate) fn plain_branch(s: &str) -> bool {
    valid_branch(s) && s != "HEAD"
}

fn valid_remote(s: &str) -> bool {
    !s.is_empty()
        && !s.starts_with(['-', '.'])
        && s.chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '_' | '-'))
}

fn valid_branch(s: &str) -> bool {
    !s.is_empty()
        && !s.starts_with(['-', '/', '.'])
        && !s.ends_with(['/', '.'])
        && !s.ends_with(".lock")
        && !s.contains("..")
        && !s.contains("//")
        && !s.contains("@{")
        && s.chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '_' | '-' | '/'))
}

#[cfg(test)]
mod tests {
    use super::*;
    use proptest::prelude::*;

    fn ord(remote: &str, branch: &str) -> PushClass {
        PushClass::Ordinary(PushTarget {
            remote: remote.into(),
            branch: branch.into(),
        })
    }

    #[test]
    fn ordinary_pushes_name_remote_and_branch() {
        assert_eq!(classify("git push origin main"), ord("origin", "main"));
        assert_eq!(
            classify("git push -u origin feature/x"),
            ord("origin", "feature/x")
        );
        assert_eq!(classify("git push origin HEAD:main"), ord("origin", "main"));
        assert_eq!(
            classify("git push --quiet up refs/heads/dev"),
            ord("up", "dev")
        );
        assert_eq!(classify("ls -la"), PushClass::NotPush);
        assert_eq!(classify("git status"), PushClass::NotPush);
    }

    #[test]
    fn force_and_doubtful_pushes_are_force() {
        for (cmd, code) in [
            ("git push --force origin main", "force_flag"),
            ("git push -f origin main", "force_flag"),
            ("git push -uf origin main", "force_flag"),
            ("git push --force-with-lease origin main", "force_flag"),
            (
                "git push --force-with-lease=main:abc origin main",
                "shell_construct",
            ),
            ("git push --force-if-includes origin main", "force_flag"),
            ("git push origin +main", "plus_refspec"),
            ("git push origin :main", "delete_refspec"),
            ("git push --delete origin main", "delete_flag"),
            ("git push --mirror origin", "multi_ref_flag"),
            ("git push", "implicit_target"),
            ("git push origin", "implicit_target"),
            ("git push origin HEAD", "implicit_target"),
            ("git push origin a b", "multiple_refspecs"),
            (
                "git push https://example.com/r.git main",
                "remote_not_named",
            ),
            ("git push ../elsewhere main", "remote_not_named"),
            ("git push origin refs/tags/v1", "non_branch_ref"),
            ("git -C /tmp push origin main", "git_global_option"),
            ("git -c alias.push=x push origin main", "shell_construct"),
            ("/usr/bin/git push origin main", "not_plain_git"),
            ("git push origin main && rm -rf .", "shell_construct"),
            ("git push origin main; echo", "shell_construct"),
            ("git push origin $(echo main)", "shell_construct"),
            ("git push 'origin' main", "quoted_words"),
            ("git push \"origin main\"", "quoted_words"),
            ("git push origin 'main", "unparsable"),
            ("GIT_DIR=x git push origin main", "shell_construct"),
        ] {
            assert_eq!(classify(cmd), PushClass::Force(code), "{cmd}");
        }
    }

    fn name() -> impl Strategy<Value = String> {
        "[a-z][a-z0-9_-]{0,8}".prop_filter("not an option", |s| !s.starts_with('-'))
    }

    fn branch() -> impl Strategy<Value = String> {
        (name(), proptest::option::of(name()))
            .prop_map(|(a, b)| b.map_or(a.clone(), |b| format!("{a}/{b}")))
            .prop_filter("valid", |s| valid_branch(s) && s != "HEAD")
    }

    fn safe_opts() -> impl Strategy<Value = Vec<&'static str>> {
        proptest::collection::vec(proptest::sample::select(SAFE_OPTIONS.to_vec()), 0..3)
    }

    /// Markers that rewrite remote history or widen the push.
    fn force_marker() -> impl Strategy<Value = String> {
        prop_oneof![
            Just("--force".to_string()),
            Just("-f".to_string()),
            Just("-uf".to_string()),
            Just("-fu".to_string()),
            Just("--force-with-lease".to_string()),
            Just("--force-if-includes".to_string()),
            Just("--mirror".to_string()),
            Just("--delete".to_string()),
            Just("-d".to_string()),
            Just("--all".to_string()),
            Just("--prune".to_string()),
            Just("--no-verify".to_string()),
            name().prop_map(|b| format!("+{b}")),
            name().prop_map(|b| format!(":{b}")),
            name().prop_map(|b| format!("+HEAD:{b}")),
        ]
    }

    /// Tokens an adversarial command is assembled from.
    fn token() -> impl Strategy<Value = String> {
        prop_oneof![
            Just("git".to_string()),
            Just("push".to_string()),
            Just("origin".to_string()),
            Just("main".to_string()),
            Just("HEAD:main".to_string()),
            Just("-u".to_string()),
            Just("&&".to_string()),
            Just(";".to_string()),
            Just("'".to_string()),
            Just("\"".to_string()),
            Just("-C".to_string()),
            Just("x=y".to_string()),
            force_marker(),
            "[ -~]{1,6}",
        ]
    }

    proptest! {
        /// Completeness: every well-formed ordinary push is ordinary, with
        /// exactly the remote and destination branch the command names.
        #[test]
        fn well_formed_push_is_ordinary(
            remote in name(), src in branch(), dst in proptest::option::of(branch()),
            before in safe_opts(), after in safe_opts(), head in any::<bool>(),
        ) {
            let refspec = match (&dst, head) {
                (Some(d), true) => format!("HEAD:{d}"),
                (Some(d), false) => format!("{src}:{d}"),
                (None, _) => src.clone(),
            };
            let mut words = vec!["git", "push"];
            words.extend(before);
            words.push(&remote);
            words.push(&refspec);
            words.extend(after);
            let cmd = words.join(" ");
            let want = dst.unwrap_or(src);
            prop_assert_eq!(classify(&cmd), ord(&remote, &want), "{}", cmd);
        }

        /// Fail-closed: any history-rewriting or widening marker anywhere in an
        /// otherwise ordinary push makes it a force push, never ordinary.
        #[test]
        fn force_marker_is_never_ordinary(
            remote in name(), b in branch(), marker in force_marker(), at in 0..4usize,
        ) {
            let mut words = vec!["git".to_string(), "push".to_string(), remote, b];
            words.insert(2 + at.min(2), marker);
            let cmd = words.join(" ");
            prop_assert!(matches!(classify(&cmd), PushClass::Force(_)), "{}", cmd);
        }

        /// Soundness over arbitrary token soup: "ordinary" is only ever said of a
        /// plain `git push` whose every word is a safe option, the named remote
        /// or the single refspec — no force marker, no shell structure.
        #[test]
        fn ordinary_only_for_fully_read_commands(tokens in proptest::collection::vec(token(), 0..8)) {
            let cmd = tokens.join(" ");
            if let PushClass::Ordinary(t) = classify(&cmd) {
                let words: Vec<&str> = cmd.split_whitespace().collect();
                prop_assert_eq!(&words[..2], &["git", "push"]);
                prop_assert!(!cmd.contains(SHELL_CONSTRUCTS));
                let positional: Vec<&&str> = words[2..].iter().filter(|w| !w.starts_with('-')).collect();
                prop_assert_eq!(positional.len(), 2);
                prop_assert_eq!(*positional[0], t.remote.as_str());
                prop_assert!(!positional[1].starts_with('+') && !positional[1].starts_with(':'));
                prop_assert!(words[2..].iter().filter(|w| w.starts_with('-')).all(|w| SAFE_OPTIONS.contains(w)));
            }
        }

        /// Appending any compound tail to an ordinary push voids the guarantee.
        #[test]
        fn compound_tail_is_force(remote in name(), b in branch(),
            sep in proptest::sample::select(vec![";", "&&", "||", "|", "&", ">", "\n"]), tail in "[a-z ]{0,8}") {
            let cmd = format!("git push {remote} {b} {sep} {tail}");
            prop_assert!(matches!(classify(&cmd), PushClass::Force(_)), "{}", cmd);
        }

        /// The permission layer agrees: ordinary pushes are not a safety net,
        /// force pushes always are (and so never become remembered grants).
        #[test]
        fn safety_net_tracks_the_classifier(tokens in proptest::collection::vec(token(), 0..8)) {
            let cmd = tokens.join(" ");
            let net = crate::permissions::is_safety_net("bash", &serde_json::json!({"cmd": cmd}));
            match classify(&cmd) {
                PushClass::Force(_) => prop_assert!(net.is_some(), "{}", cmd),
                PushClass::Ordinary(_) => prop_assert!(net.is_none(), "{}", cmd),
                PushClass::NotPush => {}
            }
        }
    }
}

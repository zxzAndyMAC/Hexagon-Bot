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
//! 纯函数，无 IO：不读 git 配置推断隐式目标（`git push` 不带参数时推到哪取决于
//! push.default / 上游 / pushRemote，读配置的判定与真正执行之间可被改写）。

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

/// 宽松识别「这条命令可能在推送」：宁可多认（多认只会走到严格解析再判强推）。
/// 覆盖面不小于改动前的安全网条件（任一词 basename 为 git 且任一词为 push）。
/// 权限卡载荷里的推送事实（UI 据此画推送卡形）：普通推送给远程 + 分支，强推给原因码，
/// 不是推送为 null。只是展示；判定与托管都从 raw_input 重新分类，不信这个字段。
pub fn card_facts(tool: &str, input: &serde_json::Value) -> serde_json::Value {
    if tool != "bash" {
        return serde_json::Value::Null;
    }
    match classify(input["cmd"].as_str().unwrap_or("")) {
        PushClass::NotPush => serde_json::Value::Null,
        PushClass::Ordinary(t) => {
            serde_json::json!({"force": false, "remote": t.remote, "branch": t.branch})
        }
        PushClass::Force(code) => serde_json::json!({"force": true, "code": code}),
    }
}

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
        (Some("git"), Some("push")) => {}
        (Some("git"), _) => return Err("git_global_option"),
        _ => return Err("not_plain_git"),
    }
    let mut positional = Vec::new();
    for w in &words[2..] {
        if w.starts_with('-') {
            if !SAFE_OPTIONS.contains(&w.as_str()) {
                return Err(option_code(w));
            }
        } else {
            positional.push(w.as_str());
        }
    }
    let [remote, refspec] = positional[..] else {
        return Err(if positional.len() < 2 {
            "implicit_target"
        } else {
            "multiple_refspecs"
        });
    };
    if !valid_remote(remote) {
        return Err("remote_not_named");
    }
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
    Ok(PushTarget {
        remote: remote.to_string(),
        branch: dst.to_string(),
    })
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

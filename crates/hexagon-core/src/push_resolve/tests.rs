//! ticket 33 P1：隐式目标推送解析。表测在临时 git 仓库里跑，判据 = 同一仓库上的
//! `git push --dry-run --porcelain`（git 自己说会推到哪）；属性测试钉「解不出的状态永不成普通推送」。
use super::*;
use crate::push_class::{classify_in, PushClass};
use proptest::prelude::*;
use std::path::{Path, PathBuf};

fn git(repo: &Path, args: &[&str]) {
    crate::git::run(repo, args).unwrap_or_else(|e| panic!("git {args:?}: {e}"));
}

/// 本地仓 `r`（分支 feat，一个提交）+ 两个裸远程 `origin`、`up`。不设上游、不设 push.default。
fn fixture() -> (tempfile::TempDir, PathBuf) {
    let tmp = tempfile::tempdir().unwrap();
    let repo = tmp.path().join("r");
    std::fs::create_dir_all(&repo).unwrap();
    for bare in ["b_origin", "b_up"] {
        let p = tmp.path().join(bare);
        std::fs::create_dir_all(&p).unwrap();
        git(&p, &["init", "-q", "--bare"]);
    }
    git(&repo, &["init", "-q", "-b", "feat"]);
    git(
        &repo,
        &[
            "-c",
            "user.name=t",
            "-c",
            "user.email=t@example.com",
            "commit",
            "-q",
            "--allow-empty",
            "-m",
            "x",
        ],
    );
    git(&repo, &["remote", "add", "origin", "../b_origin"]);
    git(&repo, &["remote", "add", "up", "../b_up"]);
    // 上游 ref 要存在才能 --set-upstream-to：先把 feat 推成两个远程上的 main 与 feat。
    for r in ["origin", "up"] {
        git(&repo, &["push", "-q", r, "feat:main", "feat:feat"]);
    }
    (tmp, repo)
}

fn set(repo: &Path, key: &str, value: &str) {
    git(repo, &["config", key, value]);
}

/// git 自己的答案：dry-run 会更新的 (远程, 分支)。只认恰好一条 refs/heads/* 更新；其余（报错、多条）= None。
fn oracle(repo: &Path, args: &[&str]) -> Option<(String, String)> {
    let mut full = vec!["push", "--dry-run", "--porcelain"];
    full.extend_from_slice(args);
    let out = crate::git::run(repo, &full).ok()?;
    let mut remote = None;
    let mut refs = Vec::new();
    for line in out.lines() {
        if let Some(to) = line.strip_prefix("To ") {
            remote = Some(to.trim().to_string());
        } else if let Some(dst) = line
            .split('\t')
            .nth(1)
            .and_then(|spec| spec.split_once(':'))
            .and_then(|(_, dst)| dst.strip_prefix("refs/heads/"))
        {
            refs.push(dst.to_string());
        }
    }
    let name = match remote?.as_str() {
        "../b_origin" => "origin",
        "../b_up" => "up",
        _ => return None,
    };
    match refs.as_slice() {
        [one] => Some((name.to_string(), one.clone())),
        _ => None,
    }
}

fn ord(remote: &str, branch: &str) -> PushClass {
    PushClass::Ordinary(PushTarget {
        remote: remote.into(),
        branch: branch.into(),
    })
}

const IMPLICIT: PushClass = PushClass::Force("implicit_target");

/// 每一行：(准备, 命令, 期望)。期望为普通推送时，git dry-run 必须恰好推到同一 (远程, 分支)。
#[test]
fn implicit_target_forms_resolve_like_git_or_stay_safety_net() {
    type Setup = fn(&Path);
    let rows: Vec<(&str, Setup, &str, PushClass)> = vec![
        // simple + 同名上游：推当前分支
        (
            "simple+same-name upstream",
            |r| {
                set(r, "push.default", "simple");
                git(r, &["branch", "-q", "--set-upstream-to=origin/feat"]);
            },
            "git push",
            ord("origin", "feat"),
        ),
        (
            "simple+same-name upstream, named",
            |r| {
                set(r, "push.default", "simple");
                git(r, &["branch", "-q", "--set-upstream-to=origin/feat"]);
            },
            "git push origin",
            ord("origin", "feat"),
        ),
        (
            "simple+same-name upstream, -u",
            |r| {
                set(r, "push.default", "simple");
                git(r, &["branch", "-q", "--set-upstream-to=origin/feat"]);
            },
            "git push -u origin",
            ord("origin", "feat"),
        ),
        // simple + 异名上游：git 报错 → 安全网
        (
            "simple+other-name upstream",
            |r| {
                set(r, "push.default", "simple");
                git(r, &["branch", "-q", "--set-upstream-to=origin/main"]);
            },
            "git push",
            IMPLICIT,
        ),
        // simple + 三角（推到非上游远程）：同 current
        (
            "simple triangular",
            |r| {
                set(r, "push.default", "simple");
                git(r, &["branch", "-q", "--set-upstream-to=origin/main"]);
            },
            "git push up",
            ord("up", "feat"),
        ),
        // simple + 无上游：安全网
        (
            "simple no upstream",
            |r| set(r, "push.default", "simple"),
            "git push",
            IMPLICIT,
        ),
        (
            "simple no upstream, named",
            |r| set(r, "push.default", "simple"),
            "git push origin",
            IMPLICIT,
        ),
        // upstream：推到上游分支
        (
            "upstream mode",
            |r| {
                set(r, "push.default", "upstream");
                git(r, &["branch", "-q", "--set-upstream-to=origin/main"]);
            },
            "git push",
            ord("origin", "main"),
        ),
        (
            "upstream mode other remote",
            |r| {
                set(r, "push.default", "upstream");
                git(r, &["branch", "-q", "--set-upstream-to=origin/main"]);
            },
            "git push up",
            IMPLICIT,
        ),
        // current：同名
        (
            "current mode",
            |r| set(r, "push.default", "current"),
            "git push origin",
            ord("origin", "feat"),
        ),
        // 歧义 push.default
        (
            "matching mode",
            |r| {
                set(r, "push.default", "matching");
                git(r, &["branch", "-q", "--set-upstream-to=origin/feat"]);
            },
            "git push",
            IMPLICIT,
        ),
        (
            "nothing mode",
            |r| {
                set(r, "push.default", "nothing");
                git(r, &["branch", "-q", "--set-upstream-to=origin/feat"]);
            },
            "git push",
            IMPLICIT,
        ),
        // HEAD refspec：当前分支推到同名，无需上游
        (
            "HEAD refspec",
            |r| set(r, "push.default", "simple"),
            "git push -u origin HEAD",
            ord("origin", "feat"),
        ),
        // HEAD:HEAD 推到远端字面 refs/heads/HEAD → 安全网
        (
            "HEAD:HEAD",
            |r| set(r, "push.default", "simple"),
            "git push origin HEAD:HEAD",
            IMPLICIT,
        ),
        // 三角配置（pushRemote / pushDefault）：不追，安全网
        (
            "pushRemote",
            |r| {
                set(r, "push.default", "simple");
                git(r, &["branch", "-q", "--set-upstream-to=origin/feat"]);
                set(r, "branch.feat.pushRemote", "up");
            },
            "git push",
            IMPLICIT,
        ),
        (
            "pushDefault",
            |r| {
                set(r, "push.default", "simple");
                git(r, &["branch", "-q", "--set-upstream-to=origin/feat"]);
                set(r, "remote.pushDefault", "up");
            },
            "git push",
            IMPLICIT,
        ),
        // 远程配了推送 refspec（可含通配）：安全网
        (
            "remote push refspec",
            |r| {
                set(r, "push.default", "current");
                set(r, "remote.origin.push", "refs/heads/*:refs/heads/*");
            },
            "git push origin",
            IMPLICIT,
        ),
        // 命令点名的远程没配置（git 当路径推）：安全网
        (
            "unknown remote",
            |r| set(r, "push.default", "current"),
            "git push nowhere",
            IMPLICIT,
        ),
        // 游离 HEAD
        (
            "detached",
            |r| {
                set(r, "push.default", "current");
                git(r, &["checkout", "-q", "--detach"]);
            },
            "git push origin HEAD",
            IMPLICIT,
        ),
        (
            "detached bare",
            |r| {
                set(r, "push.default", "current");
                git(r, &["checkout", "-q", "--detach"]);
            },
            "git push origin",
            IMPLICIT,
        ),
    ];
    for (name, setup, cmd, want) in rows {
        let (_t, repo) = fixture();
        setup(&repo);
        let got = classify_in(cmd, Some(&repo));
        assert_eq!(got, want, "{name}: {cmd}");
        if let PushClass::Ordinary(t) = &got {
            let args: Vec<&str> = cmd.split_whitespace().skip(2).collect();
            assert_eq!(
                oracle(&repo, &args),
                Some((t.remote.clone(), t.branch.clone())),
                "{name}: git disagrees for {cmd}"
            );
        }
    }
}

#[test]
fn no_repo_context_or_unreadable_repo_stays_safety_net() {
    // 无仓库上下文 = 原口径；不是 git 仓库（读配置失败）= 安全网；显式目标不受仓库影响
    let tmp = tempfile::tempdir().unwrap();
    assert_eq!(classify_in("git push", None), IMPLICIT);
    assert_eq!(classify_in("git push origin", Some(tmp.path())), IMPLICIT);
    assert_eq!(
        classify_in("git push origin main", Some(tmp.path())),
        ord("origin", "main")
    );
    // 强推不因仓库可解而放宽
    let (_t, repo) = fixture();
    set(&repo, "push.default", "current");
    assert_eq!(
        classify_in("git push -f origin", Some(&repo)),
        PushClass::Force("force_flag")
    );
}

fn facts_strategy() -> impl Strategy<Value = (RepoFacts, Vec<&'static str>)> {
    let branch = prop::option::of(prop::sample::select(vec!["feat", "main", "HEAD", "a/b"]));
    let keys = prop::collection::vec(
        (
            prop::sample::select(vec![
                "branch.feat.remote",
                "branch.feat.merge",
                "branch.feat.pushremote",
                "remote.pushdefault",
                "push.default",
                "remote.origin.url",
                "remote.up.url",
                "remote.origin.push",
            ]),
            prop::sample::select(vec![
                "origin",
                "up",
                "refs/heads/feat",
                "refs/heads/main",
                "refs/tags/v1",
                "simple",
                "current",
                "upstream",
                "matching",
                "nothing",
                "weird",
                "../x",
            ]),
        ),
        0..10,
    );
    let positional = prop::sample::select(vec![
        vec![],
        vec!["origin"],
        vec!["up"],
        vec!["nowhere"],
        vec!["origin", "HEAD"],
        vec!["origin", "HEAD:HEAD"],
        vec!["origin", "main:HEAD"],
        vec!["a", "b", "c"],
    ]);
    (branch, keys, positional).prop_map(|(branch, keys, positional)| {
        let mut config: HashMap<String, Vec<String>> = HashMap::new();
        for (k, v) in keys {
            config.entry(k.into()).or_default().push(v.into());
        }
        (
            RepoFacts {
                branch: branch.map(str::to_string),
                config,
            },
            positional,
        )
    })
}

proptest! {
    // ticket 33 P1 判定面属性测试（D12）：解不出的状态永不成普通推送——游离 HEAD、无上游且未点名远程、
    // 歧义 push.default、远程推送 refspec（可能含通配）、点名远程未配置、HEAD 以外的 refspec、三角配置。
    // 放行时目标远程必已配置、目标分支是当前分支或其上游分支（普通名，不是字面 HEAD）。
    #[test]
    fn unresolvable_states_never_become_ordinary((facts, positional) in facts_strategy()) {
        let got = resolve(&positional, &facts);
        let get = |k: &str| facts.config.get(k).and_then(|v| v.last()).map(String::as_str);
        let current = facts.branch.as_deref();
        let mode = get("push.default").unwrap_or("simple");
        let refspec = positional.get(1).copied();
        let remote = positional.first().copied().or_else(|| get("branch.feat.remote"));
        let never = current.is_none()
            || current == Some("HEAD")
            || positional.len() > 2
            || (positional.is_empty() && get("branch.feat.remote").is_none())
            || (positional.is_empty() && (get("branch.feat.pushremote").is_some() || get("remote.pushdefault").is_some()))
            || (refspec.is_none() && !matches!(mode, "simple" | "current" | "upstream" | "tracking"))
            || (refspec.is_none() && remote == Some("origin") && get("remote.origin.push").is_some())
            || refspec.is_some_and(|r| r != "HEAD")
            || remote.is_none_or(|r| get(&format!("remote.{r}.url")).is_none());
        if never {
            prop_assert_eq!(got.clone(), None, "{:?} {:?}", facts, positional);
        }
        if let Some(t) = got {
            let url_key = format!("remote.{}.url", t.remote);
            prop_assert!(get(&url_key).is_some(), "remote {} not configured", t.remote);
            let upstream = get(&format!("branch.{}.merge", current.unwrap()))
                .and_then(|m| m.strip_prefix("refs/heads/"));
            prop_assert!(Some(t.branch.as_str()) == current || Some(t.branch.as_str()) == upstream, "{:?}", t);
            prop_assert!(t.branch != "HEAD");
        }
    }
}

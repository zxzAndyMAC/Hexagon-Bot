//! 隐式目标推送的确定性解析（owner-followups-1015 ticket 33 P1；#13 负责人 2026-10-25 裁决选项 A）。
//!
//! `git push`、`git push origin`、`git push -u origin HEAD` 这类命令里读不到目标分支，`push_class::classify`
//! 判 `Force("implicit_target")`（安全网）。裁决 A：目标能从仓库里**确定地**解出来时，按普通推送处理——
//! 同一张推送卡、同一个 `push:<远程> <分支>` 记住键、托管同普通推送。解不出来的照旧是安全网。
//!
//! **代价模型**：false negative（能解却判安全网）= 负责人多点一次；false positive（解错目标判普通）=
//! 一次未审的、推到非预期 ref 的推送（还可能被「允许本项目」记住、被托管代批）。每一支都偏向 false negative：
//! 只有下面逐条写明「放行」的组合给出目标，其余（含任何读取失败）一律 `None` → 安全网。
//!
//! 解析规则（与 git 2.x `push.default` 文档、本机 `git push --dry-run --porcelain` 实测一致；测试拿 dry-run 当判据）：
//! - 当前分支：`git symbolic-ref -q HEAD` 必须是 `refs/heads/<名>`——游离 HEAD → 安全网（偏拒）。
//! - 远程：命令点名的远程必须在配置里有 `remote.<名>.url`（否则 git 把它当路径 / URL 推）；不点名时取
//!   `branch.<当前>.remote`（上游远程），且 `branch.<当前>.pushRemote` / `remote.pushDefault` 都没配
//!   （三角工作流另取远程，偏拒）。远程配了 `remote.<名>.push` 推送 refspec → 不按 push.default 走、可能含通配，偏拒。
//! - 不带 refspec 时按 `push.default`（缺省 = simple）：
//!   - `simple`：有上游、上游远程就是这个远程、上游分支名 = 当前分支名 → 推当前分支（放行）；
//!     上游存在但远程不同（三角）→ 同 current（放行，git 文档原文）；无上游 → 偏拒（git 在同远程时直接报错，
//!     `push.autoSetupRemote` 之类的旁路不追）；上游名与当前名不同 → git 报错，偏拒。
//!   - `current`：推当前分支到同名（放行）。
//!   - `upstream` / `tracking`：有上游且远程一致 → 推到上游分支（放行）；否则 git 报错，偏拒。
//!   - `matching` / `nothing` / 其他：推多个或什么都不推 → 偏拒（不确定的目标集）。
//! - refspec 只有 `HEAD`（`git push origin HEAD`）：推当前分支到远程同名分支（放行）。
//!   `HEAD:HEAD` 偏拒——实测它推到远端字面名 `refs/heads/HEAD`，不是当前分支（2026-10-25 本机 git 核对），
//!   正是代价模型里「推到非预期 ref」那一种。
//!
//! 读取只走 `crate::git::run`（参数逐个交给 git，不经 shell），只用不触发仓库配置命令的子命令
//! （`symbolic-ref`、`config --list`；不跑 `status`——fsmonitor 能执行配置里的命令）。
//! 残余风险（已知、接受）：解析与真正执行之间配置可被改写（同一 Agent 的命令串行过闸，跨 Agent 共用仓库时
//! 理论上可插入）；被否决的替代是把命令改写成显式形式再执行——改写模型给的字节违背「裁决看到的就是执行的」。

use std::collections::HashMap;
use std::path::Path;

use crate::push_class::PushTarget;

/// 解析所需的仓库事实。纯数据：真机从 git 读（[`read_facts`]），属性测试直接构造。
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct RepoFacts {
    /// 当前分支（`refs/heads/` 之后）；`None` = 游离 HEAD。
    pub branch: Option<String>,
    /// `git config --list` 的键值（键 = 小写的段名 / 变量名，子段原样；同键多值保留全部，取最后一个）。
    pub config: HashMap<String, Vec<String>>,
}

impl RepoFacts {
    fn get(&self, key: &str) -> Option<&str> {
        self.config
            .get(key)
            .and_then(|v| v.last())
            .map(String::as_str)
    }

    fn has(&self, key: &str) -> bool {
        self.config.contains_key(key)
    }
}

/// 从仓库读事实。任何读取失败 → `None`（调用方判安全网）。
pub fn read_facts(repo: &Path) -> Option<RepoFacts> {
    let list = crate::git::run(repo, &["config", "-z", "--list"]).ok()?;
    let mut config: HashMap<String, Vec<String>> = HashMap::new();
    for entry in list.split('\0').filter(|e| !e.is_empty()) {
        let (key, value) = entry.split_once('\n').unwrap_or((entry, ""));
        config
            .entry(key.to_string())
            .or_default()
            .push(value.to_string());
    }
    // 游离 HEAD 时 symbolic-ref 失败：当作没有当前分支（解析自然偏拒）。
    let branch = crate::git::run(repo, &["symbolic-ref", "-q", "HEAD"])
        .ok()
        .and_then(|r| r.strip_prefix("refs/heads/").map(str::to_string));
    Some(RepoFacts { branch, config })
}

/// 隐式目标推送的目标。`positional` = `git push` 之后的非选项词（选项已由 `push_class` 判过只含安全选项）。
/// `None` = 解不出确定目标（安全网）。
pub fn resolve(positional: &[&str], facts: &RepoFacts) -> Option<PushTarget> {
    let current = facts
        .branch
        .as_deref()
        .filter(|b| crate::push_class::plain_branch(b))?;
    let upstream_remote = facts.get(&format!("branch.{current}.remote"));
    let upstream_branch = facts
        .get(&format!("branch.{current}.merge"))
        .map(|m| m.strip_prefix("refs/heads/"));
    let (remote, refspec) = match positional {
        [] => {
            if facts.has(&format!("branch.{current}.pushremote")) || facts.has("remote.pushdefault")
            {
                return None;
            }
            (upstream_remote?, None)
        }
        [remote] => (*remote, None),
        [remote, refspec] => (*remote, Some(*refspec)),
        _ => return None,
    };
    if !crate::push_class::named_remote(remote) || !facts.has(&format!("remote.{remote}.url")) {
        return None;
    }
    let target = |branch: &str| {
        crate::push_class::plain_branch(branch).then(|| PushTarget {
            remote: remote.to_string(),
            branch: branch.to_string(),
        })
    };
    if let Some(refspec) = refspec {
        // 只认 `HEAD`：当前分支推到同名。`HEAD:HEAD` 等其余形状推到别的 ref，偏拒。
        return (refspec == "HEAD").then(|| target(current)).flatten();
    }
    if facts.has(&format!("remote.{remote}.push")) {
        return None;
    }
    let upstream = match (upstream_remote, upstream_branch) {
        (Some(r), Some(Some(b))) => Some((r, b)),
        // 上游分支不是 refs/heads/*：读不懂，偏拒。
        (Some(_), Some(None)) => return None,
        _ => None,
    };
    default_branch(facts, current, remote, upstream).and_then(target)
}

/// 无 refspec 时按 `push.default`（缺省 simple）推到的分支。`upstream` = (上游远端, 上游分支)。
fn default_branch<'a>(
    facts: &RepoFacts,
    current: &'a str,
    remote: &str,
    upstream: Option<(&str, &'a str)>,
) -> Option<&'a str> {
    let mode = facts
        .get("push.default")
        .map(str::to_ascii_lowercase)
        .unwrap_or_else(|| "simple".into());
    match mode.as_str() {
        "current" => Some(current),
        "simple" => match upstream {
            Some((r, b)) if r == remote => (b == current).then_some(current),
            Some(_) => Some(current),
            None => None,
        },
        "upstream" | "tracking" => match upstream {
            Some((r, b)) if r == remote => Some(b),
            _ => None,
        },
        _ => None,
    }
}

#[cfg(test)]
mod tests;

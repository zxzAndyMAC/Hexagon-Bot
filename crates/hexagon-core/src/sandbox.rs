//! 按风险类的 OS 沙箱（agent-senses 票 06 / ADR 0058-4）。
//!
//! 出处与代价模型：
//! - **双后端**：macOS `sandbox-exec`（seatbelt profile）、Linux `bwrap`。
//!   其他平台 → `Unavailable`——显式降级不假装隔离；裸跑仍过 Exec 必问
//!   卡，不对称代价：漏拦=一次未审副作用，多问=一次点击，故「不可用也
//!   不放行」。
//! - **seatbelt 的诚实边界**（ADR 0058-4）：macOS 沙箱网络是开关式
//!   （`(allow network*)`），没有 per-domain 粒度——域名级出网裁决永远
//!   在应用层 `permissions.rs` 做，OS 沙箱只是额外围笼。bash 的 `net`
//!   入参是唯一开关：默认关网，Agent 声明 `net:true` 才带网，且该声明
//!   进必问卡 raw_input 给负责人看。
//! - **统一漏斗**：agent 命令的 `Command::new` 唯一出口是
//!   `sessions::sh_command(dir, spec)`——wrap 在里面发生，grep 可证。
//!   kill 辅助里的 `sh -c kill` 是宿主侧工具进程，不经漏斗（本就不该
//!   被沙箱）。
//! - owned_globs 只取「仓库相对、无 `..`、无通配」的字面前缀转
//!   subpath 授权；绝对/逃逸形 glob 不生成沙箱授权（它们对 fs 工具
//!   同样过不了 repo_path，口径一致）。

use serde::Serialize;
use std::path::{Path, PathBuf};
use std::process::Command;

/// 一次执行要套的沙箱规格（由 `spec_for` 生成，`sessions` 消费）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SandboxSpec {
    /// macOS seatbelt profile 文本（sandbox-exec -p）。
    Seatbelt(String),
    /// bwrap 参数向量（`--` 之前的部分）。
    Bwrap(Vec<String>),
    /// 平台无可用沙箱后端——裸跑 + sandbox_unavailable 事件
    /// （原因取 `status().note`，规格本身不带串）。
    Unavailable,
}

/// IPC 三态（TopBar 徽章）：mode = seatbelt|bwrap|none。
#[derive(Debug, Clone, Serialize, ts_rs::TS)]
#[ts(export, export_to = "../../../ui/src/gen/")]
pub struct SandboxStatus {
    pub mode: String,
    pub available: bool,
    pub note: String,
}

/// 状态探测（结果缓存：可用性在进程生命周期内视为不变）。
pub fn status() -> SandboxStatus {
    use std::sync::OnceLock;
    static S: OnceLock<SandboxStatus> = OnceLock::new();
    S.get_or_init(detect_status).clone()
}

fn detect_status() -> SandboxStatus {
    if cfg!(target_os = "macos") {
        if Path::new("/usr/bin/sandbox-exec").exists() {
            return SandboxStatus {
                mode: "seatbelt".into(),
                available: true,
                note: "writes confined to repo; network gated per call".into(),
            };
        }
        return SandboxStatus {
            mode: "none".into(),
            available: false,
            note: "sandbox-exec missing — exec runs unsandboxed".into(),
        };
    }
    if cfg!(target_os = "linux") {
        let ok = Command::new("bwrap")
            .arg("--version")
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .status()
            .map(|s| s.success())
            .unwrap_or(false);
        return if ok {
            SandboxStatus {
                mode: "bwrap".into(),
                available: true,
                note: "writes confined to repo; network gated per call".into(),
            }
        } else {
            SandboxStatus {
                mode: "none".into(),
                available: false,
                note: "bwrap not found — exec runs unsandboxed".into(),
            }
        };
    }
    SandboxStatus {
        mode: "none".into(),
        available: false,
        note: "no sandbox backend on this platform".into(),
    }
}

/// 为一次执行生成沙箱规格。`net` = 本条命令的出网开关（bash `net`
/// 入参）；owned_globs 展开为额外可写子路径。
pub fn spec_for(repo_root: &Path, owned_globs: &[String], net: bool) -> SandboxSpec {
    let st = status();
    if !st.available {
        return SandboxSpec::Unavailable;
    }
    // canonicalize：seatbelt/bwrap 按真实路径判定——macOS 上 /var→/private/var
    // 等符号链接不解析会围错目录。原路径也保留在授权里（防链接形态写入）。
    let canon = std::fs::canonicalize(repo_root).unwrap_or_else(|_| repo_root.to_path_buf());
    match st.mode.as_str() {
        "seatbelt" => SandboxSpec::Seatbelt(seatbelt_profile(repo_root, &canon, owned_globs, net)),
        "bwrap" => SandboxSpec::Bwrap(bwrap_args(repo_root, &canon, owned_globs, net)),
        _ => SandboxSpec::Unavailable,
    }
}

/// 把已构造的 Command 包进沙箱。Unavailable 原样返回（裸跑由调用方
/// 落事件）。重建 Command 会丢 process_group——这里统一补回（本模块
/// 所有消费者都要进程组语义做 kill_tree）。
pub fn wrap_command(cmd: &mut Command, spec: &SandboxSpec) -> Command {
    let prog = cmd.get_program().to_string_lossy().into_owned();
    let args: Vec<String> = cmd
        .get_args()
        .map(|a| a.to_string_lossy().into_owned())
        .collect();
    let cwd = cmd.get_current_dir().map(PathBuf::from);
    let envs: Vec<(String, Option<String>)> = cmd
        .get_envs()
        .map(|(k, v)| {
            (
                k.to_string_lossy().into_owned(),
                v.map(|x| x.to_string_lossy().into_owned()),
            )
        })
        .collect();
    let mut c = match spec {
        SandboxSpec::Seatbelt(profile) => {
            let mut c = Command::new("/usr/bin/sandbox-exec");
            c.arg("-p").arg(profile).arg(&prog).args(&args);
            c
        }
        SandboxSpec::Bwrap(pre) => {
            let mut c = Command::new("bwrap");
            c.args(pre).arg("--").arg(&prog).args(&args);
            c
        }
        SandboxSpec::Unavailable => {
            let mut c = Command::new(&prog);
            c.args(&args);
            c
        }
    };
    if let Some(d) = cwd {
        c.current_dir(d);
    }
    for (k, v) in envs {
        match v {
            Some(v) => c.env(k, v),
            None => c.env_remove(k),
        };
    }
    #[cfg(unix)]
    {
        use std::os::unix::process::CommandExt;
        c.process_group(0);
    }
    c
}

// ---------- seatbelt ----------

/// seatbelt 授权串的引号转义（路径进 "…" 字面量）。
fn sbq(p: &Path) -> String {
    p.to_string_lossy()
        .replace('\\', "\\\\")
        .replace('"', "\\\"")
}

/// glob → 可写子路径：取首个通配符前的字面目录前缀，只允许仓库相对
/// 且无 `..`/绝对形态（逃逸形不授权——fail-closed，与 fs 工具同口径）。
fn glob_write_prefix(repo: &Path, glob: &str) -> Option<PathBuf> {
    if glob.starts_with('/') || glob.split('/').any(|s| s == "..") {
        return None;
    }
    let lit: Vec<&str> = glob
        .split('/')
        .take_while(|seg| !seg.contains(['*', '?', '[', ']']))
        .collect();
    if lit.is_empty() || lit.iter().all(|s| s.is_empty()) {
        return None;
    }
    let joined = lit.join("/");
    Some(repo.join(joined))
}

/// macOS seatbelt profile。基线 `(deny default)`：读全盘（只读不敏感，
/// 凭据类内容由应用层 builtin_deny 管）、写只放 repo + 系统临时/设备 +
/// owned_globs 前缀；网络按 `net` 开关。
fn seatbelt_profile(repo: &Path, canon: &Path, globs: &[String], net: bool) -> String {
    let mut writes = vec![
        "/dev/null".to_string(),
        "/dev/tty".to_string(),
        "/dev/ptmx".to_string(),
        "/tmp".to_string(),
        "/private/tmp".to_string(),
        "/private/var/folders".to_string(),
    ];
    // literal /dev/*、subpath 目录分开列
    let mut prof = String::from(
        "(version 1)\n(deny default)\n(allow process*)\n(allow signal)\n(allow sysctl-read)\n(allow mach-lookup)\n(allow ipc-posix-shm)\n(allow file-read*)\n(allow file-ioctl)\n(allow file-write*\n",
    );
    for w in writes.drain(..3) {
        prof.push_str(&format!("  (literal \"{w}\")\n"));
    }
    let mut subs: Vec<String> = writes;
    subs.push(sbq(repo));
    if canon != repo {
        subs.push(sbq(canon));
    }
    for g in globs {
        if let Some(p) = glob_write_prefix(repo, g) {
            let s = sbq(&p);
            if !subs.contains(&s) {
                subs.push(s);
            }
        }
        if canon != repo {
            if let Some(p) = glob_write_prefix(canon, g) {
                let s = sbq(&p);
                if !subs.contains(&s) {
                    subs.push(s);
                }
            }
        }
    }
    for s in subs {
        prof.push_str(&format!("  (subpath \"{s}\")\n"));
    }
    prof.push(')');
    if net {
        // ADR 0058-4：seatbelt 无 per-domain——开就是全网。
        // 域名裁决在 permissions.rs；这里只是围笼开关。
        prof.push_str("\n(allow network*)\n");
    }
    prof
}

// ---------- bwrap ----------

fn bwrap_args(repo: &Path, canon: &Path, globs: &[String], net: bool) -> Vec<String> {
    let mut a: Vec<String> = [
        "--die-with-parent",
        "--new-session",
        "--proc",
        "/proc",
        "--dev-bind",
        "/dev",
        "/dev",
        "--ro-bind",
        "/",
        "/",
        "--bind",
        "/tmp",
        "/tmp",
    ]
    .iter()
    .map(|s| s.to_string())
    .collect();
    let rw = |a: &mut Vec<String>, p: &Path| {
        a.push("--bind".into());
        a.push(p.to_string_lossy().into_owned());
        a.push(p.to_string_lossy().into_owned());
    };
    rw(&mut a, repo);
    if canon != repo {
        rw(&mut a, canon);
    }
    for g in globs {
        if let Some(p) = glob_write_prefix(repo, g) {
            rw(&mut a, &p);
        }
    }
    if !net {
        a.push("--unshare-net".into());
    }
    a.push("--chdir".into());
    a.push(repo.to_string_lossy().into_owned());
    a
}

#[cfg(test)]
mod tests {
    use super::*;
    use proptest::prelude::*;
    use std::path::PathBuf;

    fn subs_of(profile: &str) -> Vec<String> {
        profile
            .split("(subpath \"")
            .skip(1)
            .filter_map(|s| s.split('"').next().map(String::from))
            .collect()
    }

    proptest! {
        /// D12 不变量一：任意 owned_globs 生成的 profile，subpath 授权
        /// 只落在 repo 内或固定临时/设备白名单——永不授权 repo 外路径。
        #[test]
        fn profile_never_grants_outside_repo(
            globs in proptest::collection::vec("[a-zA-Z0-9._/*?-]{0,40}", 0..6),
            net in any::<bool>(),
        ) {
            let repo = Path::new("/repo/proj");
            let p = seatbelt_profile(repo, repo, &globs, net);
            for s in subs_of(&p) {
                let ok = s.starts_with("/repo/proj")
                    || ["/tmp", "/private/tmp", "/private/var/folders"]
                        .iter()
                        .any(|w| s.starts_with(w));
                prop_assert!(ok, "grant outside repo: {s}\n{p}");
            }
        }

        /// D12 不变量二：net=false 时 profile 不含 network 放行；
        /// deny default 在 ⇒ 网络被围死。
        #[test]
        fn no_net_means_no_network_allow(
            globs in proptest::collection::vec("[a-zA-Z0-9._/*?-]{0,40}", 0..4),
        ) {
            let repo = Path::new("/repo/proj");
            let p = seatbelt_profile(repo, repo, &globs, false);
            prop_assert!(!p.contains("network"), "net=false leaked network:\n{p}");
            prop_assert!(p.contains("(deny default)"));
            let p2 = seatbelt_profile(repo, repo, &globs, true);
            prop_assert!(p2.contains("(allow network*)"));
        }

        /// bwrap 同口径：net=false 必有 --unshare-net。
        #[test]
        fn bwrap_net_toggle(
            globs in proptest::collection::vec("[a-zA-Z0-9._/*?-]{0,40}", 0..4),
            net in any::<bool>(),
        ) {
            let repo = Path::new("/repo/proj");
            let a = bwrap_args(repo, repo, &globs, net);
            prop_assert_eq!(a.contains(&"--unshare-net".to_string()), !net);
            // repo 必须 rw bind
            prop_assert!(a.windows(3).any(|w| w[0] == "--bind" && w[1] == "/repo/proj"));
        }
    }

    #[test]
    fn glob_prefix_rejects_escapes() {
        let repo = Path::new("/r");
        assert_eq!(
            glob_write_prefix(repo, "src/**"),
            Some(PathBuf::from("/r/src"))
        );
        assert_eq!(
            glob_write_prefix(repo, "docs/a/*.md"),
            Some(PathBuf::from("/r/docs/a"))
        );
        assert_eq!(glob_write_prefix(repo, "/abs/**"), None);
        assert_eq!(glob_write_prefix(repo, "../out/**"), None);
        assert_eq!(glob_write_prefix(repo, "**"), None);
    }

    /// macOS 实测门控（票 06 验收）：沙箱内 repo 写成功、$HOME 写被
    /// 内核拒。只在有 sandbox-exec 的机器上跑。
    #[test]
    #[cfg(target_os = "macos")]
    fn seatbelt_confines_writes() {
        if !status().available {
            return;
        }
        let dir = tempfile::tempdir().unwrap();
        let repo = dir.path();
        let spec = spec_for(repo, &[], false);
        let inside = repo.join("ok.txt");
        let mut base = Command::new("sh");
        base.arg("-c")
            .arg(
                "echo hi > ok.txt; echo x > \"$HOME/sb_escape.txt\" 2>&1; echo code=$?; cat ok.txt",
            )
            .current_dir(repo);
        let out = wrap_command(&mut base, &spec).output().unwrap();
        let stdout = String::from_utf8_lossy(&out.stdout);
        assert!(inside.exists(), "repo write blocked: {stdout}");
        assert!(
            stdout.contains("code=1"),
            "escape write not denied: {stdout}"
        );
        assert!(!Path::new(&format!("{}/sb_escape.txt", std::env::var("HOME").unwrap())).exists());
        // 关网探测：curl 应失败（不真发请求——连不存在的端口也是网络动作）
        let mut c2 = Command::new("sh");
        c2.arg("-c")
            .arg("nc -z -w1 127.0.0.1 1 2>/dev/null; echo net=$?")
            .current_dir(repo);
        let o2 = wrap_command(&mut c2, &spec).output().unwrap();
        let s2 = String::from_utf8_lossy(&o2.stdout);
        assert!(!s2.contains("net=0"), "net open under net=false: {s2}");
    }
}

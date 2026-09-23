//! 诊断记录（2026-09-23 负责人裁决 / diagnostic-records 票 01）。
//!
//! 一次内部判定的结构化记录，只用来在测试时对上「走了哪一支」——不是业务
//! 事实，不进轨迹、不进事件表。分类只有四档。判定、槽位的正常回退、宿主
//! 记 Debug。拒绝，以及槽位上的失败，记 Warn。
//!
//! 记录里是种类、id、分支、原因码、耗时。提示词、钥匙、工具输出和消息
//! 正文不进这里——它们留在轨迹和消息里。测试不断言日志字符串，断言的是
//! `records()` 读回的结构。
//!
//! 一本账：`<日志目录>/diagnostics.jsonl` 是记录的唯一存储，设置「日志」
//! 页经 [`records`] 读回这同一份文件——不落内存副本、不送外部观测服务。
//! 写盘与文本日志同一闸（`log::max_level`，开关的唯一真相）：关着只留
//! Warn——Debug 记录根本不落盘。读回再按当前开关过滤一次（关着时，开着
//! 期间写的旧 Debug 记录同样不显示）。
//! 被否决：写盘不受开关限制（重开开关能回放关掉期间的判定）——「关着只
//! 留 Warn」按负责人裁决是落盘语义，不只是视图过滤。

use serde::{Deserialize, Serialize};
use std::io::Write as _;
use std::path::{Path, PathBuf};
use std::sync::{Mutex, RwLock};

/// 分类四档（词表定名，不可加档）。
pub const CLASS_JUDGE: &str = "判定";
pub const CLASS_REJECT: &str = "拒绝";
pub const CLASS_SLOT: &str = "槽位";
pub const CLASS_HOST: &str = "宿主";

const FILE_NAME: &str = "diagnostics.jsonl";
/// 落盘有界：超过即砍最旧段，只留最近的完整行。
const MAX_BYTES: u64 = 256 * 1024;
const KEEP_BYTES: usize = 128 * 1024;
/// 页面只服务「最近」——读回上限。
const READ_LIMIT: usize = 300;

/// 一条诊断记录（ADR 0054）：唯一字段集，没有的 id 就是 null。
#[derive(Debug, Clone, Serialize, Deserialize, ts_rs::TS)]
#[ts(export, export_to = "../../../ui/src/gen/")]
pub struct DiagRecord {
    /// RFC3339 UTC。
    pub ts: String,
    /// 四档之一：判定 / 拒绝 / 槽位 / 宿主。
    pub class: String,
    /// debug | warn。
    pub level: String,
    pub project: Option<String>,
    pub agent: Option<String>,
    pub activation: Option<String>,
    pub trace: Option<String>,
    /// 走了哪一支（如 permission / pm_route / execute_judgment）。
    pub branch: String,
    /// 分支上的原因码（如 builtin_deny / fallback_default:chat）。
    pub code: String,
    #[ts(type = "number")] // JS number 域（ts-rs v11 默认 u64→bigint，与 wire 不符）
    pub ms: u64,
}

static DIR: RwLock<Option<PathBuf>> = RwLock::new(None);
/// JSONL 追加序列化：多线程同写不串行（POSIX O_APPEND 单写是原子的，
/// 但行内多段 write 会交叠——一条 write_all 加一把锁最简单）。
static APPEND: Mutex<()> = Mutex::new(());

/// 落盘目录由壳层启动时注入（与 tauri-plugin-log 的 LogDir 同一目录，
/// 「日志目录里的文件是这些记录的落盘」）。
pub fn set_dir(dir: &Path) {
    *DIR.write().unwrap_or_else(|e| e.into_inner()) = Some(dir.to_path_buf());
}

fn sink_path() -> PathBuf {
    if let Some(d) = DIR.read().unwrap_or_else(|e| e.into_inner()).clone() {
        return d.join(FILE_NAME);
    }
    #[cfg(test)]
    {
        // 测试进程私目录：不写进真机的日志目录。同进程各测试共享，
        // 用例用唯一 code 标识自己的记录。
        std::env::temp_dir()
            .join(format!("hexagon-diag-test-{}", std::process::id()))
            .join(FILE_NAME)
    }
    #[cfg(not(test))]
    {
        // set_dir 没跑过（壳层 setup 失败或非常规入口）时的兜底——
        // 落到 ~/.hexagon/logs 而不是 cwd 下一个裸文件名；拿不到 HOME
        // 才退到 cwd。与 hexagon.log 的 LogDir 不是同一目录时这里会偏，
        // 但比静默写 cwd 可预期。
        std::env::var_os("HOME")
            .map(|h| PathBuf::from(h).join(".hexagon/logs").join(FILE_NAME))
            .unwrap_or_else(|| PathBuf::from(FILE_NAME))
    }
}

#[allow(clippy::too_many_arguments)]
pub fn note(
    class: &str,
    warn: bool,
    project: Option<&str>,
    agent: Option<&str>,
    activation: Option<&str>,
    trace: Option<&str>,
    branch: &str,
    code: &str,
    started: std::time::Instant,
) {
    let ms = started.elapsed().as_millis() as u64;
    // 文本行照旧走 log——hexagon.log 由 tauri-plugin-log 落盘+轮转，
    // 开关经 max_level 收口（写盘继续走现在的插件，本模块不接管）。
    let line = format!(
        "diag class={class} project={} agent={} activation={} trace={} branch={branch} code={code} ms={ms}",
        project.unwrap_or("-"),
        agent.unwrap_or("-"),
        activation.unwrap_or("-"),
        trace.unwrap_or("-"),
    );
    if warn {
        log::warn!("{line}");
    } else {
        log::debug!("{line}");
    }
    // 结构化账本与文本行同一闸（模块头裁决）：关着只留 Warn。
    // 门用 max_level() 不是 log_enabled!——后者还问 logger().enabled()，
    // 测试进程没装 logger 时恒 false，记录会整批丢。
    let level = if warn {
        log::Level::Warn
    } else {
        log::Level::Debug
    };
    if level.to_level_filter() > log::max_level() {
        return;
    }
    append(&DiagRecord {
        ts: now_iso(),
        class: class.into(),
        level: if warn { "warn" } else { "debug" }.into(),
        project: project.map(Into::into),
        agent: agent.map(Into::into),
        activation: activation.map(Into::into),
        trace: trace.map(Into::into),
        branch: branch.into(),
        code: code.into(),
        ms,
    });
}

/// 宿主类记录的便捷入口：无项目/Agent/激活/轨迹 上下文（启动、开关、
/// 凭据库、沙箱）。
pub fn host(branch: &str, code: &str, started: std::time::Instant) {
    note(
        CLASS_HOST, false, None, None, None, None, branch, code, started,
    );
}

/// 「槽位」正常回退（Debug）：`slot` 没单独绑定，落到了 default——
/// 原因码写明哪个槽回退（票 03）。回退判定在调用点用
/// [`crate::provider_config::fell_back_to_default`]；没绑到任何东西
/// 是错误不是回退，那种地方别记。`started` 取解析前一刻，ms 量的是
/// 这次槽位解析本身，不是落记录这一刻。
pub fn slot_fallback(
    project: Option<&str>,
    agent: Option<&str>,
    activation: Option<&str>,
    branch: &str,
    slot: &str,
    started: std::time::Instant,
) {
    note(
        CLASS_SLOT,
        false,
        project,
        agent,
        activation,
        None,
        branch,
        &format!("fallback_default:{slot}"),
        started,
    );
}

fn append(r: &DiagRecord) {
    let _g = APPEND.lock().unwrap_or_else(|e| e.into_inner());
    let path = sink_path();
    if let Some(parent) = path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    if std::fs::metadata(&path).map(|m| m.len()).unwrap_or(0) > MAX_BYTES {
        truncate_oldest(&path);
    }
    if let Ok(mut f) = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(&path)
    {
        if let Ok(mut line) = serde_json::to_string(r) {
            line.push('\n');
            let _ = f.write_all(line.as_bytes());
        }
    }
}

/// 只留文件尾部 KEEP_BYTES 的完整行（从最旧的段砍，不写第二本账）。
fn truncate_oldest(path: &Path) {
    let Ok(text) = std::fs::read(path) else {
        return;
    };
    let start = text.len().saturating_sub(KEEP_BYTES);
    let start = text[start..]
        .iter()
        .position(|b| *b == b'\n')
        .map(|i| start + i + 1)
        .unwrap_or(text.len());
    let _ = std::fs::write(path, &text[start..]);
}

/// 「日志」页读回（票 01）：与落盘文件同一份。
/// - project=Some(p)：本项目记录 + 宿主类记录（宿主不写 project id）；
/// - project=None：只回宿主类记录——非宿主类的 projectless 行是异常，
///   不借「没写 project」混进无项目视图（票 01「无项目只显示宿主」）；
/// - 开关关着（max_level < Debug）时 Debug 记录读回也滤掉——开着期间
///   写的旧 Debug 记录同样不显示；
/// - 最新在前，封顶 READ_LIMIT。
pub fn records(project: Option<&str>, class: Option<&str>) -> Vec<DiagRecord> {
    let Ok(text) = std::fs::read_to_string(sink_path()) else {
        return vec![];
    };
    let debug_on = log::max_level() >= log::LevelFilter::Debug;
    let mut out: Vec<DiagRecord> = text
        .lines()
        .filter_map(|l| serde_json::from_str(l).ok())
        .filter(|r: &DiagRecord| debug_on || r.level != "debug")
        .filter(|r| class.is_none_or(|c| r.class == c))
        .filter(|r| match project {
            Some(p) => r.project.as_deref() == Some(p) || r.class == CLASS_HOST,
            None => r.class == CLASS_HOST,
        })
        .collect();
    out.reverse();
    out.truncate(READ_LIMIT);
    out
}

/// epoch 秒 → RFC3339 UTC（不落 chrono；天→民用日期用标准换算）。
fn now_iso() -> String {
    let secs = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    let days = (secs / 86_400) as i64;
    let rem = secs % 86_400;
    // Howard Hinnant civil_from_days
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = (z - era * 146_097) as u64;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let y = yoe as i64 + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let mo = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = if mo <= 2 { y + 1 } else { y };
    format!(
        "{y:04}-{mo:02}-{d:02}T{:02}:{:02}:{:02}Z",
        rem / 3600,
        rem % 3600 / 60,
        rem % 60
    )
}

/// log::set_max_level 是进程全局——碰它且断言记录的用例必须同锁排队，
/// 否则并发用例会在「关着」窗口里丢 Debug 记录。diag.rs 与 api/tests.rs
/// 的记录用例共用这一把。
#[cfg(test)]
pub(crate) static TEST_LEVEL_LOCK: Mutex<()> = Mutex::new(());

#[cfg(test)]
mod tests {
    use super::*;

    fn set_level(f: log::LevelFilter) {
        log::set_max_level(f);
    }

    #[test]
    fn iso_clock() {
        let s = now_iso();
        assert_eq!(s.len(), 20, "{s}");
        assert!(s.ends_with('Z') && s.contains('T'));
    }

    #[test]
    fn roundtrip_scope_and_level() {
        let _g = TEST_LEVEL_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        set_level(log::LevelFilter::Debug);
        let t = std::time::Instant::now();
        note(
            CLASS_JUDGE,
            false,
            Some("p1"),
            Some("a9"),
            None,
            None,
            "test_branch",
            "ut_judge",
            t,
        );
        note(
            CLASS_REJECT,
            true,
            Some("p1"),
            None,
            None,
            None,
            "test_branch",
            "ut_reject",
            t,
        );
        host("test_host", "ut_host", t);
        note(
            CLASS_SLOT,
            false,
            Some("other-proj"),
            None,
            None,
            None,
            "test_branch",
            "ut_other",
            t,
        );

        // 有项目：本项目 + 宿主；别的项目不进列。
        let rs = records(Some("p1"), None);
        assert!(rs.iter().any(|r| r.code == "ut_judge"));
        assert!(rs.iter().any(|r| r.code == "ut_reject"));
        assert!(rs.iter().any(|r| r.code == "ut_host"));
        assert!(!rs.iter().any(|r| r.code == "ut_other"));
        assert!(rs
            .iter()
            .all(|r| r.project.is_none() || r.project.as_deref() == Some("p1")));
        // 无项目：只有宿主类——projectless 的非宿主行（异常行）
        // 不借「没写 project」混进无项目视图。
        note(
            CLASS_SLOT,
            false,
            None,
            None,
            None,
            None,
            "test_branch",
            "ut_stray",
            t,
        );
        let rs = records(None, None);
        assert!(rs.iter().all(|r| r.class == CLASS_HOST));
        assert!(rs.iter().any(|r| r.code == "ut_host"));
        assert!(!rs.iter().any(|r| r.code == "ut_judge"));
        assert!(!rs.iter().any(|r| r.code == "ut_stray"));
        // 分类过滤。
        let rs = records(Some("p1"), Some(CLASS_REJECT));
        assert!(rs.iter().all(|r| r.class == CLASS_REJECT));
        // 字段面：只有规格字段。
        let r = rs.iter().find(|r| r.code == "ut_reject").unwrap();
        assert_eq!(r.level, "warn");
        assert_eq!(r.branch, "test_branch");

        // 关着：读回滤 Debug（开着期间写的旧记录同样不显示），Warn 仍在。
        set_level(log::LevelFilter::Warn);
        let rs = records(Some("p1"), None);
        assert!(rs.iter().all(|r| r.level != "debug"));
        assert!(rs.iter().any(|r| r.code == "ut_reject"));
        // 关着时 Debug 不落盘；Warn 照落。
        note(
            CLASS_JUDGE,
            false,
            Some("p1"),
            None,
            None,
            None,
            "test_branch",
            "ut_off_debug",
            t,
        );
        note(
            CLASS_REJECT,
            true,
            Some("p1"),
            None,
            None,
            None,
            "test_branch",
            "ut_off_warn",
            t,
        );
        set_level(log::LevelFilter::Debug);
        let rs = records(Some("p1"), None);
        assert!(
            !rs.iter().any(|r| r.code == "ut_off_debug"),
            "关着期间 Debug 不落盘"
        );
        assert!(rs.iter().any(|r| r.code == "ut_off_warn"));
    }
}

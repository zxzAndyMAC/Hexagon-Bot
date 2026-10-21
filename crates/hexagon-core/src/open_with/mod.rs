//! 「打开方式」：用本机已装的 IDE / 编辑器打开项目里的文件、目录或整个项目
//! （owner-followups-1015 #31，2026-10-21 负责人需求；components A16b「打开方式」子菜单）。
//!
//! 判定 = 三道闸，全在 core（壳层只转发）：
//! 1. **根约束**：路径走 `reveal::resolve_in_root`（词法消解 + 解链接后必须在项目根内），越界拒。
//! 2. **白名单**：只列 / 只启动 [`EDITORS`] 里的 bundle id——系统说「能打开」的应用很多
//!    （预览、终端、浏览器、脚本解释器……），把任意登记应用交给一条路径就是执行面。
//! 3. **系统登记**：应用位置只取 LaunchServices 的答复（[`AppCatalog`]），不拼路径、不猜
//!    `/Applications/X.app`；启动时参数逐个交给进程，不经 shell。
//!
//! 文件 = 系统登记的「能打开这个文件」的应用 ∩ 白名单；目录（含项目根）= 白名单里
//! （系统登记能打开该目录 ∪ 已装且标了 `opens_dirs`）的应用——只列支持打开目录的。
//! 启动前按同一规则**重算一次**候选：界面传来的 app id 不在当下候选里就拒（防止拿文件的候选去开目录、
//! 或界面缓存了已卸载的应用）。
//!
//! 代价模型（fail-closed）：false negative（少列一个能用的编辑器）= 负责人自己去访达里拖一次；
//! false positive（放行非白名单应用或根外路径）= 一次未审的外部程序启动。实现偏向拒绝：
//! 探测失败一律当「没有」，路径判不了一律拒。
//!
//! 探测在 [`AppCatalog`] 接缝后面：测试注入假目录，从不真启动应用；macOS 用 NSWorkspace，
//! 其他平台返回空（未做，见 [`system_catalog`]）。
//! 被否决：硬编码 `/Applications/<名>.app` 路径（装在 ~/Applications、改名、JetBrains Toolbox
//! 都会漏，且路径即信任）；`open -a <名字>`（按显示名解析，同名应用可冒充）。

#[cfg(target_os = "macos")]
mod macos;

use crate::reveal::{resolve_in_root, RevealError};
use serde::Serialize;
use std::collections::HashSet;
use std::path::{Path, PathBuf};

/// 白名单一项：bundle id + 是否已知支持「打开文件夹」。
#[derive(Debug, Clone, Copy)]
pub struct EditorEntry {
    pub bundle_id: &'static str,
    pub opens_dirs: bool,
}

const fn ed(bundle_id: &'static str, opens_dirs: bool) -> EditorEntry {
    EditorEntry {
        bundle_id,
        opens_dirs,
    }
}

/// IDE / 编辑器白名单（唯一一处）。出处：#31 负责人列举（VS Code、JetBrains 系、Cursor、
/// Windsurf、Devin、Zed、Xcode、Sublime 等，「实际以探测为准」）；bundle id 取各应用 Info.plist
/// 的 `CFBundleIdentifier`（2026-10-21 本机核对：Cursor、Devin（沿用 Windsurf 的
/// `com.exafunction.windsurf`）、Antigravity、Xcode）。显示名不在这里——取系统登记的应用名，
/// 改名（Windsurf → Devin）跟着系统走。加一项 = 改这张表 + glossary。
/// `opens_dirs = false` 的只在系统登记它能打开该目录时才列给目录。
pub const EDITORS: &[EditorEntry] = &[
    ed("com.microsoft.VSCode", true),
    ed("com.microsoft.VSCodeInsiders", true),
    ed("com.vscodium", true),
    ed("com.todesktop.230313mzl4w4u92", true), // Cursor
    ed("com.exafunction.windsurf", true),      // Windsurf / Devin
    ed("com.google.antigravity", true),
    ed("com.google.antigravity-ide", true),
    ed("dev.kiro.desktop", true),
    ed("com.trae.app", true),
    ed("dev.zed.Zed", true),
    ed("dev.zed.Zed-Preview", true),
    ed("com.apple.dt.Xcode", true),
    ed("com.sublimetext.4", true),
    ed("com.sublimetext.3", true),
    ed("com.jetbrains.intellij", true),
    ed("com.jetbrains.intellij.ce", true),
    ed("com.jetbrains.WebStorm", true),
    ed("com.jetbrains.pycharm", true),
    ed("com.jetbrains.pycharm.ce", true),
    ed("com.jetbrains.goland", true),
    ed("com.jetbrains.CLion", true),
    ed("com.jetbrains.rustrover", true),
    ed("com.jetbrains.PhpStorm", true),
    ed("com.jetbrains.rider", true),
    ed("com.jetbrains.rubymine", true),
    ed("com.jetbrains.datagrip", true),
    ed("com.jetbrains.fleet", true),
    ed("com.google.android.studio", true),
    ed("com.panic.Nova", true),
    ed("com.barebones.bbedit", true),
    ed("com.macromates.TextMate", true),
    ed("com.coteditor.CotEditor", false),
];

pub fn editor(bundle_id: &str) -> Option<&'static EditorEntry> {
    EDITORS.iter().find(|e| e.bundle_id == bundle_id)
}

/// 系统登记的一个应用（LaunchServices 答复）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InstalledApp {
    pub bundle_id: String,
    /// 系统显示名（不翻译）。
    pub name: String,
    /// 应用包位置——只来自系统答复，启动时原样交回系统。
    pub app_path: PathBuf,
}

/// 探测接缝：系统 API 的最小面。实现只回答事实，不做白名单判定。
pub trait AppCatalog: Send + Sync {
    /// 系统登记的「能打开 `target`」的应用（文件按类型，目录按 `public.folder`）。
    fn handlers(&self, target: &Path) -> Vec<InstalledApp>;
    /// 按 bundle id 找已装应用（系统首选的那一份）。
    fn installed(&self, bundle_id: &str) -> Option<InstalledApp>;
    /// 应用图标 → `data:image/png;base64,…`（菜单行 16pt @2x = 32px）；取不到 `None` → 界面中性图标。
    fn icon_data_url(&self, app: &InstalledApp) -> Option<String>;
    /// 用该应用打开 `target`（已判过根的绝对路径）。
    fn launch(&self, app: &InstalledApp, target: &Path) -> Result<(), String>;
}

/// 「打开方式」子菜单的一行（IPC DTO）。
#[derive(Debug, Clone, Serialize, PartialEq, Eq, ts_rs::TS)]
#[ts(export, export_to = "../../../ui/src/gen/")]
pub struct OpenWithApp {
    /// bundle id（`open_with` 回传）。
    pub id: String,
    /// 系统显示名，不翻译。
    pub name: String,
    /// PNG data URL；`null` = 中性图标。
    pub icon: Option<String>,
}

/// 拒绝 / 失败原因（IPC 码面见 `errcode.rs`，界面走 `errors.<code>`）。
#[derive(Debug, thiserror::Error)]
pub enum OpenWithError {
    /// 规范化后不在项目根内。
    #[error("path is outside the project: {0}")]
    OpenWithOutsideProject(String),
    /// 项目内没有这条路径。
    #[error("no such path in the project: {0}")]
    OpenWithMissing(String),
    /// 应用不在白名单、没装，或不能打开这类目标（文件 / 目录）。
    #[error("this app can't open the path: {0}")]
    OpenWithUnsupported(String),
    /// 系统没能启动应用。
    #[error("could not open the app: {0}")]
    OpenWithLaunch(String),
}

fn resolve(root: &Path, rel: &str) -> Result<PathBuf, OpenWithError> {
    resolve_in_root(root, rel).map_err(|e| match e {
        RevealError::RevealOutsideProject(p) => OpenWithError::OpenWithOutsideProject(p),
        RevealError::RevealMissing(p) | RevealError::RevealLaunch(p) => {
            OpenWithError::OpenWithMissing(p)
        }
    })
}

/// 白名单 ∩ 系统登记，按显示名排序（同名按 bundle id）。纯判定，不写诊断。
fn eligible(catalog: &dyn AppCatalog, target: &Path) -> Vec<InstalledApp> {
    let is_dir = target.is_dir();
    let mut seen = HashSet::new();
    let mut out: Vec<InstalledApp> = Vec::new();
    for app in catalog.handlers(target) {
        if editor(&app.bundle_id).is_some() && seen.insert(app.bundle_id.clone()) {
            out.push(app);
        }
    }
    if is_dir {
        for e in EDITORS.iter().filter(|e| e.opens_dirs) {
            if seen.contains(e.bundle_id) {
                continue;
            }
            // 系统答复的 id 必须与问的一致——实现层返回别的应用不算数。
            if let Some(app) = catalog
                .installed(e.bundle_id)
                .filter(|a| a.bundle_id == e.bundle_id)
            {
                seen.insert(app.bundle_id.clone());
                out.push(app);
            }
        }
    }
    out.sort_by(|a, b| {
        a.name
            .to_lowercase()
            .cmp(&b.name.to_lowercase())
            .then_with(|| a.bundle_id.cmp(&b.bundle_id))
    });
    out
}

fn note(class: &str, warn: bool, branch: &str, code: &str, started: std::time::Instant) {
    crate::diag::note(
        class,
        warn,
        Some(crate::PROJECT_ID),
        None,
        None,
        None,
        branch,
        code,
        started,
    );
}

/// 候选列表：`rel` 为项目相对路径（空串 = 项目根）。判定记 Debug，拒绝记 Warn。
pub fn candidates(
    catalog: &dyn AppCatalog,
    root: &Path,
    rel: &str,
) -> Result<Vec<OpenWithApp>, OpenWithError> {
    let started = std::time::Instant::now();
    let target = match resolve(root, rel) {
        Ok(t) => t,
        Err(e) => {
            note(
                crate::diag::CLASS_REJECT,
                true,
                "open_with_candidates",
                &crate::errcode::variant_code(&e),
                started,
            );
            return Err(e);
        }
    };
    let list: Vec<OpenWithApp> = eligible(catalog, &target)
        .into_iter()
        .map(|app| OpenWithApp {
            icon: catalog.icon_data_url(&app),
            id: app.bundle_id,
            name: app.name,
        })
        .collect();
    let code = if list.is_empty() { "empty" } else { "listed" };
    note(
        crate::diag::CLASS_JUDGE,
        false,
        "open_with_candidates",
        code,
        started,
    );
    Ok(list)
}

/// 用 `app_id` 打开 `rel`。白名单、系统登记、目标种类在这里按候选规则重判一次。
pub fn open_with(
    catalog: &dyn AppCatalog,
    root: &Path,
    rel: &str,
    app_id: &str,
) -> Result<(), OpenWithError> {
    let started = std::time::Instant::now();
    let out = (|| {
        let target = resolve(root, rel)?;
        let app = eligible(catalog, &target)
            .into_iter()
            .find(|a| a.bundle_id == app_id)
            .ok_or_else(|| OpenWithError::OpenWithUnsupported(app_id.to_string()))?;
        catalog
            .launch(&app, &target)
            .map_err(OpenWithError::OpenWithLaunch)
    })();
    match &out {
        Ok(()) => note(
            crate::diag::CLASS_HOST,
            false,
            "open_with",
            "launched",
            started,
        ),
        Err(e) => note(
            crate::diag::CLASS_REJECT,
            true,
            "open_with",
            &crate::errcode::variant_code(e),
            started,
        ),
    }
    out
}

/// 本机系统目录。macOS = NSWorkspace / LaunchServices；其他平台未做，恒空（菜单显示空态）。
pub fn system_catalog() -> &'static dyn AppCatalog {
    #[cfg(target_os = "macos")]
    {
        static CATALOG: std::sync::OnceLock<macos::MacCatalog> = std::sync::OnceLock::new();
        CATALOG.get_or_init(macos::MacCatalog::default)
    }
    #[cfg(not(target_os = "macos"))]
    {
        static CATALOG: EmptyCatalog = EmptyCatalog;
        &CATALOG
    }
}

/// Windows / Linux：未做探测（#31 注明），列表恒空、启动恒拒。
#[cfg(not(target_os = "macos"))]
struct EmptyCatalog;

#[cfg(not(target_os = "macos"))]
impl AppCatalog for EmptyCatalog {
    fn handlers(&self, _: &Path) -> Vec<InstalledApp> {
        Vec::new()
    }
    fn installed(&self, _: &str) -> Option<InstalledApp> {
        None
    }
    fn icon_data_url(&self, _: &InstalledApp) -> Option<String> {
        None
    }
    fn launch(&self, _: &InstalledApp, _: &Path) -> Result<(), String> {
        Err("open with is only available on macOS".into())
    }
}

#[cfg(test)]
mod tests;

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
//! 候选按目标**类型**分三类（#32，2026-10-21 负责人裁决「候选随文件类型变」）：
//! - **目录**（含项目根）= 白名单里（系统登记能打开该目录 ∪ 已装且标了 `opens_dirs`）的应用——只列支持打开目录的。
//! - **文本 / 代码 / 脚本 / 可执行**（UTI 符合 [`TEXT_TYPES`] 或 [`EXEC_TYPES`]）= 系统登记的「能打开这个文件」的应用 ∩ 白名单（#31 原口径）。
//! - **其余**（pdf、演示稿、文稿、表格、图片、音视频……，以及系统给不出 UTI 的未知类型）= 系统登记的应用，
//!   同访达「打开方式」：系统默认应用排第一并标 `is_default`，其余按显示名，去重、最多 [`MAX_SYSTEM_APPS`] 个；
//!   不额外补 IDE（系统自己登记了才出现），也不走白名单——但剔掉 [`LAUNCHER_DENY`]（终端 / 脚本运行器 / 安装器 / 挂载 / 描述文件……）。
//!   分类走 UTI 符合关系（[`AppCatalog::type_of`] / [`AppCatalog::conforms`]，macOS = LaunchServices），不只看扩展名：
//!   `.command`、无扩展名但可执行的文件、`.jsonc` 这类扩展名表漏掉的类型都由系统判。
//!
//! 启动前按同一规则**重算一次**候选：界面传来的 app id 不在当下候选里就拒（防止拿文件的候选去开目录、
//! 或界面缓存了已卸载的应用）。
//!
//! 代价模型（两类不对称，ticket 33 / 2026-10-25 批次审查 S1 把注释改成与代码一致）：
//! - **文本 / 目录类 = 白名单（fail-closed）**：false negative（少列一个能用的编辑器）= 负责人自己去访达里拖一次；
//!   false positive（放行非白名单应用或根外路径）= 一次未审的外部程序启动。实现偏向拒绝：探测失败当「没有」，路径判不了一律拒。
//! - **其余类 = 系统列表减 deny 列表（fail-open 于 deny 之外）**：负责人 #32 裁决（2026-10-21）——看 pdf 要的是预览 / Keynote，
//!   不是 IDE；**UTI 探测失败 / 未知类型也走这一类**（系统给什么列什么），所以这里的 false positive 是「系统登记的某个
//!   会执行 / 安装所开文件的处理程序被列出并启动」，false negative 是「少列一个无害的查看器」。收口靠两道：脚本 / 可执行类型
//!   永远走白名单（交给终端就是执行），系统列表再剔一遍 [`LAUNCHER_DENY`]（以 bundle id，见那张表）。deny 列表注定不完备——
//!   漏一个执行类处理程序就是一次未审的副作用，所以宁可多挡（Archive Utility、磁盘映像挂载这类「开即落盘」的也挡）。
//!
//! 被否决：按扩展名表分类（`.command`、无扩展名脚本会落进「其余」被交给终端）；「其余」类也只列白名单（看不了 pdf）；
//! 未知类型一律空列表（违背 #32 裁决「未知类型走系统列表」）。
//!
//! 探测在 [`AppCatalog`] 接缝后面：测试注入假目录，从不真启动应用；macOS 用 NSWorkspace，
//! 其他平台返回空（未做，见 [`system_catalog`]）。
//! 被否决：硬编码 `/Applications/<名>.app` 路径（装在 ~/Applications、改名、JetBrains Toolbox
//! 都会漏，且路径即信任）；`open -a <名字>`（按显示名解析，同名应用可冒充）。

#[cfg(target_os = "macos")]
mod macos;

use crate::reveal::{resolve_in_root, InRootError};
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

/// UTI 符合其一 = 文本 / 代码类：候选只列白名单编辑器（#31 口径）。`public.text` 已涵盖 plain-text /
/// source-code / json / xml 的大多数，其余逐个列出是防系统登记表里某个类型只声明了其中一支。
pub const TEXT_TYPES: &[&str] = &[
    "public.text",
    "public.plain-text",
    "public.source-code",
    "public.script",
    "public.shell-script",
    "public.json",
    "public.xml",
    "public.yaml",
    "net.daringfireball.markdown",
];

/// UTI 符合其一 = 可执行 / 应用包：同文本类只列白名单编辑器——系统登记能「打开」它们的是终端、安装器，打开即执行。
pub const EXEC_TYPES: &[&str] = &[
    "public.executable",
    "public.unix-executable",
    "com.apple.application",
    "com.apple.installer-package-archive",
    "com.microsoft.windows-executable",
];

/// 系统没登记 UTI（`dyn.*` / 取不到）时的补充：这些扩展名按文本 / 代码类。只在 UTI 缺位时用，不覆盖系统已给的类型。
/// 出处：#32 本机核对（2026-10-25 `list_this_machine`）——这台机器上 `.rs` 的 UTI 是 `dyn.ah62d4rv4ge81e62`
/// （装的 IDE 都没声明 `public.rust-source`），纯 UTI 分类把 Rust 源码落进「其余」、丢掉 Xcode 等白名单项。
/// 负责人裁决「不只看扩展名」= UTI 为主、扩展名补缺；被否决：缺位一律当「其余」（源码文件列成系统杂项）、
/// 嗅探内容判文本（读盘、且二进制前缀不可靠）。
pub const TEXT_EXTS: &[&str] = &[
    "rs",
    "ts",
    "tsx",
    "mts",
    "cts",
    "js",
    "jsx",
    "mjs",
    "cjs",
    "go",
    "py",
    "rb",
    "java",
    "kt",
    "kts",
    "swift",
    "c",
    "h",
    "cc",
    "cpp",
    "hpp",
    "m",
    "mm",
    "cs",
    "php",
    "lua",
    "zig",
    "dart",
    "scala",
    "ex",
    "exs",
    "hs",
    "toml",
    "yaml",
    "yml",
    "json",
    "jsonc",
    "json5",
    "ini",
    "cfg",
    "conf",
    "env",
    "lock",
    "md",
    "mdx",
    "txt",
    "csv",
    "tsv",
    "log",
    "sql",
    "graphql",
    "proto",
    "css",
    "scss",
    "less",
    "html",
    "vue",
    "svelte",
    "sh",
    "bash",
    "zsh",
    "fish",
    "ps1",
    "gradle",
    "cmake",
    "make",
    "mk",
    "dockerfile",
    "gitignore",
];

/// 「其余」类系统列表里剔掉的处理程序：系统可能登记它们能开某个未知类型（或 `public.data`），交过去就是执行 / 安装 /
/// 改系统状态。以 bundle id 判（显示名可冒充、可本地化）。出处：#32 原表（终端 / 脚本运行器 / 安装器）；
/// ticket 33（2026-10-25 批次审查 S1）补齐「打开即执行 / 安装所开文件」的系统处理程序——bundle id 取本机
/// `/System/Library/CoreServices`、`/System/Applications` 下各应用 Info.plist 的 `CFBundleIdentifier`（macOS 26 核对），
/// Jar Launcher 新旧两个 id 都列。加一项 = 改这张表 + glossary；属性测试钉「表内 id 永不出现在任何候选里、也启动不了」。
pub const LAUNCHER_DENY: &[&str] = &[
    // 终端：打开即执行脚本
    "com.apple.Terminal",
    "com.googlecode.iterm2",
    "com.mitchellh.ghostty",
    "dev.warp.Warp-Stable",
    "net.kovidgoyal.kitty",
    "io.alacritty",
    "com.github.wez.wezterm",
    "co.zeit.hyper",
    // 脚本 / 自动化运行器
    "com.apple.ScriptEditor2",
    "com.apple.ScriptMenuApp",
    "com.apple.ScriptMonitor",
    "com.apple.AppleScriptUtility",
    "com.apple.FolderActionsSetup",
    "com.apple.Automator",
    "com.apple.automator.runner",
    "com.apple.Automator.Automator-Application-Stub",
    "com.apple.AutomatorInstaller",
    "com.apple.shortcuts",
    "com.apple.shortcuts.droplet",
    "com.apple.shortcuts.events",
    "com.apple.ShortcutsActions",
    "org.python.PythonLauncher",
    // Java 归档运行器（.jar 打开即执行）
    "com.apple.JarLauncher",
    "com.apple.JavaLauncher",
    // 安装器（.pkg / .ipa / 命令行工具）
    "com.apple.installer",
    "com.apple.Installer-Progress",
    "com.apple.IPAInstaller",
    "com.apple.dt.CommandLineTools.installondemand",
    // 挂载 / 解包：打开即落盘
    "com.apple.DiskImageMounter",
    "com.apple.archiveutility",
    // 描述文件 / 证书 / 钥匙串：打开即安装或导入信任
    "com.apple.systempreferences",
    "com.apple.mcx.ProfileHelper",
    "com.apple.keychainaccess",
    "com.apple.CertificateAssistant",
];

/// 「其余」类最多列几个（访达式，长尾留给访达；默认应用总在内）。ticket 33 L3（2026-10-25 批次审查，#32 修复项
/// 「system-list cap/order」）由 12 调到 20——12 个在装了多套办公 / 图像软件的机器上把 Apple 自带的查看器截掉了；菜单超高滚动。
pub const MAX_SYSTEM_APPS: usize = 20;

/// 目标分类（[`classify`]）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TargetClass {
    /// 目录 / 项目根。
    Folder,
    /// 文本、代码、脚本、可执行：白名单编辑器。
    Text,
    /// 其余：系统登记列表（默认在前）。
    Other,
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
    /// 系统对该文件的默认打开应用（「其余」类排第一）；取不到 `None`。
    fn default_handler(&self, target: &Path) -> Option<InstalledApp>;
    /// 文件的 UTI；系统给不出 → `None`（按「其余」类走系统列表）。
    fn type_of(&self, target: &Path) -> Option<String>;
    /// UTI 符合关系（`ty` 是否符合 `to`，含自身）。
    fn conforms(&self, ty: &str, to: &str) -> bool;
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
    /// 系统默认打开应用（只在「其余」类标；界面在名后加「（默认）」，#32）。
    pub is_default: bool,
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
    // ticket 33 S10：判定只会给越界 / 缺失两种，穷尽映射（原先借 RevealError，要把 RevealLaunch 硬映成「缺失」）。
    resolve_in_root(root, rel).map_err(|e| match e {
        InRootError::Outside(p) => OpenWithError::OpenWithOutsideProject(p),
        InRootError::Missing(p) => OpenWithError::OpenWithMissing(p),
    })
}

fn by_name(a: &InstalledApp, b: &InstalledApp) -> std::cmp::Ordering {
    a.name
        .to_lowercase()
        .cmp(&b.name.to_lowercase())
        .then_with(|| a.bundle_id.cmp(&b.bundle_id))
}

/// 目标分类：普通目录（`public.folder` / `public.directory`）→ Folder；UTI 符合文本 / 可执行类 → Text；
/// 其余 → Other；系统给不出 UTI / `dyn.*`（无登记的动态类型）时扩展名在 [`TEXT_EXTS`] → Text，否则 Other。
/// 文档包（`.key` / `.pages` / `.numbers` / `.rtfd`……）在磁盘上是目录，但 UTI 符合 `com.apple.package`——
/// 系统当一个文件，按 Other 列 Keynote / Pages（只看 `is_dir` 会给 pdf 一样的 Keynote 稿列一排 IDE）；
/// 应用包同时符合可执行类，仍走白名单目录口径。
pub fn classify(catalog: &dyn AppCatalog, target: &Path) -> TargetClass {
    let ty = catalog.type_of(target).filter(|t| !t.starts_with("dyn."));
    let is = |to: &str| ty.as_deref().is_some_and(|t| catalog.conforms(t, to));
    let ext_text = || {
        target
            .extension()
            .and_then(|e| e.to_str())
            .is_some_and(|e| TEXT_EXTS.contains(&e.to_ascii_lowercase().as_str()))
    };
    let editor_only =
        TEXT_TYPES.iter().chain(EXEC_TYPES).any(|to| is(to)) || (ty.is_none() && ext_text());
    if target.is_dir() {
        if is("com.apple.package") && !editor_only {
            return TargetClass::Other;
        }
        return TargetClass::Folder;
    }
    if editor_only {
        TargetClass::Text
    } else {
        TargetClass::Other
    }
}

/// 一行候选 + 是否系统默认。
type Eligible = (InstalledApp, bool);

/// 按分类算候选。纯判定，不写诊断；`open_with` 启动前用同一函数重算。
fn eligible(catalog: &dyn AppCatalog, target: &Path) -> Vec<Eligible> {
    match classify(catalog, target) {
        TargetClass::Other => system_list(catalog, target),
        class => editor_list(catalog, target, class == TargetClass::Folder),
    }
}

/// 白名单 ∩ 系统登记（目录另补已装且 `opens_dirs`），按显示名排序（同名按 bundle id）。
/// `is_dir` 取分类结果（Folder），不是 `Path::is_dir`——文档包已在 [`classify`] 分去 Other。
fn editor_list(catalog: &dyn AppCatalog, target: &Path, is_dir: bool) -> Vec<Eligible> {
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
    out.sort_by(by_name);
    out.into_iter().map(|a| (a, false)).collect()
}

/// Apple 系统应用（`com.apple.*`）排在第三方之前（ticket 33 L3：默认 → Apple 系统应用 → 其余按名）。
fn is_apple(a: &InstalledApp) -> bool {
    a.bundle_id.starts_with("com.apple.")
}

/// 「其余」类：系统登记列表去重、剔启动器，默认应用排第一，再 Apple 系统应用、其余各按名，截到 [`MAX_SYSTEM_APPS`]。
/// 默认应用只在它也出现在系统登记列表里时才算（不变量：列出的每一项都是系统对这个文件报过的）。
fn system_list(catalog: &dyn AppCatalog, target: &Path) -> Vec<Eligible> {
    let mut seen = HashSet::new();
    let mut apps: Vec<InstalledApp> = catalog
        .handlers(target)
        .into_iter()
        .filter(|a| !LAUNCHER_DENY.contains(&a.bundle_id.as_str()))
        .filter(|a| seen.insert(a.bundle_id.clone()))
        .collect();
    apps.sort_by(|a, b| is_apple(b).cmp(&is_apple(a)).then_with(|| by_name(a, b)));
    let default_id = catalog.default_handler(target).map(|a| a.bundle_id);
    let mut out: Vec<Eligible> = Vec::with_capacity(apps.len());
    if let Some(i) = default_id
        .as_deref()
        .and_then(|id| apps.iter().position(|a| a.bundle_id == id))
    {
        out.push((apps.remove(i), true));
    }
    out.extend(apps.into_iter().map(|a| (a, false)));
    out.truncate(MAX_SYSTEM_APPS);
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
        .map(|(app, is_default)| OpenWithApp {
            icon: catalog.icon_data_url(&app),
            id: app.bundle_id,
            name: app.name,
            is_default,
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
            .map(|(a, _)| a)
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

/// 启动页最近项目的根（#32，2026-10-21 负责人裁决「启动页最近项目右键加打开方式，路径校验以该项目根为界」）：
/// `dir` 必须逐字等于 app 自己的最近记录（壳层读 `recent_projects.json` 传入 `known`）里的一条，根取**记录里那一条**，
/// 不取界面传来的字符串；之后走与项目内同一套根约束（`rel` 恒为空串 = 项目根，只列能开目录的应用）。
/// 被否决：接受任意绝对路径再判「是目录」——启动页没打开项目，没有根可依，等于把启动面放宽到全盘。
fn recent_root<'a>(known: &'a [PathBuf], dir: &str) -> Option<&'a Path> {
    known
        .iter()
        .find(|k| k.as_os_str() == std::ffi::OsStr::new(dir))
        .map(PathBuf::as_path)
}

fn not_recent(branch: &str, dir: &str, started: std::time::Instant) -> OpenWithError {
    let e = OpenWithError::OpenWithOutsideProject(dir.to_string());
    note(
        crate::diag::CLASS_REJECT,
        true,
        branch,
        &crate::errcode::variant_code(&e),
        started,
    );
    e
}

/// 启动页最近项目的「打开方式」候选（项目级，目录口径）。不在最近记录里 → 拒（Warn，分支 `open_with_recent_candidates`）。
pub fn recent_candidates(
    catalog: &dyn AppCatalog,
    known: &[PathBuf],
    dir: &str,
) -> Result<Vec<OpenWithApp>, OpenWithError> {
    let started = std::time::Instant::now();
    match recent_root(known, dir) {
        Some(root) => candidates(catalog, root, ""),
        None => Err(not_recent("open_with_recent_candidates", dir, started)),
    }
}

/// 用 `app_id` 打开最近项目 `dir`：先认最近记录，再走 [`open_with`] 的重算候选闸。
pub fn recent_open_with(
    catalog: &dyn AppCatalog,
    known: &[PathBuf],
    dir: &str,
    app_id: &str,
) -> Result<(), OpenWithError> {
    let started = std::time::Instant::now();
    match recent_root(known, dir) {
        Some(root) => open_with(catalog, root, "", app_id),
        None => Err(not_recent("open_with_recent", dir, started)),
    }
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
    fn default_handler(&self, _: &Path) -> Option<InstalledApp> {
        None
    }
    fn type_of(&self, _: &Path) -> Option<String> {
        None
    }
    fn conforms(&self, _: &str, _: &str) -> bool {
        false
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

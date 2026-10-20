//! 系统文件管理器揭示（owner-followups-1015 #07，A16b「在访达中显示」）。
//! 入参必须是 `hexagon_core::reveal::reveal_target` 判过的规范化绝对路径——
//! 这里不再判根，只按平台调系统：
//! - macOS：`open -R`（访达里选中该项）
//! - Windows：`explorer /select,<path>`（explorer 成功也常返回 1，不看退出码，只看能否启动）
//! - 其他：打开所在目录（各家文件管理器没有统一的「选中」入口；被否决：走 D-Bus
//!   `org.freedesktop.FileManager1.ShowItems`——要引 dbus 依赖，且不是所有桌面都实现）
//!
//! 参数逐个传给进程、不经 shell：路径里的空格 / 引号 / `;` 不会被解释。
use hexagon_core::reveal::RevealError;
use std::path::Path;

pub fn show_in_file_manager(target: &Path) -> Result<(), RevealError> {
    launch(target).map_err(|e| RevealError::RevealLaunch(e.to_string()))
}

#[cfg(target_os = "macos")]
fn launch(target: &Path) -> std::io::Result<()> {
    let status = std::process::Command::new("open")
        .arg("-R")
        .arg(target)
        .status()?;
    if status.success() {
        Ok(())
    } else {
        Err(std::io::Error::other(format!(
            "open -R exited with {status}"
        )))
    }
}

#[cfg(target_os = "windows")]
fn launch(target: &Path) -> std::io::Result<()> {
    // canonicalize 在 Windows 给出 `\\?\C:\…` 逐字前缀，explorer 不认，去掉再传。
    let shown = target.to_string_lossy();
    let plain = shown.strip_prefix(r"\\?\").unwrap_or(&shown);
    std::process::Command::new("explorer")
        .arg(format!("/select,{plain}"))
        .spawn()
        .map(|_| ())
}

#[cfg(not(any(target_os = "macos", target_os = "windows")))]
fn launch(target: &Path) -> std::io::Result<()> {
    let dir = target.parent().unwrap_or(target);
    open::that(dir)
}

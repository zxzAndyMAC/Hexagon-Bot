//! Computer Use platform readiness, owner decision 2026-10-01 / desktop ticket 07.
//! System grants are separate from project approvals; readiness never authorizes an action.
use serde::{Deserialize, Serialize};
use std::path::Path;

pub mod actions;
pub mod preview;
pub use actions::{DesktopControl, DesktopScreenshot, DesktopStatus};

#[derive(Debug, Clone, Serialize, Deserialize, ts_rs::TS)]
#[ts(export, export_to = "../../../ui/src/gen/")]
pub struct DesktopPermissions {
    pub host_name: String,
    pub supported: bool,
    pub available: bool,
    pub accessibility: bool,
    pub screen_recording: bool,
    pub input_events: bool,
    pub ready: bool,
    pub error: Option<String>,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, ts_rs::TS)]
#[serde(rename_all = "snake_case")]
#[ts(export, export_to = "../../../ui/src/gen/")]
pub enum DesktopPermission {
    Accessibility,
    ScreenRecording,
}

pub fn settings_url(permission: DesktopPermission) -> &'static str {
    match permission {
        DesktopPermission::Accessibility => {
            "x-apple.systempreferences:com.apple.settings.PrivacySecurity.extension?Privacy_Accessibility"
        }
        DesktopPermission::ScreenRecording => {
            "x-apple.systempreferences:com.apple.settings.PrivacySecurity.extension?Privacy_ScreenCapture"
        }
    }
}

pub(crate) fn decode(result: Result<u32, String>, supported: bool) -> DesktopPermissions {
    // Owner 2026-10-01: a broken component is NOT a missing grant. Reject unknown
    // protocol bits. False negative costs a recheck; false positive may dispatch
    // an operation without usable system grants, so uncertainty fails closed.
    let bits = result.and_then(|bits| {
        if bits & !7 == 0x1000 {
            Ok(bits)
        } else {
            Err("incompatible computer-use component".into())
        }
    });
    let value = bits.as_ref().copied().unwrap_or(0);
    DesktopPermissions {
        host_name: std::env::current_exe()
            .ok()
            .as_deref()
            .map(host_name)
            .unwrap_or_else(|| "Hexagon".into()),
        supported,
        available: supported && bits.is_ok(),
        accessibility: supported && value & 1 != 0,
        screen_recording: supported && value & 2 != 0,
        input_events: supported && value & 4 != 0,
        ready: supported && bits.is_ok() && value & 7 == 7,
        error: bits.err(),
    }
}

fn host_name(executable: &Path) -> String {
    executable
        .ancestors()
        .find(|path| path.extension().is_some_and(|ext| ext == "app"))
        .and_then(Path::file_stem)
        .or_else(|| executable.file_stem())
        .map(|name| name.to_string_lossy().into_owned())
        .unwrap_or_else(|| "Hexagon".into())
}

pub fn permissions(library: &Path) -> DesktopPermissions {
    let started = std::time::Instant::now();
    let status = decode(native_permissions(library), cfg!(target_os = "macos"));
    crate::diag::host(
        "computer_use",
        if status.ready {
            "permissions_ready"
        } else if status.available {
            "permissions_missing"
        } else {
            "component_unavailable"
        },
        started,
    );
    status
}

#[cfg(not(target_os = "macos"))]
fn native_permissions(_: &Path) -> Result<u32, String> {
    Err("macOS is required".into())
}

#[cfg(target_os = "macos")]
static LIBRARY: std::sync::OnceLock<std::sync::Mutex<Option<libloading::Library>>> =
    std::sync::OnceLock::new();

#[cfg(target_os = "macos")]
fn native_permissions(path: &Path) -> Result<u32, String> {
    use std::sync::Mutex;
    let mut library = LIBRARY
        .get_or_init(|| Mutex::new(None))
        .lock()
        .map_err(|_| "computer-use component unavailable")?;
    // Only the shell supplies the bundled absolute path, never model or IPC input.
    // Keep Swift loaded for the process lifetime: unloading registered Swift types
    // is unsafe. A failed load is retriable after repairing the component.
    if library.is_none() {
        if !path.is_absolute() {
            return Err("component path must be absolute".into());
        }
        *library = Some(unsafe { libloading::Library::new(path) }.map_err(|e| e.to_string())?);
    }
    let library = library.as_ref().expect("loaded above");
    // ABI is fixed by native/computer-use/Permissions.swift: no pointers or allocation
    // cross this boundary. The Symbol cannot outlive the retained Library.
    unsafe {
        let status: libloading::Symbol<unsafe extern "C" fn() -> u32> = library
            .get(b"hexagon_computer_permissions_v1\0")
            .map_err(|e| e.to_string())?;
        Ok(status())
    }
}

/// Explicit owner interaction only; does not set or remember project approval.
pub fn request_permission(path: &Path, permission: DesktopPermission) -> Result<(), String> {
    native_permissions(path)?;
    #[cfg(target_os = "macos")]
    {
        // Owner ticket 07: AppKit permission prompts can reenter the event loop.
        // Never hold the loader mutex across a foreign call; the static library
        // remains loaded, so copying its function pointer preserves its lifetime.
        let request = {
            let guard = LIBRARY
                .get()
                .ok_or("component not loaded")?
                .lock()
                .map_err(|_| "component unavailable")?;
            let library = guard.as_ref().ok_or("component not loaded")?;
            unsafe {
                *library
                    .get::<unsafe extern "C" fn(u32)>(b"hexagon_computer_request_v1\0")
                    .map_err(|e| e.to_string())?
            }
        };
        unsafe {
            request(match permission {
                DesktopPermission::Accessibility => 0,
                DesktopPermission::ScreenRecording => 1,
            });
        }
        Ok(())
    }
    #[cfg(not(target_os = "macos"))]
    {
        let _ = permission;
        Err("macOS is required".into())
    }
}

#[cfg(test)]
#[test]
fn permission_host_name_matches_the_running_bundle() {
    assert_eq!(
        host_name(Path::new(
            "/Applications/HexagonLive.app/Contents/MacOS/hexagon"
        )),
        "HexagonLive"
    );
    assert_eq!(host_name(Path::new("/tmp/hexagon")), "hexagon");
}

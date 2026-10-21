//! macOS 系统目录（#31）：NSWorkspace / LaunchServices 回答「谁能打开」「装在哪」，
//! NSWorkspace 图标画进 32×32 位图转 PNG；启动走 `/usr/bin/open -a <系统给的应用包> <路径>`，
//! 参数逐个交给进程、不经 shell。
//!
//! 被否决：`NSWorkspace openURLs:withApplicationAtURL:configuration:completionHandler:`——
//! 要引 block2 且结果异步回调，壳层同步命令拿不到失败；`open -a` 同样走 LaunchServices，
//! 同步返回退出码。图标走 `TIFFRepresentation`——会把 1024px 全套表示编进去再取第一张，慢且大。
use super::{AppCatalog, InstalledApp};
use base64::Engine;
use objc2::rc::Retained;
use objc2::AnyThread;
use objc2_app_kit::{
    NSBitmapImageFileType, NSBitmapImageRep, NSCompositingOperation, NSDeviceRGBColorSpace,
    NSGraphicsContext, NSImage, NSWorkspace,
};
use objc2_foundation::{
    NSBundle, NSDictionary, NSFileManager, NSPoint, NSRect, NSSize, NSString, NSURL,
};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::time::SystemTime;

/// 菜单行图标 16pt @2x。
const ICON_PX: isize = 32;

/// 图标缓存键 = 应用包位置 + 包修改时间：应用升级 / 重装换了图标，mtime 变即重取；
/// 候选列表本身每次现查系统，不缓存（装 / 卸应用下次打开菜单就反映）。
type IconCache = HashMap<(PathBuf, Option<SystemTime>), Option<String>>;

#[derive(Default)]
pub struct MacCatalog {
    icons: Mutex<IconCache>,
}

fn app_from_url(url: &NSURL) -> Option<InstalledApp> {
    let path = url.to_file_path()?;
    let bundle = NSBundle::bundleWithURL(url)?;
    let id = bundle.bundleIdentifier()?.to_string();
    let name = NSFileManager::defaultManager()
        .displayNameAtPath(&NSString::from_str(&path.to_string_lossy()))
        .to_string();
    let name = name.strip_suffix(".app").unwrap_or(&name).to_string();
    Some(InstalledApp {
        bundle_id: id,
        name,
        app_path: path,
    })
}

fn icon_png(app_path: &Path) -> Option<Vec<u8>> {
    let image: Retained<NSImage> = NSWorkspace::sharedWorkspace()
        .iconForFile(&NSString::from_str(&app_path.to_string_lossy()));
    // SAFETY: planes = null 让 AppKit 自己分配缓冲；其余参数是 32×32 RGBA8 的合法组合。
    let rep = unsafe {
        NSBitmapImageRep::initWithBitmapDataPlanes_pixelsWide_pixelsHigh_bitsPerSample_samplesPerPixel_hasAlpha_isPlanar_colorSpaceName_bytesPerRow_bitsPerPixel(
            NSBitmapImageRep::alloc(),
            std::ptr::null_mut(),
            ICON_PX,
            ICON_PX,
            8,
            4,
            true,
            false,
            NSDeviceRGBColorSpace,
            0,
            0,
        )
    }?;
    let ctx = NSGraphicsContext::graphicsContextWithBitmapImageRep(&rep)?;
    NSGraphicsContext::saveGraphicsState_class();
    NSGraphicsContext::setCurrentContext(Some(&ctx));
    let side = ICON_PX as f64;
    let rect = NSRect::new(NSPoint::new(0.0, 0.0), NSSize::new(side, side));
    image.drawInRect_fromRect_operation_fraction(
        rect,
        NSRect::ZERO,
        NSCompositingOperation::Copy,
        1.0,
    );
    ctx.flushGraphics();
    NSGraphicsContext::restoreGraphicsState_class();
    // SAFETY: 空属性字典对任何文件类型都合法。
    let data = unsafe {
        rep.representationUsingType_properties(NSBitmapImageFileType::PNG, &NSDictionary::new())
    }?;
    Some(data.to_vec())
}

impl AppCatalog for MacCatalog {
    fn handlers(&self, target: &Path) -> Vec<InstalledApp> {
        let Some(url) = NSURL::from_file_path(target) else {
            return Vec::new();
        };
        NSWorkspace::sharedWorkspace()
            .URLsForApplicationsToOpenURL(&url)
            .iter()
            .filter_map(|u| app_from_url(&u))
            .collect()
    }

    fn installed(&self, bundle_id: &str) -> Option<InstalledApp> {
        let url = NSWorkspace::sharedWorkspace()
            .URLForApplicationWithBundleIdentifier(&NSString::from_str(bundle_id))?;
        app_from_url(&url)
    }

    fn icon_data_url(&self, app: &InstalledApp) -> Option<String> {
        let mtime = std::fs::metadata(&app.app_path)
            .and_then(|m| m.modified())
            .ok();
        let key = (app.app_path.clone(), mtime);
        if let Some(hit) = self.icons.lock().ok()?.get(&key) {
            return hit.clone();
        }
        let url = icon_png(&app.app_path).map(|png| {
            format!(
                "data:image/png;base64,{}",
                base64::engine::general_purpose::STANDARD.encode(png)
            )
        });
        if let Ok(mut cache) = self.icons.lock() {
            cache.insert(key, url.clone());
        }
        url
    }

    fn launch(&self, app: &InstalledApp, target: &Path) -> Result<(), String> {
        let status = std::process::Command::new("/usr/bin/open")
            .arg("-a")
            .arg(&app.app_path)
            .arg(target)
            .status()
            .map_err(|e| e.to_string())?;
        if status.success() {
            Ok(())
        } else {
            Err(format!("open -a exited with {status}"))
        }
    }
}

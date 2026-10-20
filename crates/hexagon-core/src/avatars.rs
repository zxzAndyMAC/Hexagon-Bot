//! 头像存储（owner-followups-1015 票 17，规范 components E4「头像」，2026-10-17 负责人裁决）。
//!
//! 两层：**角色头像**（一个角色设一次，所有实例生效）+ **实例覆盖**（Agent 详情里单独换图）。
//! 读取优先级：实例覆盖 > 角色头像 > 默认（UI 画色块首字，本模块回 None）。
//! 「恢复默认」= 删除对应层级的自定义图（实例层删了会露出角色层）。
//!
//! 落盘布局（纯文件，ADR 0052：读写只要 repo_root，不占 wb 锁）：
//! - 实例：`.hexagon/avatars/<agent_id>.<ext>`——与票 17 之前按实例存的路径**完全一致**，
//!   所以升级前的实例头像不做迁移、天然成为实例覆盖。被否决：把旧实例图搬成角色图
//!   （同角色多实例各有图时没法决定谁赢，且改变了负责人已看到的结果）。
//! - 角色：`.hexagon/avatars/roles/<key>.<ext>`，key = sha256(角色名) 前 32 位十六进制。
//!   角色名是任意 Unicode（「前端」「QA / 测试」），直接当文件名会撞路径分隔符与大小写不敏感
//!   文件系统；哈希定长且不可能穿目录。被否决：百分号转义（长名超 255 字节）。
//!
//! 写入时服务端规整（规范 E4：不做裁剪器）：解码后 >2MB 拒；非正方形按中心裁方并重编码 PNG；
//! 已是正方形保留原字节与格式（GIF 动图不被压成首帧）。UI 仍本地预检 2MB，省一次 IPC。
//! 解不开的字节一律拒（fail-closed）：曾经只认 data URL 包装、任意字节都落盘（票 17 之前的
//! 测试就是写 `fakepng`），坏图只会在界面上静默回退首字，负责人不知道为什么没换成。

use base64::Engine;
use serde::Deserialize;
use std::path::{Path, PathBuf};

use crate::db::Db;

/// 头像上限（解码后字节）。规范 E4「>2MB 拒」。
pub const MAX_AVATAR_BYTES: usize = 2 * 1024 * 1024;

const EXTS: [(&str, &str); 4] = [
    ("png", "image/png"),
    ("jpg", "image/jpeg"),
    ("webp", "image/webp"),
    ("gif", "image/gif"),
];

#[derive(Debug, thiserror::Error)]
pub enum AvatarError {
    #[error("头像超过 2MB")]
    AvatarTooLarge,
    #[error("头像不是可识别的图片: {0}")]
    AvatarUnreadable(String),
    #[error("非法的 Agent id: {0}")]
    BadAgentId(String),
    #[error(transparent)]
    Io(#[from] std::io::Error),
    #[error("sqlite: {0}")]
    Sqlite(#[from] rusqlite::Error),
}

/// 规整后的头像：可直接落盘的字节 + 扩展名。
#[derive(Debug, Clone)]
pub struct Normalized {
    bytes: Vec<u8>,
    ext: &'static str,
}

/// 向导建项目时随单下发的角色头像（向导时项目还不存在，图先留在界面内存）。
#[derive(Debug, Clone, Deserialize, serde::Serialize, ts_rs::TS)]
#[ts(export, export_to = "../../../ui/src/gen/")]
pub struct RoleAvatar {
    pub role: String,
    pub data_url: String,
}

/// data URL → 规整字节。2MB、可解码、中心裁方三道闸都在这里。
pub fn normalize(data_url: &str) -> Result<Normalized, AvatarError> {
    let (_mime, b64) = data_url
        .strip_prefix("data:")
        .and_then(|s| s.split_once(";base64,"))
        .ok_or_else(|| {
            AvatarError::AvatarUnreadable("expected data:<mime>;base64,<payload>".into())
        })?;
    // base64 解码前先按长度粗拦：4 字符 ≈ 3 字节，免得把 200MB 的串先解进内存。
    if b64.len() / 4 * 3 > MAX_AVATAR_BYTES + 3 {
        return Err(AvatarError::AvatarTooLarge);
    }
    let bytes = base64::engine::general_purpose::STANDARD
        .decode(b64.trim())
        .map_err(|e| AvatarError::AvatarUnreadable(e.to_string()))?;
    if bytes.len() > MAX_AVATAR_BYTES {
        return Err(AvatarError::AvatarTooLarge);
    }
    // 格式按内容嗅探，不信 data URL 声称的 mime（文件名改个后缀就能骗过）。
    let format =
        image::guess_format(&bytes).map_err(|e| AvatarError::AvatarUnreadable(e.to_string()))?;
    let ext = match format {
        image::ImageFormat::Png => "png",
        image::ImageFormat::Jpeg => "jpg",
        image::ImageFormat::WebP => "webp",
        image::ImageFormat::Gif => "gif",
        other => return Err(AvatarError::AvatarUnreadable(format!("{other:?}"))),
    };
    let img = image::load_from_memory_with_format(&bytes, format)
        .map_err(|e| AvatarError::AvatarUnreadable(e.to_string()))?;
    let (w, h) = (img.width(), img.height());
    if w == 0 || h == 0 {
        return Err(AvatarError::AvatarUnreadable("empty image".into()));
    }
    if w == h {
        return Ok(Normalized { bytes, ext });
    }
    let side = w.min(h);
    let cropped = img.crop_imm((w - side) / 2, (h - side) / 2, side, side);
    let mut out = std::io::Cursor::new(Vec::new());
    cropped
        .write_to(&mut out, image::ImageFormat::Png)
        .map_err(|e| AvatarError::AvatarUnreadable(e.to_string()))?;
    Ok(Normalized {
        bytes: out.into_inner(),
        ext: "png",
    })
}

fn avatars_dir(root: &Path) -> PathBuf {
    root.join(".hexagon/avatars")
}

fn roles_dir(root: &Path) -> PathBuf {
    avatars_dir(root).join("roles")
}

fn role_key(role: &str) -> String {
    use sha2::Digest;
    let d = sha2::Sha256::digest(role.as_bytes());
    d.iter().take(16).map(|b| format!("{b:02x}")).collect()
}

/// agent id 进文件名：只许 `[A-Za-z0-9_-]`，挡 `../` 之类穿目录（id 由核生成，形如 `a0`）。
fn check_agent_id(agent_id: &str) -> Result<(), AvatarError> {
    if agent_id.is_empty()
        || !agent_id
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-')
    {
        return Err(AvatarError::BadAgentId(agent_id.into()));
    }
    Ok(())
}

/// 找某层的已存文件（stem 不含扩展名）。
fn find(dir: &Path, stem: &str) -> Result<Option<(PathBuf, &'static str)>, AvatarError> {
    for (ext, mime) in EXTS {
        let p = dir.join(format!("{stem}.{ext}"));
        if p.exists() {
            crate::db::validate_generic_file_access(&p)?;
            return Ok(Some((p, mime)));
        }
    }
    Ok(None)
}

fn read_url(hit: Option<(PathBuf, &'static str)>) -> Result<Option<String>, AvatarError> {
    let Some((p, mime)) = hit else {
        return Ok(None);
    };
    let b = std::fs::read(&p)?;
    Ok(Some(format!(
        "data:{mime};base64,{}",
        base64::engine::general_purpose::STANDARD.encode(b)
    )))
}

fn write(dir: &Path, stem: &str, n: &Normalized) -> Result<(), AvatarError> {
    std::fs::create_dir_all(dir)?;
    clear(dir, stem)?;
    std::fs::write(dir.join(format!("{stem}.{}", n.ext)), &n.bytes)?;
    Ok(())
}

/// 删该层全部扩展名（换格式时清旧文件，也是「恢复默认」）。不存在不算错。
fn clear(dir: &Path, stem: &str) -> Result<(), AvatarError> {
    for (ext, _) in EXTS {
        let p = dir.join(format!("{stem}.{ext}"));
        if p.exists() {
            std::fs::remove_file(p)?;
        }
    }
    Ok(())
}

// ---- 实例层 ----

/// 只读实例覆盖（不回落角色层）。
pub fn instance_avatar(root: &Path, agent_id: &str) -> Result<Option<String>, AvatarError> {
    check_agent_id(agent_id)?;
    read_url(find(&avatars_dir(root), agent_id)?)
}

pub fn set_agent_avatar(root: &Path, agent_id: &str, data_url: &str) -> Result<(), AvatarError> {
    check_agent_id(agent_id)?;
    let n = normalize(data_url)?;
    write(&avatars_dir(root), agent_id, &n)
}

/// 实例层「恢复默认」：删覆盖，露出角色头像（没有则色块首字）。
pub fn clear_agent_avatar(root: &Path, agent_id: &str) -> Result<(), AvatarError> {
    check_agent_id(agent_id)?;
    clear(&avatars_dir(root), agent_id)
}

// ---- 角色层 ----

pub fn role_avatar(root: &Path, role: &str) -> Result<Option<String>, AvatarError> {
    read_url(find(&roles_dir(root), &role_key(role))?)
}

pub fn set_role_avatar(root: &Path, role: &str, data_url: &str) -> Result<(), AvatarError> {
    let n = normalize(data_url)?;
    write_role(root, role, &n)
}

/// 已规整的角色图落盘（向导建项目：先在建目录前规整全部，建好后再写）。
pub fn write_role(root: &Path, role: &str, n: &Normalized) -> Result<(), AvatarError> {
    write(&roles_dir(root), &role_key(role), n)
}

pub fn clear_role_avatar(root: &Path, role: &str) -> Result<(), AvatarError> {
    clear(&roles_dir(root), &role_key(role))
}

/// 向导下发的一批角色头像：全部先规整，任何一张不合格整单拒（建项目之前调用，目录还没动）。
pub fn prepare_role_avatars(list: &[RoleAvatar]) -> Result<Vec<(String, Normalized)>, AvatarError> {
    list.iter()
        .map(|a| Ok((a.role.clone(), normalize(&a.data_url)?)))
        .collect()
}

// ---- 两层读取 ----

/// 生效头像来源：读取优先级的唯一实现，团队行哈希与 data URL 读都走它。
fn effective(
    root: &Path,
    agent_id: &str,
    role: Option<&str>,
) -> Result<Option<(PathBuf, &'static str)>, AvatarError> {
    check_agent_id(agent_id)?;
    if let Some(hit) = find(&avatars_dir(root), agent_id)? {
        return Ok(Some(hit));
    }
    match role {
        Some(r) => find(&roles_dir(root), &role_key(r)),
        None => Ok(None),
    }
}

/// 某 Agent 的生效头像 data URL（实例覆盖 > 角色头像 > None）。角色从 agents 表取。
pub fn agent_avatar(db: &Db, root: &Path, agent_id: &str) -> Result<Option<String>, AvatarError> {
    let role = agent_role(db, agent_id)?;
    read_url(effective(root, agent_id, role.as_deref())?)
}

fn agent_role(db: &Db, agent_id: &str) -> Result<Option<String>, AvatarError> {
    use rusqlite::OptionalExtension;
    Ok(db
        .conn()
        .query_row("SELECT role FROM agents WHERE id=?1", [agent_id], |r| {
            r.get(0)
        })
        .optional()?)
}

/// 团队行用：生效头像的内容哈希 + 是否有实例覆盖。
/// 哈希混入来源层：角色图与实例图字节相同也算变化（「恢复默认」后界面要刷新）。
/// DefaultHasher::new() 定值（非 RandomState），同工具链内可复现（arch-review 票 07）。
pub fn avatar_state(
    root: &Path,
    agent_id: &str,
    role: &str,
) -> Result<(Option<String>, bool), AvatarError> {
    use std::hash::{Hash, Hasher};
    check_agent_id(agent_id)?;
    let override_hit = find(&avatars_dir(root), agent_id)?.is_some();
    let Some((p, _)) = effective(root, agent_id, Some(role))? else {
        return Ok((None, false));
    };
    let b = std::fs::read(&p)?;
    let mut h = std::collections::hash_map::DefaultHasher::new();
    override_hit.hash(&mut h);
    b.hash(&mut h);
    Ok((Some(format!("{:016x}", h.finish())), override_hit))
}

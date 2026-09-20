//! 流程包编辑（US11/US12）：预置包复制 → 草稿编辑 → 保存/模板化/YAML 导出。
//!
//! 核心分离（守住既有钉住机制）：
//! - `.hexagon/pack.json`        = 可编辑草稿（下次开跑生效）
//! - `.hexagon/pack.active.json` = 运行中实例钉住的只读快照
//!
//! 编辑永远写 pack.json，绝不碰 pack.active.json——运行中实例已被钉版本隔离。
//! 保存仅在经过 `presets::validate_pack`（拒绝零阶段/重复阶段/幽灵角色引用）后发生。

use crate::db::Db;
use crate::orchestra::PackDef;
use serde_json::Value;
use std::path::{Path, PathBuf};

#[derive(Debug, thiserror::Error)]
pub enum PackEditError {
    #[error(transparent)]
    Validate(#[from] crate::presets::PresetError),
    #[error("no pack draft found")]
    NoDraft,
    #[error("io: {0}")]
    Io(#[from] std::io::Error),
    #[error("json: {0}")]
    Json(#[from] serde_json::Error),
    #[error("sqlite: {0}")]
    Sqlite(#[from] rusqlite::Error),
    #[error(transparent)]
    Db(#[from] crate::db::DbError),
    #[error(transparent)]
    Orch(#[from] crate::orchestra::OrchError),
}

fn draft_path(repo_root: &Path) -> PathBuf {
    repo_root.join(".hexagon/pack.json")
}

/// 读编辑草稿：`.hexagon/pack.json` 优先，没有则以钉住副本为复制源。
pub fn load_draft(repo_root: &Path) -> Result<PackDef, PackEditError> {
    let draft = draft_path(repo_root);
    if draft.exists() {
        return Ok(PackDef::load(&draft)?);
    }
    let pinned = repo_root.join(".hexagon/pack.active.json");
    if pinned.exists() {
        return Ok(PackDef::load(&pinned)?);
    }
    Err(PackEditError::NoDraft)
}

/// 存草稿：按团队名册校验后写 pack.json。
/// 不碰 pack.active.json（运行中实例已隔离）。
pub fn save_draft(
    db: &Db,
    repo_root: &Path,
    project_id: &str,
    pack: &PackDef,
) -> Result<(), PackEditError> {
    let known = crate::roles::team_roles(db, project_id).map_err(|e| {
        PackEditError::Sqlite(match e {
            crate::roles::RoleError::Sqlite(inner) => inner,
            _ => rusqlite::Error::InvalidQuery,
        })
    })?;
    crate::presets::validate_pack(pack, &known)?;
    let dir = repo_root.join(".hexagon");
    std::fs::create_dir_all(&dir)?;
    std::fs::write(draft_path(repo_root), serde_json::to_string_pretty(pack)?)?;
    Ok(())
}

/// 用户模板目录：~/.config/hexagon/templates/（没有则 ~/.hexagon/templates/）
/// 测试可用 HEXAGON_TEMPLATES_DIR 覆盖。
fn templates_dir() -> PathBuf {
    if let Ok(d) = std::env::var("HEXAGON_TEMPLATES_DIR") {
        return PathBuf::from(d);
    }
    let home = std::env::var("HOME")
        .or_else(|_| std::env::var("USERPROFILE"))
        .unwrap_or_else(|_| ".".into());
    let cfg = Path::new(&home).join(".config/hexagon/templates");
    if cfg.exists() {
        cfg
    } else {
        Path::new(&home).join(".hexagon/templates")
    }
}

/// 存为个人模板（前提是已校验草稿；保险起见由调用方复检）。
pub fn save_template(pack: &PackDef) -> Result<PathBuf, PackEditError> {
    let dir = templates_dir();
    std::fs::create_dir_all(&dir)?;
    let path = dir.join(format!("{}.json", pack.name.replace('/', "-")));
    std::fs::write(&path, serde_json::to_string_pretty(pack)?)?;
    Ok(path)
}

/// 已存个人模板名列表。
pub fn list_templates() -> Result<Vec<String>, PackEditError> {
    let dir = templates_dir();
    if !dir.is_dir() {
        return Ok(vec![]);
    }
    let mut names = Vec::new();
    for e in std::fs::read_dir(&dir)? {
        let e = e?;
        if e.path().extension().and_then(|x| x.to_str()) == Some("json") {
            if let Ok(pack) = PackDef::load(e.path()) {
                names.push(pack.name);
            }
        }
    }
    names.sort();
    Ok(names)
}

/// PackDef → YAML（手写发射器：形状固定，不为它引入 serde_yaml 依赖）。
pub fn to_yaml(pack: &PackDef) -> String {
    let mut y = String::new();
    let list = |v: &[String]| -> String {
        if v.is_empty() {
            "[]".into()
        } else {
            format!("[{}]", v.join(", "))
        }
    };
    y.push_str(&format!("name: {}\nversion: {}\n", pack.name, pack.version));
    // 策略旋钮（票 04）：非默认才渲染,缺席=内核默认;
    // 与 stages 的流程定义分行,读文件即见「策略面」。
    let k = &pack.knobs;
    if k.judge.is_some()
        || k.flag_patience.is_some()
        || k.auto_backfill.is_some()
        || k.consult_auto_wake.is_some()
    {
        y.push_str("knobs:\n");
        if let Some(j) = &k.judge {
            y.push_str(&format!("  judge: {j}\n"));
        }
        if let Some(v) = k.flag_patience {
            y.push_str(&format!("  flag_patience: {v}\n"));
        }
        if let Some(v) = k.auto_backfill {
            y.push_str(&format!("  auto_backfill: {v}\n"));
        }
        if let Some(v) = k.consult_auto_wake {
            y.push_str(&format!("  consult_auto_wake: {v}\n"));
        }
    }
    y.push_str("stages:\n");
    for s in &pack.stages {
        y.push_str(&format!("  - name: {}\n", s.name));
        y.push_str(&format!("    roles: {}\n", list(&s.roles)));
        y.push_str(&format!("    due: {}\n", list(&s.due)));
        if !s.checks.is_empty() {
            y.push_str(&format!("    checks: {}\n", list(&s.checks)));
        }
        if !s.reviews.is_empty() {
            y.push_str("    reviews:\n");
            for r in &s.reviews {
                y.push_str(&format!(
                    "      - artifact_kind: {}\n        reviewer: {}\n",
                    r.artifact_kind, r.reviewer
                ));
            }
        }
        if s.stamp_point {
            y.push_str("    stamp_point: true\n");
        }
        if !s.backfill_edges.is_empty() {
            y.push_str("    backfill_edges:\n");
            for (f, t) in &s.backfill_edges {
                y.push_str(&format!("      - [{f}, {t}]\n"));
            }
        }
        if !s.consult_wake.is_empty() {
            y.push_str(&format!("    consult_wake: {}\n", list(&s.consult_wake)));
        }
    }
    y
}

/// YAML 导出：把草稿（没有则钉住副本）写到指定位置。
pub fn export_yaml(repo_root: &Path, dest: &Path) -> Result<(), PackEditError> {
    let pack = load_draft(repo_root)?;
    if let Some(parent) = dest.parent() {
        std::fs::create_dir_all(parent)?;
    }
    std::fs::write(dest, to_yaml(&pack))?;
    Ok(())
}

/// JSON 字符串 → PackDef（UI JSON 编辑面的解析+校验入口）。
pub fn parse_draft(json_text: &str) -> Result<PackDef, PackEditError> {
    Ok(serde_json::from_str::<PackDef>(json_text)?)
}

/// 草稿摘要（UI 展示用）：阶段名列表与主要统计。
pub fn draft_summary(pack: &PackDef) -> Value {
    serde_json::json!({
        "name": pack.name,
        "version": pack.version,
        "stage_count": pack.stages.len(),
        "knobs": pack.knobs,
        "stages": pack.stages.iter().map(|s| serde_json::json!({
            "name": s.name, "roles": s.roles, "due": s.due,
            "checks": s.checks, "stamp_point": s.stamp_point,
            "reviews": s.reviews, "consult_wake": s.consult_wake,
            "backfill_edges": s.backfill_edges,
        })).collect::<Vec<_>>(),
    })
}

//! 流程包編集（US11/US12）：預置包の複製 → ドラフト編集 → 保存/テンプレ化/YAML エクスポート。
//!
//! 核心の分離（既存の釘付け機構を守る）：
//! - `.hexagon/pack.json`      = 編集可能なドラフト（次回開跑に効く）
//! - `.hexagon/pack.active.json` = 走行中インスタンスが釘付けした読み取り専用スナップ
//!
//! 編集は必ず pack.json に書き、pack.active.json には絶対触れない
//! —— 走行中インスタンスは钉版本で隔離済み。
//! 保存は `presets::validate_pack`（零阶段/重複阶段/幽灵角色参照を拒否）通過時のみ。

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

/// 編集用ドラフトを読む：`.hexagon/pack.json` 優先、無ければ钉住副本を複製源に。
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

/// ドラフト保存：チーム名簿で検証してから pack.json に書く。
/// pack.active.json には触れない（走行中インスタンス隔離）。
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

/// ユーザーテンプレート置き場：~/.config/hexagon/templates/（無ければ ~/.hexagon/templates/）
/// テストでは HEXAGON_TEMPLATES_DIR で差し替え可能。
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

/// 個人テンプレートとして保存（検証済みドラフト前提だが念のため再検証は呼び出し側）。
pub fn save_template(pack: &PackDef) -> Result<PathBuf, PackEditError> {
    let dir = templates_dir();
    std::fs::create_dir_all(&dir)?;
    let path = dir.join(format!("{}.json", pack.name.replace('/', "-")));
    std::fs::write(&path, serde_json::to_string_pretty(pack)?)?;
    Ok(path)
}

/// 保存済み個人テンプレート名一覧。
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

/// PackDef → YAML（手書きエミッタ：固定形なので serde_yaml 依存を足さない）。
pub fn to_yaml(pack: &PackDef) -> String {
    let mut y = String::new();
    let list = |v: &[String]| -> String {
        if v.is_empty() {
            "[]".into()
        } else {
            format!("[{}]", v.join(", "))
        }
    };
    y.push_str(&format!(
        "name: {}\nversion: {}\nstages:\n",
        pack.name, pack.version
    ));
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

/// YAML エクスポート：ドラフト（無ければ钉住副本）を指定先に書く。
pub fn export_yaml(repo_root: &Path, dest: &Path) -> Result<(), PackEditError> {
    let pack = load_draft(repo_root)?;
    if let Some(parent) = dest.parent() {
        std::fs::create_dir_all(parent)?;
    }
    std::fs::write(dest, to_yaml(&pack))?;
    Ok(())
}

/// JSON 文字列 → PackDef（UI の JSON 編集面用のパース+検証入口）。
pub fn parse_draft(json_text: &str) -> Result<PackDef, PackEditError> {
    Ok(serde_json::from_str::<PackDef>(json_text)?)
}

/// ドラフトのサマリ（UI 表示用）：段名リストと主要統計。
pub fn draft_summary(pack: &PackDef) -> Value {
    serde_json::json!({
        "name": pack.name,
        "version": pack.version,
        "stage_count": pack.stages.len(),
        "stages": pack.stages.iter().map(|s| serde_json::json!({
            "name": s.name, "roles": s.roles, "due": s.due,
            "checks": s.checks, "stamp_point": s.stamp_point,
            "reviews": s.reviews, "consult_wake": s.consult_wake,
            "backfill_edges": s.backfill_edges,
        })).collect::<Vec<_>>(),
    })
}

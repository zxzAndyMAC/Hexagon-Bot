//! 全局角色模板库（ADR 0057）：`~/.hexagon/roles.json` 存用户自定义 RoleDef，
//! 与内置预置并列成「角色模板」目录——同名自定义覆盖内置（就近优先）。
//!
//! - 模板只携带定义（职责/上级/模型槽/globs/技能名），权限规则、授权清单、
//!   密钥不进模板；建项目时按名**复制**物化为项目 Agent，不回写。
//! - 读失败按空表处理（损坏文件不应让设置页/向导整体打不开——读写不对称：
//!   读 fail-open，写 fail-closed）。写走 tmp+rename 原子落盘（同 provider_config）。
//! - 校验只做定义级：名非空、不自审、上级在合并目录内。技能名不校验——
//!   技能集是动态两层（全局∪项目），此刻校验会误伤合法引用。

use crate::presets::{preset_roles, PresetError, RoleDef};
use serde::Serialize;
use std::collections::HashSet;
use std::path::{Path, PathBuf};

#[derive(Debug, thiserror::Error)]
pub enum TemplateError {
    #[error(transparent)]
    Io(#[from] std::io::Error),
    #[error(transparent)]
    Json(#[from] serde_json::Error),
    #[error(transparent)]
    Preset(#[from] PresetError),
    #[error("角色模板名不能为空")]
    EmptyName,
    #[error("角色模板不能自审: {0}")]
    SelfReviewer(String),
    #[error("角色模板「{role}」的上级「{reviewer}」不在模板目录内")]
    UnknownReviewer { role: String, reviewer: String },
    #[error("无法定位 HOME 目录")]
    NoHome,
}

/// 一条模板 = 定义 + 来源。`origin`: "builtin" | "custom"（与 SkillRow.origin 同款字符串）。
#[derive(Debug, Clone, Serialize, ts_rs::TS)]
#[ts(export, export_to = "../../../ui/src/gen/")]
pub struct RoleTemplate {
    pub def: RoleDef,
    pub origin: String,
}

fn global_roles_path() -> Result<PathBuf, TemplateError> {
    if let Ok(p) = std::env::var("HEXAGON_ROLES_PATH") {
        return Ok(PathBuf::from(p));
    }
    std::env::var_os("HOME")
        .map(|h| Path::new(&h).join(".hexagon/roles.json"))
        .ok_or(TemplateError::NoHome)
}

/// 自定义层原文（读失败=空表+warn；写路径才 fail-closed）。
fn load_custom_at(path: &Path) -> Vec<RoleDef> {
    match std::fs::read_to_string(path) {
        Ok(t) => serde_json::from_str(&t).unwrap_or_else(|e| {
            log::warn!("{path:?} 损坏，自定义角色模板按空表处理: {e}");
            Vec::new()
        }),
        Err(_) => Vec::new(),
    }
}

/// 合并目录：同名自定义覆盖内置，其余内置按序在前、自定义在后。
pub fn merge_templates(builtin: Vec<RoleDef>, custom: Vec<RoleDef>) -> Vec<RoleTemplate> {
    let custom_names: HashSet<&str> = custom.iter().map(|d| d.name.as_str()).collect();
    let mut out: Vec<RoleTemplate> = builtin
        .into_iter()
        .filter(|d| !custom_names.contains(d.name.as_str()))
        .map(|def| RoleTemplate {
            def,
            origin: "builtin".into(),
        })
        .collect();
    out.extend(custom.into_iter().map(|def| RoleTemplate {
        def,
        origin: "custom".into(),
    }));
    out
}

/// 全量模板目录（内置∪自定义）。
pub fn role_templates() -> Result<Vec<RoleTemplate>, TemplateError> {
    let custom = global_roles_path()
        .map(|p| load_custom_at(&p))
        .unwrap_or_default();
    Ok(merge_templates(preset_roles()?, custom))
}

/// 保存前的定义级校验。`catalog_names` = 保存后目录内可用的角色名全集
/// （被保存项自身也算——但自审单独拒）。
fn validate_def(def: &RoleDef, catalog_names: &HashSet<&str>) -> Result<(), TemplateError> {
    let name = def.name.trim();
    if name.is_empty() {
        return Err(TemplateError::EmptyName);
    }
    if def.reviewer.as_deref() == Some(name) {
        return Err(TemplateError::SelfReviewer(def.name.clone()));
    }
    if let Some(rev) = &def.reviewer {
        if !catalog_names.contains(rev.as_str()) {
            return Err(TemplateError::UnknownReviewer {
                role: def.name.clone(),
                reviewer: rev.clone(),
            });
        }
    }
    Ok(())
}

fn save_at(path: &Path, def: &RoleDef) -> Result<(), TemplateError> {
    let builtin = preset_roles()?;
    let mut custom = load_custom_at(path);
    // 目录名全集 = 内置 ∪ 现存自定义 ∪ 本项（替换同名旧项）
    let names: HashSet<&str> = builtin
        .iter()
        .map(|d| d.name.as_str())
        .chain(custom.iter().map(|d| d.name.as_str()))
        .chain(std::iter::once(def.name.as_str()))
        .collect();
    validate_def(def, &names)?;
    custom.retain(|d| d.name != def.name);
    custom.push(def.clone());
    write_at(path, &custom)
}

fn delete_at(path: &Path, name: &str) -> Result<(), TemplateError> {
    let mut custom = load_custom_at(path);
    custom.retain(|d| d.name != name);
    write_at(path, &custom)
}

fn write_at(path: &Path, custom: &[RoleDef]) -> Result<(), TemplateError> {
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir)?;
    }
    // tmp+rename：半截写不留下损坏目录文件（同 provider_config::write_doc）。
    let tmp = path.with_extension("tmp");
    std::fs::write(&tmp, serde_json::to_string_pretty(custom)?)?;
    std::fs::rename(&tmp, path)?;
    Ok(())
}

pub fn save_role_template(def: &RoleDef) -> Result<(), TemplateError> {
    save_at(&global_roles_path()?, def)
}

/// 只删自定义层；同名覆盖内置时删除后内置复活（就近优先的自然结果）。
pub fn delete_role_template(name: &str) -> Result<(), TemplateError> {
    delete_at(&global_roles_path()?, name)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn def(name: &str, reviewer: Option<&str>) -> RoleDef {
        RoleDef {
            name: name.into(),
            duty: "d".into(),
            reviewer: reviewer.map(String::from),
            model_slot: "chat".into(),
            globs: vec![],
            skills: vec![],
        }
    }

    #[test]
    fn custom_overrides_builtin_same_name() {
        let custom = vec![def("产品策划", None)];
        let merged = merge_templates(preset_roles().unwrap(), custom);
        let t = merged.iter().find(|t| t.def.name == "产品策划").unwrap();
        assert_eq!(t.origin, "custom");
        assert_eq!(t.def.duty, "d");
        assert_eq!(merged.len(), 12); // 覆盖不增项；12 = 预置角色数（含项目经理，票 08）
    }

    #[test]
    fn save_upsert_delete_roundtrip() {
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().join("roles.json");
        save_at(&p, &def("自建A", Some("产品策划"))).unwrap();
        save_at(&p, &def("自建A", None)).unwrap(); // upsert
        let custom = load_custom_at(&p);
        assert_eq!(custom.len(), 1);
        assert_eq!(custom[0].reviewer, None);
        // 删后文件仍在（空表），目录回落纯内置
        delete_at(&p, "自建A").unwrap();
        assert!(load_custom_at(&p).is_empty());
        assert_eq!(role_templates_at_len(&p), 12);
    }

    fn role_templates_at_len(path: &Path) -> usize {
        merge_templates(preset_roles().unwrap(), load_custom_at(path)).len()
    }

    #[test]
    fn corrupted_file_reads_as_empty() {
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().join("roles.json");
        std::fs::write(&p, "{not json").unwrap();
        assert!(load_custom_at(&p).is_empty());
    }

    #[test]
    fn validation_rejects_bad_defs() {
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().join("roles.json");
        assert!(matches!(
            save_at(&p, &def("  ", None)),
            Err(TemplateError::EmptyName)
        ));
        assert!(matches!(
            save_at(&p, &def("自审", Some("自审"))),
            Err(TemplateError::SelfReviewer(_))
        ));
        assert!(matches!(
            save_at(&p, &def("x", Some("幽灵"))),
            Err(TemplateError::UnknownReviewer { .. })
        ));
    }

    #[test]
    fn custom_reviewer_can_be_custom_role() {
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().join("roles.json");
        save_at(&p, &def("上级甲", None)).unwrap();
        save_at(&p, &def("下级乙", Some("上级甲"))).unwrap();
        assert_eq!(load_custom_at(&p).len(), 2);
    }
}

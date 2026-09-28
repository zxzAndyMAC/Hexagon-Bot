//! Governance 14: explicit empty-list creation binds file and author grant.
use super::{
    gate,
    governance::{hash, target_path},
    ExperienceRequest, ExperienceTarget,
};
use crate::{db::Db, proposals::PropError, tools::ToolContext};
use rusqlite::{params, OptionalExtension};
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub(super) struct RoleSkill {
    pub role: String,
    pub skill: String,
    pub new_file: bool,
}

pub(super) fn prospective(ctx: &ToolContext, skill: &str) -> Result<PathBuf, PropError> {
    crate::skills::validate_name(skill).map_err(|e| PropError::Rejected(e.to_string()))?;
    let relative = format!(".hexagon/skills/{skill}/SKILL.md");
    let resolved = crate::tools::repo_path(&ctx.repo_root, &relative)
        .map_err(|e| PropError::Rejected(e.to_string()))?;
    let mut part = ctx.repo_root.clone();
    for segment in relative.split('/') {
        part.push(segment);
        if let Ok(metadata) = part.symlink_metadata() {
            if metadata.file_type().is_symlink() {
                return Err(PropError::UngovernedExperience);
            }
        }
    }
    Ok(resolved)
}
pub(super) fn prepare(
    db: &Db,
    ctx: &ToolContext,
    request: &mut ExperienceRequest,
) -> Result<Option<RoleSkill>, PropError> {
    if !request.create_role_skill {
        return Ok(None);
    }
    if !request.targets.is_empty() {
        return Err(PropError::Rejected(
            "role creation cannot carry alternate targets".into(),
        ));
    }
    let current = gate(db, ctx, request.review_event)?;
    if !current.skills.is_empty() {
        return Err(PropError::StaleExperience);
    }
    let skill = crate::skills::experience_dir_name(&current.role);
    let path = prospective(ctx, &skill)?;
    let registered: Option<String> = db
        .conn()
        .query_row(
            "SELECT skill FROM experience_role_skills WHERE project_id=?1 AND role=?2",
            params![ctx.project_id, current.role],
            |r| r.get(0),
        )
        .optional()?;
    let new_file = registered.is_none();
    let digest = if let Some(registered) = registered {
        if registered != skill {
            return Err(PropError::StaleExperience);
        }
        hash(&std::fs::read_to_string(target_path(ctx, &skill)?)?)
    } else {
        if path.parent().is_some_and(|p| p.exists())
            || crate::skills::list_global().iter().any(|s| s.name == skill)
        {
            return Err(PropError::Rejected(
                "role skill target conflicts with an existing skill or directory".into(),
            ));
        }
        "absent".into()
    };
    request.targets.push(ExperienceTarget {
        skill: skill.clone(),
        change: None,
        expected_digest: digest,
        reason: format!("Reviewed role experience for {}", current.role),
    });
    Ok(Some(RoleSkill {
        role: current.role,
        skill,
        new_file,
    }))
}
pub(super) fn revalidate(db: &Db, ctx: &ToolContext, role: &RoleSkill) -> Result<(), PropError> {
    let current = gate(db, ctx, None)?;
    if current.role != role.role || !current.skills.is_empty() {
        return Err(PropError::StaleExperience);
    }
    Ok(())
}
pub(super) fn commit(db: &Db, ctx: &ToolContext, role: &RoleSkill) -> Result<(), PropError> {
    revalidate(db, ctx, role)?;
    db.conn().execute(
        "INSERT OR IGNORE INTO experience_role_skills(project_id,role,skill) VALUES(?1,?2,?3)",
        params![ctx.project_id, role.role, role.skill],
    )?;
    crate::grants::grant_reviewed_experience(db, &ctx.project_id, &ctx.agent_id, &role.skill)
        .map_err(|e| PropError::Rejected(e.to_string()))
}

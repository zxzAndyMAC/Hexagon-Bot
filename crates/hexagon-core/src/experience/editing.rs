//! Governance 08: owner project edits retain exact text and version preconditions.
use super::governance::{hash, target_path};
use crate::{db::Db, proposals::PropError, tools::ToolContext};
use serde::{Deserialize, Serialize};
use std::io::Write;

#[derive(Debug, Clone, Serialize, Deserialize, ts_rs::TS)]
#[serde(deny_unknown_fields)]
#[ts(export, export_to = "../../../ui/src/gen/")]
pub struct ProjectSkillDocument {
    pub project_root: String,
    pub skill: String,
    pub digest: String,
    pub content: String,
}
pub fn read(db: &Db, ctx: &ToolContext, skill: &str) -> Result<ProjectSkillDocument, PropError> {
    let content = std::fs::read_to_string(target_path(ctx, skill)?)?;
    // Reading the editor is also an observation of the real experience bytes.
    super::entries(db, ctx, skill)?;
    Ok(ProjectSkillDocument {
        project_root: ctx.repo_root.canonicalize()?.to_string_lossy().into_owned(),
        skill: skill.into(),
        digest: hash(&content),
        content,
    })
}
pub fn save(
    db: &Db,
    ctx: &ToolContext,
    document: &ProjectSkillDocument,
) -> Result<ProjectSkillDocument, PropError> {
    let started = std::time::Instant::now();
    let result = save_inner(db, ctx, document);
    crate::diag::note(
        if result.is_ok() {
            crate::diag::CLASS_JUDGE
        } else {
            crate::diag::CLASS_REJECT
        },
        result.is_err(),
        Some(&ctx.project_id),
        Some(&ctx.agent_id),
        None,
        None,
        "project_skill_edit",
        if result.is_ok() {
            "saved"
        } else {
            "conflict_or_forbidden"
        },
        started,
    );
    result
}
fn save_inner(
    db: &Db,
    ctx: &ToolContext,
    document: &ProjectSkillDocument,
) -> Result<ProjectSkillDocument, PropError> {
    if ctx.agent_id != "owner"
        || document.project_root != ctx.repo_root.canonicalize()?.to_string_lossy()
    {
        return Err(PropError::Rejected(
            "project skill editor identity changed".into(),
        ));
    }
    let _lease = if ctx.write_lease.is_none() {
        Some(crate::tools::writeguard::repository_lock(ctx)?)
    } else {
        None
    };
    let path = target_path(ctx, &document.skill)?;
    let before = std::fs::read_to_string(&path)?;
    if hash(&before) != document.digest {
        return Err(PropError::StaleExperience);
    }
    // Observe old bytes before an edit can mask a previously changed receipt.
    super::entries(db, ctx, &document.skill)?;
    let mut temp = tempfile::NamedTempFile::new_in(path.parent().expect("skill file parent"))?;
    temp.as_file()
        .set_permissions(path.metadata()?.permissions())?;
    temp.write_all(document.content.as_bytes())?;
    temp.as_file().sync_all()?;
    if std::fs::read_to_string(&path)? != before {
        return Err(PropError::StaleExperience);
    }
    temp.persist(&path).map_err(|e| PropError::Io(e.error))?;
    read(db, ctx, &document.skill)
}

//! Governance 02: explicit targets and host-derived source bindings.
//! False rejection costs another review; a false positive writes unreviewed
//! policy content. A model's payload is never a host approval receipt.

use super::{gate, Qualification};
use crate::{db::Db, proposals::PropError, tools::ToolContext};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::path::PathBuf;

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize, ts_rs::TS)]
#[serde(deny_unknown_fields)]
#[ts(export, export_to = "../../../ui/src/gen/")]
pub struct ExperienceConditions {
    pub roles: Vec<String>,
    pub stages: Vec<String>,
    pub paths: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, ts_rs::TS)]
#[serde(rename_all = "snake_case")]
#[ts(export, export_to = "../../../ui/src/gen/")]
pub enum ExperienceChangeKind {
    Revise,
    Replace,
    Reactivate,
}

#[derive(Debug, Clone, Serialize, Deserialize, ts_rs::TS)]
#[serde(deny_unknown_fields)]
#[ts(export, export_to = "../../../ui/src/gen/")]
pub struct ExperienceChange {
    pub kind: ExperienceChangeKind,
    pub entry_id: String,
    pub expected_revision: u32,
    pub reason: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, ts_rs::TS)]
#[serde(deny_unknown_fields)]
#[ts(export, export_to = "../../../ui/src/gen/")]
pub struct ExperienceTarget {
    pub skill: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub change: Option<ExperienceChange>,
    pub expected_digest: String,
    pub reason: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, ts_rs::TS)]
#[serde(deny_unknown_fields)]
#[ts(export, export_to = "../../../ui/src/gen/")]
pub struct ExperienceRequest {
    #[serde(default)]
    pub create_role_skill: bool,
    pub body: String,
    pub notes: String,
    pub conditions: ExperienceConditions,
    pub targets: Vec<ExperienceTarget>,
    #[ts(type = "number | null")]
    pub review_event: Option<i64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct Prepared {
    pub schema_version: u32,
    pub request: ExperienceRequest,
    pub qualification: Qualification,
    #[serde(default)]
    pub previous_entries: Vec<super::ExperienceEntry>,
    #[serde(default)]
    pub legacy: Option<super::LegacyRange>,
    #[serde(default)]
    pub role_skill: Option<super::role_skill::RoleSkill>,
}

pub(super) fn hash(text: &str) -> String {
    format!("{:x}", Sha256::digest(text.as_bytes()))
}

fn invalid(message: &str) -> PropError {
    PropError::Rejected(message.into())
}

pub(super) fn validate_request(
    request: &ExperienceRequest,
    entry_limit: u32,
) -> Result<(), PropError> {
    if request.body.trim().is_empty() {
        return Err(invalid("experience body is empty"));
    }
    let mut names = std::collections::HashSet::new();
    if request
        .targets
        .iter()
        .any(|target| !names.insert(&target.skill))
    {
        return Err(invalid("experience targets must be distinct"));
    }
    for path in &request.conditions.paths {
        if !super::matching::normalized_path(path) {
            return Err(invalid(
                "experience condition requires a normalized relative path prefix",
            ));
        }
    }
    if request
        .conditions
        .roles
        .iter()
        .chain(&request.conditions.stages)
        .any(|s| s.trim().is_empty())
    {
        return Err(invalid("experience condition identity is empty"));
    }
    if super::limits::entry_size(&request.body, &request.notes, &request.conditions)
        > entry_limit as usize
    {
        return Err(invalid(
            "experience entry exceeds the configured character limit",
        ));
    }
    Ok(())
}

pub(super) fn target_path(ctx: &ToolContext, name: &str) -> Result<PathBuf, PropError> {
    let name = crate::skills::validate_name(name).map_err(|e| invalid(&e.to_string()))?;
    let relative = format!(".hexagon/skills/{name}/SKILL.md");
    let resolved =
        crate::tools::repo_path(&ctx.repo_root, &relative).map_err(|e| invalid(&e.to_string()))?;
    // Governance 02: reject aliases even inside the repository; otherwise a
    // skill write can modify a protected peer file through a hard/symbolic link.
    let mut part = ctx.repo_root.clone();
    for segment in relative.split('/') {
        part.push(segment);
        if part.symlink_metadata()?.file_type().is_symlink() {
            return Err(invalid(
                "experience skill path must not contain symbolic links",
            ));
        }
    }
    let metadata = resolved.metadata()?;
    if !metadata.is_file() {
        return Err(invalid("experience target is not a project skill file"));
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        if metadata.nlink() != 1 {
            return Err(invalid("experience target must not be a hard-link alias"));
        }
    }
    Ok(resolved)
}

pub(super) fn propose(
    db: &Db,
    ctx: &ToolContext,
    request: &ExperienceRequest,
) -> Result<String, PropError> {
    let started = std::time::Instant::now();
    let result = propose_inner(db, ctx, request, None);
    crate::diag::note(
        if result.is_ok() {
            crate::diag::CLASS_JUDGE
        } else {
            crate::diag::CLASS_REJECT
        },
        result.is_err(),
        Some(&ctx.project_id),
        Some(&ctx.agent_id),
        ctx.stage_run_id.as_deref(),
        None,
        "experience_propose",
        if result.is_ok() {
            "source_and_target_bound"
        } else {
            "invalid_source_or_target"
        },
        started,
    );
    result
}

pub(super) fn propose_inner(
    db: &Db,
    ctx: &ToolContext,
    request: &ExperienceRequest,
    legacy: Option<super::LegacyRange>,
) -> Result<String, PropError> {
    let mut normalized = request.clone();
    let role_skill = super::role_skill::prepare(db, ctx, &mut normalized)?;
    let request = &normalized;
    validate_request(
        request,
        super::limits::read(db, &ctx.project_id)?.entry_chars,
    )?;
    let gate = gate(db, ctx, request.review_event)?;
    let qualification = if legacy.is_some() {
        super::curation::historical(
            db,
            ctx,
            request
                .review_event
                .ok_or(PropError::UnreviewedExperience)?,
        )?
    } else {
        gate.qualification.ok_or(PropError::UnreviewedExperience)?
    };
    if qualification.evidence.author != ctx.agent_id {
        return Err(PropError::UnreviewedExperience);
    }
    if legacy.is_none() && gate.delivered {
        return Err(PropError::FrozenExperience);
    }
    let mut previous_entries = Vec::new();
    for target in &request.targets {
        if target.reason.trim().is_empty()
            || (role_skill.is_none() && !gate.skills.contains(&target.skill))
        {
            return Err(invalid(
                "experience target needs an authorized skill and relevance reason",
            ));
        }
        if let Some(change) = &target.change {
            if change.entry_id.is_empty()
                || change.expected_revision == 0
                || change.reason.trim().is_empty()
            {
                return Err(invalid(
                    "experience change requires an identity, revision and reason",
                ));
            }
            let entry = super::entries(db, ctx, &target.skill)?
                .into_iter()
                .find(|v| v.entry.entry_id == change.entry_id)
                .ok_or(PropError::StaleExperience)?;
            if entry.entry.revision != change.expected_revision || entry.recovery_pending {
                return Err(PropError::StaleExperience);
            }
            previous_entries.push(entry.entry);
        }
        if role_skill.as_ref().is_some_and(|role| role.new_file) {
            continue;
        }
        let path = target_path(ctx, &target.skill)?;
        if crate::tools::writeguard::digest(&path)?.as_deref()
            != Some(target.expected_digest.as_str())
        {
            return Err(invalid(
                "experience target version changed; reread and resubmit",
            ));
        }
    }
    let prepared = Prepared {
        schema_version: 1,
        request: request.clone(),
        qualification,
        previous_entries,
        legacy,
        role_skill,
    };
    let payload = serde_json::to_string_pretty(&prepared)
        .map_err(|e| invalid(&e.to_string()))?
        .replace('`', "\\u0060");
    // Prefixing diff lines keeps lesson Markdown from creating extra fences.
    // The host stores the exact prepared payload independently of this artifact.
    let diff = payload
        .lines()
        .map(|line| format!("+ {line}\n"))
        .collect::<String>();
    let target = request
        .targets
        .first()
        .map(|t| format!(".hexagon/skills/{}/SKILL.md", t.skill))
        .unwrap_or_else(|| ".hexagon/skills/".into());
    let content = format!("---\nkind: 改进提案\nauthor: {}\nsurface: skill\ntarget: {target}\n---\n\n## 动机\n经验条目及适用条件复审\n\n## 改动面\n```diff\n{diff}```\n\n## 预期收益\n仅在适用工作中复用已复审经验\n\n## 验证方法\n核对来源、条件与技能版本\n\n```experience\n{payload}\n```\n",ctx.agent_id);
    let path = format!("props/experience-entry-{}.md", db.next_id("exp-proposal")?);
    let artifact =
        crate::artifacts::deliver(db, ctx, &ctx.tiers, &path, &content, Some("改进提案"))
            .map_err(|e| invalid(&e.to_string()))?;
    let proposal = crate::proposals::submit(db, ctx, &artifact, &content)?;
    db.conn().execute(
        "INSERT INTO experience_proposals(project_id,proposal_id,payload_json,artifact_digest) VALUES(?1,?2,?3,?4)",
        rusqlite::params![ctx.project_id,proposal,payload,hash(&content)],
    )?;
    Ok(proposal)
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, ts_rs::TS)]
#[ts(export, export_to = "../../../ui/src/gen/")]
pub struct ExperienceSource {
    #[ts(type = "number")]
    pub review_event: i64,
    #[ts(type = "number")]
    pub activation: i64,
    pub artifact_id: String,
    pub author: String,
    #[ts(type = "number")]
    pub artifact_version: i64,
    pub artifact_digest: String,
}

#[derive(Debug, Clone, Serialize, ts_rs::TS)]
#[ts(export, export_to = "../../../ui/src/gen/")]
pub struct ExperienceProposalView {
    pub proposal_id: String,
    pub request: ExperienceRequest,
    pub source: ExperienceSource,
    pub previous_entries: Vec<super::ExperienceEntry>,
    pub legacy: Option<super::LegacyRange>,
    pub recovery_pending: bool,
}

pub fn proposal_view(
    db: &Db,
    project: &str,
    proposal: &str,
) -> Result<Option<ExperienceProposalView>, PropError> {
    use rusqlite::OptionalExtension;
    let payload: Option<String> = db
        .conn()
        .query_row(
            "SELECT payload_json FROM experience_proposals WHERE project_id=?1 AND proposal_id=?2",
            rusqlite::params![project, proposal],
            |r| r.get(0),
        )
        .optional()?;
    let Some(payload) = payload else {
        return Ok(None);
    };
    let p: Prepared = serde_json::from_str(&payload).map_err(|e| invalid(&e.to_string()))?;
    let recovery_pending:bool=db.conn().query_row("SELECT EXISTS(SELECT 1 FROM experience_operations o JOIN proposals p ON p.id=o.proposal_id WHERE o.project_id=?1 AND o.proposal_id=?2 AND (o.state!='complete' OR p.status NOT IN ('active','rolled_back')))",rusqlite::params![project,proposal],|r|r.get(0))?;
    Ok(Some(ExperienceProposalView {
        recovery_pending,
        previous_entries: p.previous_entries,
        legacy: p.legacy,
        proposal_id: proposal.into(),
        request: p.request,
        source: ExperienceSource {
            review_event: p.qualification.review_event,
            activation: p.qualification.activation,
            artifact_id: p.qualification.evidence.id,
            author: p.qualification.evidence.author,
            artifact_version: p.qualification.evidence.version,
            artifact_digest: p.qualification.evidence.digest,
        },
    }))
}

#[derive(Debug, Clone, Serialize, ts_rs::TS)]
#[ts(export, export_to = "../../../ui/src/gen/")]
pub struct ExperienceSubmission {
    pub proposal_id: String,
}

/// Governance 02 review regression: approval must describe the bytes actually
/// submitted, not a temporary edited proposal that is reverted before apply.
pub(crate) fn validate_review(
    db: &Db,
    project: &str,
    proposal: &str,
    body: &str,
) -> Result<Option<String>, PropError> {
    use rusqlite::OptionalExtension;
    let digest: Option<String> = db.conn().query_row("SELECT artifact_digest FROM experience_proposals WHERE project_id=?1 AND proposal_id=?2",rusqlite::params![project,proposal],|r|r.get(0)).optional()?;
    if digest
        .as_ref()
        .is_some_and(|expected| *expected != hash(body))
    {
        return Err(PropError::StaleExperience);
    }
    Ok(digest)
}

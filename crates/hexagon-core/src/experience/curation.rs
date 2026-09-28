//! Governance 13: historical proof is distinct from a current activation grant.
use super::{
    governance::{hash, target_path},
    ExperienceRequest, Qualification,
};
use crate::{db::Db, proposals::PropError, tools::ToolContext};
use rusqlite::params;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, ts_rs::TS)]
#[serde(deny_unknown_fields)]
#[ts(export, export_to = "../../../ui/src/gen/")]
pub struct LegacyRange {
    pub skill: String,
    pub start_byte: u32,
    pub end_byte: u32,
    pub digest: String,
}
#[derive(Debug, Clone, Serialize, Deserialize, ts_rs::TS)]
#[serde(deny_unknown_fields)]
#[ts(export, export_to = "../../../ui/src/gen/")]
pub struct ExperienceCuration {
    pub project_root: String,
    pub request: ExperienceRequest,
    pub legacy: LegacyRange,
    #[ts(type = "number")]
    pub review_event: i64,
}

pub(super) fn validate_range(ctx: &ToolContext, range: &LegacyRange) -> Result<(), PropError> {
    let text = std::fs::read_to_string(target_path(ctx, &range.skill)?)?;
    let start = range.start_byte as usize;
    let end = range.end_byte as usize;
    let selected = text
        .get(start..end)
        .filter(|s| !s.trim().is_empty())
        .ok_or(PropError::StaleExperience)?;
    // Whole-line bounds ensure partial Markdown headings cannot masquerade as
    // hidden legacy text. Managed blocks are not legacy curation candidates.
    if (start > 0 && text.as_bytes().get(start - 1) != Some(&b'\n'))
        || (end < text.len() && text.as_bytes().get(end - 1) != Some(&b'\n'))
        || selected.contains("<!-- hexagon-")
        || hash(selected) != range.digest
        || super::legacy_loading_view(&text[..start]).1 == 0
        || super::legacy_loading_view(&text[..start]).0
            != super::legacy_loading_view(&text[..end]).0
    {
        return Err(PropError::StaleExperience);
    }
    Ok(())
}

pub(super) fn historical(
    db: &Db,
    ctx: &ToolContext,
    event: i64,
) -> Result<Qualification, PropError> {
    let (reviewer,raw):(String,String)=db.conn().query_row("SELECT agent_id,payload FROM events WHERE project_id=?1 AND id=?2 AND kind='review_passed'",params![ctx.project_id,event],|r|Ok((r.get(0)?,r.get(1)?))).map_err(|_|PropError::UnreviewedExperience)?;
    let payload: serde_json::Value =
        serde_json::from_str(&raw).map_err(|_| PropError::UnreviewedExperience)?;
    let evidence: crate::artifacts::evidence::ArtifactEvidence =
        serde_json::from_value(payload["evidence"].clone())
            .map_err(|_| PropError::UnreviewedExperience)?;
    if evidence.project != ctx.project_id
        || evidence.author == reviewer
        || matches!(evidence.kind.as_str(), "改进提案" | "复审意见")
    {
        return Err(PropError::UnreviewedExperience);
    }
    let snapshot = crate::artifacts::content_at(
        db,
        &ctx.repo_root,
        &ctx.project_id,
        &evidence.path,
        evidence.version,
    )
    .map_err(|_| PropError::UnreviewedExperience)?
    .ok_or(PropError::UnreviewedExperience)?;
    // A code review may bind additional source bytes. If the stored artifact
    // cannot reconstruct those bytes exactly, historical eligibility is absent.
    if hash(&snapshot) != evidence.digest
        || evidence
            .source_digest
            .as_ref()
            .is_some_and(|digest| *digest != hash(&snapshot))
    {
        return Err(PropError::UnreviewedExperience);
    }
    let valid:bool=db.conn().query_row("SELECT EXISTS(SELECT 1 FROM artifacts WHERE project_id=?1 AND id=?2 AND path=?3 AND version=?4 AND author_agent_id=?5) AND EXISTS(SELECT 1 FROM events WHERE project_id=?1 AND kind='artifact_delivered' AND agent_id=?5 AND id<?6 AND json_extract(payload,'$.artifact_id')=?2) AND NOT EXISTS(SELECT 1 FROM events WHERE project_id=?1 AND id>?6 AND kind IN ('review_passed','review_rejected') AND agent_id=?7 AND json_extract(payload,'$.artifact_id')=?2)",params![ctx.project_id,evidence.id,evidence.path,evidence.version,evidence.author,event,reviewer],|r|r.get(0))?;
    if !valid {
        return Err(PropError::UnreviewedExperience);
    }
    let activation=db.conn().query_row("SELECT COALESCE(MAX(id),0) FROM events WHERE project_id=?1 AND agent_id=?2 AND kind='agent_activated' AND id<?3",params![ctx.project_id,evidence.author,event],|r|r.get(0))?;
    Ok(Qualification {
        review_event: event,
        activation,
        evidence,
    })
}

fn propose_inner(
    db: &Db,
    ctx: &ToolContext,
    curation: &ExperienceCuration,
) -> Result<String, PropError> {
    if ctx.agent_id != "owner"
        || curation.project_root != ctx.repo_root.canonicalize()?.to_string_lossy()
        || curation.request.targets.len() != 1
        || curation.request.targets[0].skill != curation.legacy.skill
    {
        return Err(PropError::UngovernedExperience);
    }
    validate_range(ctx, &curation.legacy)?;
    let source = historical(db, ctx, curation.review_event)?;
    let mut author = ctx.clone();
    author.agent_id = source.evidence.author;
    let mut request = curation.request.clone();
    request.review_event = Some(curation.review_event);
    super::governance::propose_inner(db, &author, &request, Some(curation.legacy.clone()))
}

pub fn propose(
    db: &Db,
    ctx: &ToolContext,
    request: &ExperienceCuration,
) -> Result<String, PropError> {
    let started = std::time::Instant::now();
    let result = propose_inner(db, ctx, request);
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
        "experience_curation",
        if result.is_ok() {
            "historical_proof_bound"
        } else {
            "proof_or_range_invalid"
        },
        started,
    );
    result
}
#[cfg(test)]
mod tests {
    use super::*;
    use proptest::prelude::*;
    proptest! {
        #[test]
        fn ordinary_lines_never_pass_as_legacy(body in "[a-z]{1,40}") {
            let dir=tempfile::tempdir().unwrap();
            let db=Db::open_in_memory().unwrap();
            let ctx=ToolContext::owner(&db,dir.path());
            let parent=dir.path().join(".hexagon/skills/alpha");
            std::fs::create_dir_all(&parent).unwrap();
            let prefix="## 经验\nOlder line\n## Ordinary\n";
            let line=format!("{body}\n");
            let text=format!("{prefix}{line}");
            std::fs::write(parent.join("SKILL.md"),&text).unwrap();
            let range=LegacyRange {skill:"alpha".into(),start_byte:prefix.len() as u32,end_byte:text.len() as u32,digest:hash(&line)};
            prop_assert!(validate_range(&ctx,&range).is_err());
        }
    }
}

//! D08: evidence identifies one author's concrete deliverable, never a kind.
use crate::{
    db::Db,
    tools::{readable_repo_path, writeguard},
};
use rusqlite::OptionalExtension;
use serde::{Deserialize, Serialize};
use std::path::Path;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct ArtifactEvidence {
    pub id: String,
    pub project: String,
    pub path: String,
    pub kind: String,
    pub stage: Option<String>,
    pub author: String,
    pub version: i64,
    pub digest: String,
    pub source_digest: Option<String>,
}

pub(crate) fn runnable_source(path: &str) -> bool {
    matches!(
        Path::new(path)
            .extension()
            .and_then(|s| s.to_str())
            .unwrap_or(""),
        "html" | "css" | "js" | "mjs" | "cjs" | "ts" | "tsx" | "jsx" | "vue"
    )
}

// Unknown identity/body is not evidence: false negative costs a fresh review;
// false positive accepts unreviewed bytes. Legacy events are never backfilled.
pub(crate) fn capture(
    db: &Db,
    root: &Path,
    project: &str,
    id: &str,
) -> rusqlite::Result<Option<ArtifactEvidence>> {
    let record=db.conn().query_row(
        "SELECT path,kind,stage_run_id,author_agent_id,version FROM artifacts a WHERE a.project_id=?1 AND a.id=?2 AND a.status IN ('valid','stamped') AND NOT EXISTS(SELECT 1 FROM artifacts newer WHERE newer.project_id=a.project_id AND newer.path=a.path AND newer.version>a.version)",
        rusqlite::params![project,id], |r| Ok((r.get::<_,String>(0)?,r.get::<_,String>(1)?,r.get::<_,Option<String>>(2)?,r.get::<_,Option<String>>(3)?,r.get::<_,i64>(4)?))).optional()?;
    let Some((path, kind, stage, Some(author), version)) = record else {
        return Ok(None);
    };
    let Ok(target) =
        readable_repo_path(root, &format!(".hexagon/{}", path.trim_start_matches('/')))
    else {
        return Ok(None);
    };
    let pending:bool=db.conn().query_row("SELECT EXISTS(SELECT 1 FROM artifact_materializations WHERE project_id=?1 AND state='pending' AND (path=?2 OR json_extract(intent_json,'$.resolved')=?3))",rusqlite::params![project,path,target.to_string_lossy()],|r|r.get(0))?;
    if pending {
        return Ok(None);
    }
    let Some(digest) = writeguard::digest(&target).ok().flatten() else {
        return Ok(None);
    };
    let source_digest = if kind == "代码" || runnable_source(&path) {
        let Some(digest) = readable_repo_path(root, path.trim_start_matches('/'))
            .ok()
            .and_then(|p| writeguard::digest(&p).ok().flatten())
        else {
            return Ok(None);
        };
        Some(digest)
    } else {
        None
    };
    Ok(Some(ArtifactEvidence {
        id: id.into(),
        project: project.into(),
        path,
        kind,
        stage,
        author,
        version,
        digest,
        source_digest,
    }))
}

pub(crate) fn matches(
    recorded: Option<&ArtifactEvidence>,
    current: Option<&ArtifactEvidence>,
) -> bool {
    recorded.zip(current).is_some_and(|(a, b)| a == b)
}

pub(crate) fn review_satisfied(
    db: &Db,
    root: &Path,
    project: &str,
    run: &str,
    id: &str,
    reviewer: &str,
) -> rusqlite::Result<bool> {
    let current = capture(db, root, project, id)?;
    let event:Option<(String,String)>=db.conn().query_row("SELECT kind,payload FROM events WHERE project_id=?1 AND stage_run_id=?2 AND kind IN ('review_passed','review_rejected') AND json_extract(payload,'$.artifact_id')=?3 AND json_extract(payload,'$.reviewer')=?4 ORDER BY id DESC LIMIT 1",rusqlite::params![project,run,id,reviewer],|r|Ok((r.get(0)?,r.get(1)?))).optional()?;
    let Some((kind, payload)) = event else {
        return Ok(false);
    };
    let recorded = serde_json::from_str::<serde_json::Value>(&payload)
        .ok()
        .and_then(|p| serde_json::from_value::<ArtifactEvidence>(p["evidence"].clone()).ok());
    Ok(kind == "review_passed" && matches(recorded.as_ref(), current.as_ref()))
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub(crate) struct ReviewBinding {
    pub evidence: ArtifactEvidence,
    pub verdict: String,
    pub reviewer: String,
}

pub(crate) fn prepare_review(
    db: &Db,
    ctx: &crate::tools::ToolContext,
    meta: &super::ArtifactMeta,
) -> Result<Option<ReviewBinding>, super::ArtifactError> {
    if meta.kind != "复审意见" {
        return Ok(None);
    }
    let started = std::time::Instant::now();
    let result = (|| {
        let target = meta
            .extra
            .get("target")
            .filter(|s| !s.trim().is_empty())
            .ok_or(super::ArtifactError::MissingField("target"))?;
        let verdict = meta
            .extra
            .get("verdict")
            .filter(|v| matches!(v.as_str(), "pass" | "reject"))
            .ok_or(super::ArtifactError::MissingField("verdict: pass/reject"))?;
        let id:Option<String>=db.conn().query_row("SELECT id FROM artifacts WHERE project_id=?1 AND (id=?2 OR path=?2) ORDER BY version DESC LIMIT 1",rusqlite::params![ctx.project_id,target],|r|r.get(0)).optional()?;
        let evidence = id
            .map(|id| capture(db, &ctx.repo_root, &ctx.project_id, &id))
            .transpose()?
            .flatten()
            .ok_or(super::ArtifactError::MissingField("current review target"))?;
        // D08 review finding: capture-at-submit used to turn an old review of A
        // into evidence for B. The original read receipt must survive any wait.
        let recorded = meta
            .extra
            .get("target_evidence")
            .and_then(|raw| serde_json::from_str::<ArtifactEvidence>(raw).ok())
            .ok_or(super::ArtifactError::MissingField(
                "target_evidence from artifact_read",
            ))?;
        if !matches(Some(&recorded), Some(&evidence)) {
            return Err(super::ArtifactError::StaleReview);
        }
        let reviewer = db.conn().query_row(
            "SELECT role FROM agents WHERE project_id=?1 AND id=?2",
            rusqlite::params![ctx.project_id, ctx.agent_id],
            |r| r.get(0),
        )?;
        Ok(Some(ReviewBinding {
            evidence,
            verdict: verdict.clone(),
            reviewer,
        }))
    })();
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
        "review_target",
        if result.is_ok() {
            "bound"
        } else {
            "unavailable"
        },
        started,
    );
    result
}

#[derive(Debug, Clone, Serialize, ts_rs::TS)]
#[ts(export, export_to = "../../../ui/src/gen/")]
pub struct ArtifactReview {
    #[ts(type = "'passed' | 'rejected' | 'stale'")]
    pub status: String,
    #[ts(type = "number")]
    pub event_id: i64,
    pub reviewer: String,
    #[ts(type = "number | null")]
    pub version: Option<i64>,
    pub digest: Option<String>,
}

pub(crate) fn review_state(
    db: &Db,
    root: &Path,
    project: &str,
    row: &super::ArtifactRow,
) -> rusqlite::Result<Option<ArtifactReview>> {
    // Historical kind-only events stay visible as stale, never borrowed as
    // proof for a specific object. No migration fabricates missing evidence.
    let event:Option<(i64,String,String)>=db.conn().query_row(
        "SELECT id,kind,payload FROM events WHERE project_id=?1 AND kind IN ('review_passed','review_rejected') AND
        (json_extract(payload,'$.artifact_id')=?2 OR json_extract(payload,'$.artifact_path')=?3 OR
         (json_extract(payload,'$.artifact_id') IS NULL AND json_extract(payload,'$.artifact_kind')=?4 AND stage_run_id IS ?5))
        ORDER BY id DESC LIMIT 1",rusqlite::params![project,row.id,row.path,row.kind,row.stage_run_id],|r|Ok((r.get(0)?,r.get(1)?,r.get(2)?))).optional()?;
    let Some((event_id, kind, payload)) = event else {
        return Ok(None);
    };
    let payload = serde_json::from_str::<serde_json::Value>(&payload).unwrap_or_default();
    let recorded = serde_json::from_value::<ArtifactEvidence>(payload["evidence"].clone()).ok();
    let current = capture(db, root, project, &row.id)?;
    let status = if !matches(recorded.as_ref(), current.as_ref()) {
        "stale"
    } else if kind == "review_passed" {
        "passed"
    } else {
        "rejected"
    };
    Ok(Some(ArtifactReview {
        status: status.into(),
        event_id,
        reviewer: payload["reviewer"].as_str().unwrap_or("").into(),
        version: recorded.as_ref().map(|e| e.version),
        digest: recorded.map(|e| e.digest),
    }))
}

pub(crate) fn for_path(
    db: &Db,
    root: &Path,
    project: &str,
    path: &str,
) -> rusqlite::Result<Option<ArtifactEvidence>> {
    let id:Option<String>=db.conn().query_row("SELECT id FROM artifacts WHERE project_id=?1 AND path=?2 ORDER BY version DESC LIMIT 1",rusqlite::params![project,path],|r|r.get(0)).optional()?;
    id.map(|id| capture(db, root, project, &id))
        .transpose()
        .map(Option::flatten)
}

#[cfg(test)]
mod properties {
    use super::*;
    use proptest::prelude::*;
    proptest! {
        #[test]
        fn review_evidence_cannot_survive_changed_identity_or_bytes(value in ".{0,50}", field in 0usize..8) {
            let a=ArtifactEvidence{id:"a".into(),project:"p".into(),path:"x".into(),kind:"doc".into(),stage:Some("run".into()),author:"author".into(),version:1,digest:"original".into(),source_digest:Some("source".into())};
            let mut b=a.clone();
            let changed=format!("changed:{value}");
            match field {0=>b.id=changed,1=>b.project=changed,2=>b.path=changed,3=>b.stage=Some(changed),4=>b.author=changed,5=>b.version=2,6=>b.digest=changed,_=>b.source_digest=Some(changed)}
            prop_assert!(!matches(Some(&a),Some(&b)));
            prop_assert!(!matches(None,Some(&a)));
            prop_assert!(!matches(Some(&a),None));
            prop_assert!(matches(Some(&a),Some(&a)));
        }
    }
}

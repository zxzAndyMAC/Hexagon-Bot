//! Governance 15: project-bound, stable pages; browsing never grants eligibility.
use super::{
    governance::hash,
    storage::{all_changes, Intent},
    ExperienceEntry, ExperienceSource,
};
use crate::{db::Db, proposals::PropError, tools::ToolContext};
use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine};
use rusqlite::{params, OptionalExtension};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, ts_rs::TS)]
#[serde(rename_all = "snake_case")]
#[ts(export, export_to = "../../../ui/src/gen/")]
pub enum ExperienceHistoryKind {
    Sources,
    Versions,
    Related,
}
#[derive(Debug, Clone, Serialize, Deserialize, ts_rs::TS)]
#[serde(deny_unknown_fields)]
#[ts(export, export_to = "../../../ui/src/gen/")]
pub struct ExperienceHistoryRequest {
    pub project_root: String,
    pub skill: String,
    pub entry_id: String,
    pub kind: ExperienceHistoryKind,
    pub cursor: Option<String>,
    pub limit: u32,
}
#[derive(Debug, Clone, Serialize, ts_rs::TS)]
#[serde(tag = "kind", rename_all = "snake_case")]
#[ts(export, export_to = "../../../ui/src/gen/")]
pub enum ExperienceHistoryItem {
    Source {
        source: ExperienceSource,
        path: Option<String>,
    },
    Version {
        operation_id: String,
        entry: ExperienceEntry,
        state: String,
        source_count: u32,
    },
    Control {
        operation_id: String,
        operation: String,
        state: String,
        reason: String,
    },
    Related {
        skill: String,
        entry_id: String,
        revision: u32,
        state: String,
    },
}
#[derive(Debug, Clone, Serialize, ts_rs::TS)]
#[ts(export, export_to = "../../../ui/src/gen/")]
pub struct ExperienceHistoryPage {
    pub items: Vec<ExperienceHistoryItem>,
    pub next_cursor: Option<String>,
}
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Cursor {
    root: String,
    skill: String,
    entry: String,
    kind: ExperienceHistoryKind,
    offset: usize,
    fingerprint: String,
}

pub fn query(
    db: &Db,
    ctx: &ToolContext,
    request: &ExperienceHistoryRequest,
) -> Result<ExperienceHistoryPage, PropError> {
    let started = std::time::Instant::now();
    let result = query_inner(db, ctx, request);
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
        "experience_history",
        if result.is_ok() {
            "page_returned"
        } else {
            "invalid_or_stale_scope"
        },
        started,
    );
    result
}
fn query_inner(
    db: &Db,
    ctx: &ToolContext,
    request: &ExperienceHistoryRequest,
) -> Result<ExperienceHistoryPage, PropError> {
    let root = ctx.repo_root.canonicalize()?.to_string_lossy().into_owned();
    if root != request.project_root || request.limit == 0 || request.limit > 50 {
        return Err(PropError::Rejected(
            "invalid experience history scope or page size".into(),
        ));
    }
    let current = super::storage::entries(db, ctx, &request.skill)?
        .into_iter()
        .find(|v| v.entry.entry_id == request.entry_id)
        .ok_or(PropError::StaleExperienceHistory)?;
    let mut items = Vec::new();
    match request.kind {
        ExperienceHistoryKind::Sources => {
            let mut sources = current.entry.sources.clone();
            sources.sort_by(|a, b| {
                a.review_event
                    .cmp(&b.review_event)
                    .then(a.artifact_id.cmp(&b.artifact_id))
            });
            for source in sources {
                let path = db
                    .conn()
                    .query_row(
                        "SELECT path FROM artifacts WHERE project_id=?1 AND id=?2 AND version=?3",
                        params![ctx.project_id, source.artifact_id, source.artifact_version],
                        |r| r.get(0),
                    )
                    .optional()?;
                items.push(ExperienceHistoryItem::Source { source, path });
            }
        }
        ExperienceHistoryKind::Versions => {
            let mut query=db.conn().prepare("SELECT proposal_id,intent_json,state FROM experience_operations WHERE project_id=?1 ORDER BY rowid")?;
            for row in query.query_map([&ctx.project_id], |r| {
                Ok((
                    r.get::<_, String>(0)?,
                    r.get::<_, String>(1)?,
                    r.get::<_, String>(2)?,
                ))
            })? {
                let (id, raw, operation_state) = row?;
                let intent: Intent =
                    serde_json::from_str(&raw).map_err(|_| PropError::UngovernedExperience)?;
                for target in all_changes(&intent) {
                    if target.entry.skill != request.skill
                        || target.entry.entry_id != request.entry_id
                    {
                        continue;
                    }
                    let exact = serde_json::to_string(&target.entry).ok()
                        == serde_json::to_string(&current.entry).ok();
                    let state = if operation_state != "complete" {
                        "pending_recovery".into()
                    } else if exact {
                        current.state.clone()
                    } else {
                        "historical".into()
                    };
                    let mut entry = target.entry.clone();
                    let source_count = entry.sources.len() as u32;
                    entry.sources.clear();
                    items.push(ExperienceHistoryItem::Version {
                        operation_id: id.clone(),
                        entry,
                        state,
                        source_count,
                    });
                }
            }
            let mut query=db.conn().prepare("SELECT id,kind,state,reason FROM experience_controls WHERE project_id=?1 AND ((skill=?2 AND entry_id=?3) OR EXISTS(SELECT 1 FROM json_each(payload_json,'$.siblings') sibling WHERE json_extract(sibling.value,'$.skill')=?2 AND json_extract(sibling.value,'$.rollback.entry.entry_id')=?3)) ORDER BY rowid")?;
            for row in query.query_map(
                params![ctx.project_id, request.skill, request.entry_id],
                |r| {
                    Ok(ExperienceHistoryItem::Control {
                        operation_id: r.get(0)?,
                        operation: r.get(1)?,
                        state: r.get(2)?,
                        reason: r.get(3)?,
                    })
                },
            )? {
                items.push(row?);
            }
        }
        ExperienceHistoryKind::Related => {
            let mut query=db.conn().prepare("SELECT skill,record_json,state FROM experience_entries WHERE project_id=?1 ORDER BY skill,entry_id")?;
            for row in query.query_map([&ctx.project_id], |r| {
                Ok((
                    r.get::<_, String>(0)?,
                    r.get::<_, String>(1)?,
                    r.get::<_, String>(2)?,
                ))
            })? {
                let (skill, raw, _state) = row?;
                let entry: ExperienceEntry =
                    serde_json::from_str(&raw).map_err(|_| PropError::UngovernedExperience)?;
                if skill == request.skill && entry.entry_id == request.entry_id {
                    continue;
                }
                if !entry
                    .sources
                    .iter()
                    .any(|source| current.entry.sources.contains(source))
                {
                    continue;
                }
                let state = super::storage::entries(db, ctx, &skill)
                    .ok()
                    .and_then(|entries| {
                        entries
                            .into_iter()
                            .find(|v| v.entry.entry_id == entry.entry_id)
                    })
                    .map(|v| v.state)
                    .unwrap_or_else(|| "changed".into());
                items.push(ExperienceHistoryItem::Related {
                    skill,
                    entry_id: entry.entry_id,
                    revision: entry.revision,
                    state,
                });
            }
        }
    }
    let fingerprint =
        hash(&serde_json::to_string(&items).map_err(|_| PropError::UngovernedExperience)?);
    let offset = if let Some(cursor) = &request.cursor {
        let bytes = URL_SAFE_NO_PAD
            .decode(cursor)
            .map_err(|_| PropError::StaleExperienceHistory)?;
        let cursor: Cursor =
            serde_json::from_slice(&bytes).map_err(|_| PropError::StaleExperienceHistory)?;
        if cursor.root != root
            || cursor.skill != request.skill
            || cursor.entry != request.entry_id
            || cursor.kind != request.kind
            || cursor.fingerprint != fingerprint
            || cursor.offset > items.len()
        {
            return Err(PropError::StaleExperienceHistory);
        }
        cursor.offset
    } else {
        0
    };
    let end = (offset + request.limit as usize).min(items.len());
    let next_cursor = if end < items.len() {
        Some(
            URL_SAFE_NO_PAD.encode(
                serde_json::to_vec(&Cursor {
                    root,
                    skill: request.skill.clone(),
                    entry: request.entry_id.clone(),
                    kind: request.kind.clone(),
                    offset: end,
                    fingerprint,
                })
                .map_err(|_| PropError::UngovernedExperience)?,
            ),
        )
    } else {
        None
    };
    Ok(ExperienceHistoryPage {
        items: items.drain(offset..end).collect(),
        next_cursor,
    })
}

#[derive(Debug, Clone, Serialize, Deserialize, ts_rs::TS)]
#[serde(deny_unknown_fields)]
#[ts(export, export_to = "../../../ui/src/gen/")]
pub struct ExperienceSourceRequest {
    pub project_root: String,
    pub skill: String,
    pub entry_id: String,
    #[ts(type = "number")]
    pub review_event: i64,
}
#[derive(Debug, Clone, Serialize, ts_rs::TS)]
#[ts(export, export_to = "../../../ui/src/gen/")]
pub struct ExperienceSourceDocument {
    pub source: ExperienceSource,
    pub path: String,
    pub content: String,
}
pub fn source_document(
    db: &Db,
    ctx: &ToolContext,
    request: &ExperienceSourceRequest,
) -> Result<ExperienceSourceDocument, PropError> {
    let started = std::time::Instant::now();
    let result = (|| {
        if request.project_root != ctx.repo_root.canonicalize()?.to_string_lossy() {
            return Err(PropError::StaleExperienceHistory);
        }
        let source = super::storage::entries(db, ctx, &request.skill)?
            .into_iter()
            .find(|v| v.entry.entry_id == request.entry_id)
            .and_then(|v| {
                v.entry
                    .sources
                    .into_iter()
                    .find(|s| s.review_event == request.review_event)
            })
            .ok_or(PropError::StaleExperienceHistory)?;
        let path: String = db.conn().query_row(
            "SELECT path FROM artifacts WHERE project_id=?1 AND id=?2 AND version=?3",
            params![ctx.project_id, source.artifact_id, source.artifact_version],
            |r| r.get(0),
        )?;
        let content = crate::artifacts::content_at(
            db,
            &ctx.repo_root,
            &ctx.project_id,
            &path,
            source.artifact_version,
        )
        .map_err(|_| PropError::StaleExperienceHistory)?
        .ok_or(PropError::StaleExperienceHistory)?;
        if hash(&content) != source.artifact_digest {
            return Err(PropError::StaleExperienceHistory);
        }
        Ok(ExperienceSourceDocument {
            source,
            path,
            content,
        })
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
        "experience_source",
        if result.is_ok() {
            "verified_version"
        } else {
            "invalid_or_stale_scope"
        },
        started,
    );
    result
}

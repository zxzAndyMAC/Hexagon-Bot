//! Governance 09: owner revocation disables host reuse before touching files.
use super::{
    governance::target_path,
    storage::{block, encode, replace_text},
    ExperienceEntryView, ExperienceRecovery,
};
use crate::{db::Db, proposals::PropError, tools::ToolContext};
use rusqlite::params;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, ts_rs::TS)]
#[serde(deny_unknown_fields)]
#[ts(export, export_to = "../../../ui/src/gen/")]
pub struct ExperienceRevocation {
    pub project_root: String,
    pub skill: String,
    pub entry_id: String,
    pub expected_revision: u32,
    pub reason: String,
}
#[derive(Serialize, Deserialize)]
pub(super) struct StopIntent {
    pub skill: String,
    pub before: String,
    pub after: String,
    pub synchronizable: bool,
    #[serde(default)]
    pub rollback: Option<RollbackReceipt>,
    #[serde(default)]
    pub siblings: Vec<StopIntent>,
}

#[derive(Serialize, Deserialize)]
pub(super) struct RollbackReceipt {
    pub proposal: String,
    pub retained_proposal: String,
    pub entry: super::ExperienceEntry,
    pub state: String,
}

pub fn revoke(
    db: &Db,
    ctx: &ToolContext,
    request: &ExperienceRevocation,
) -> Result<ExperienceEntryView, PropError> {
    let started = std::time::Instant::now();
    let result = revoke_inner(db, ctx, request);
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
        "experience_revoke",
        if result.is_ok() {
            "host_stopped"
        } else {
            "invalid_or_stale"
        },
        started,
    );
    result
}
fn revoke_inner(
    db: &Db,
    ctx: &ToolContext,
    request: &ExperienceRevocation,
) -> Result<ExperienceEntryView, PropError> {
    if ctx.agent_id != "owner"
        || request.reason.trim().is_empty()
        || request.project_root != ctx.repo_root.canonicalize()?.to_string_lossy()
    {
        return Err(PropError::Rejected(
            "revocation requires owner, current project and reason".into(),
        ));
    }
    let _lease = if ctx.write_lease.is_none() {
        Some(crate::tools::writeguard::repository_lock(ctx)?)
    } else {
        None
    };
    let view = super::storage::entries(db, ctx, &request.skill)?
        .into_iter()
        .find(|v| v.entry.entry_id == request.entry_id)
        .ok_or(PropError::StaleExperience)?;
    if view.entry.revision != request.expected_revision {
        return Err(PropError::StaleExperience);
    }
    if view.state == "revoked" {
        return Ok(view);
    }
    if view.state == "pending_recovery" {
        return Err(PropError::Rejected(
            "recover the pending entry before revocation".into(),
        ));
    }
    let candidate = super::role_skill::prospective(ctx, &request.skill)?;
    let before = if candidate.exists() {
        std::fs::read_to_string(target_path(ctx, &request.skill)?)?
    } else {
        String::new()
    };
    let old = block(&encode(&view.entry)?);
    // Keep body/source bytes as historical evidence; this host marker is only
    // a visible stop annotation, never authority to re-enable an entry.
    let stopped = format!(
        "<!-- hexagon-stopped-experience:{} -->\n{}",
        view.entry.entry_id, old
    );
    let synchronizable = before.matches(&old).count() == 1;
    let after = if synchronizable {
        before.replacen(&old, &stopped, 1)
    } else {
        before.clone()
    };
    let id = format!("expctl{}", db.next_id("experience-control")?);
    let tx = db.conn().unchecked_transaction()?;
    db.conn().execute("UPDATE experience_entries SET state='revoked' WHERE project_id=?1 AND skill=?2 AND entry_id=?3",params![ctx.project_id,request.skill,request.entry_id])?;
    db.conn().execute("INSERT INTO experience_controls(id,project_id,skill,entry_id,kind,state,reason,payload_json) VALUES(?1,?2,?3,?4,'revoke','pending',?5,?6)",params![id,ctx.project_id,request.skill,request.entry_id,request.reason,encode(&StopIntent{skill:request.skill.clone(),before,after,synchronizable,rollback:None,siblings:Vec::new()})?])?;
    tx.commit()?;
    #[cfg(test)]
    super::storage::fault(super::ExperienceFault::AfterIntent)?;
    // The stop is durable even if synchronization conflicts. Do not roll it
    // back or report the file as complete; the result carries recovery_pending.
    recover(db, ctx)?;
    super::storage::entries(db, ctx, &request.skill)?
        .into_iter()
        .find(|v| v.entry.entry_id == request.entry_id)
        .ok_or(PropError::StaleExperience)
}
pub(super) fn recover(db: &Db, ctx: &ToolContext) -> Result<Vec<ExperienceRecovery>, PropError> {
    let mut query = db.conn().prepare("SELECT id,payload_json FROM experience_controls WHERE project_id=?1 AND state!='complete' ORDER BY id")?;
    let rows = query
        .query_map([&ctx.project_id], |r| {
            Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?))
        })?
        .collect::<Result<Vec<_>, _>>()?;
    let mut results = Vec::new();
    for (id, raw) in rows {
        let started = std::time::Instant::now();
        let attempt = (|| -> Result<(), PropError> {
            let intent: StopIntent =
                serde_json::from_str(&raw).map_err(|_| PropError::UngovernedExperience)?;
            for intent in std::iter::once(&intent).chain(intent.siblings.iter()) {
                if !intent.synchronizable {
                    return Err(PropError::StaleExperience);
                }
                let current = std::fs::read_to_string(target_path(ctx, &intent.skill)?)?;
                if current != intent.after {
                    if current != intent.before {
                        return Err(PropError::StaleExperience);
                    }
                    replace_text(ctx, &intent.skill, &intent.before, &intent.after)?;
                }
                #[cfg(test)]
                super::storage::fault(super::ExperienceFault::AfterReplace)?;
            }
            // Governance 12: all target files must be observed before any
            // contribution receipt or proposal completion is committed.
            for target in std::iter::once(&intent).chain(intent.siblings.iter()) {
                if std::fs::read_to_string(target_path(ctx, &target.skill)?)? != target.after {
                    return Err(PropError::StaleExperience);
                }
            }
            let tx = db.conn().unchecked_transaction()?;
            for intent in std::iter::once(&intent).chain(intent.siblings.iter()) {
                if let Some(receipt) = &intent.rollback {
                    db.conn().execute("UPDATE experience_entries SET record_json=?1,state=?2,proposal_id=?3 WHERE project_id=?4 AND skill=?5 AND entry_id=?6",params![encode(&receipt.entry)?,receipt.state,receipt.retained_proposal,ctx.project_id,receipt.entry.skill,receipt.entry.entry_id])?;
                    let changed=db.conn().execute("UPDATE proposals SET status='rolled_back' WHERE project_id=?1 AND id=?2 AND status!='rolled_back'",params![ctx.project_id,receipt.proposal])?;
                    if changed > 0 {
                        db.append_event(&ctx.project_id,crate::trace::EventKind::ProposalRolledBack,serde_json::json!({"proposal_id":receipt.proposal,"entry_id":receipt.entry.entry_id,"retained_sources":receipt.entry.sources.len(),"whole_file_restored":false}),None,None)?;
                    }
                }
            }
            tx.commit()?;
            Ok(())
        })();
        let state = if attempt.is_ok() {
            "complete"
        } else {
            "conflict"
        };
        db.conn().execute(
            "UPDATE experience_controls SET state=?1 WHERE id=?2 AND project_id=?3",
            params![state, id, ctx.project_id],
        )?;
        crate::diag::note(
            if attempt.is_ok() {
                crate::diag::CLASS_JUDGE
            } else {
                crate::diag::CLASS_REJECT
            },
            attempt.is_err(),
            Some(&ctx.project_id),
            Some(&ctx.agent_id),
            ctx.stage_run_id.as_deref(),
            None,
            "experience_control_recovery",
            if attempt.is_ok() {
                "completed"
            } else {
                "preserved_conflict"
            },
            started,
        );
        results.push(ExperienceRecovery {
            operation_id: id,
            proposal_id: None,
            state: state.into(),
            reason_code: attempt.err().map(|e| {
                // Governance 16 native smoke: control recovery validates
                // file/receipt preconditions, not a proposal's qualification.
                // Reusing the proposal mapping mislabeled file drift as
                // approval expiry and sent the owner to the wrong remedy.
                if matches!(e, PropError::StaleExperience) {
                    "target_changed".into()
                } else {
                    super::storage::recovery_reason(&e).into()
                }
            }),
        });
    }
    Ok(results)
}

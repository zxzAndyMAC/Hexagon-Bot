//! D08 / reliability 19: owner exceptions attest a version, not execution success.
use super::*;
use rusqlite::OptionalExtension;
use std::collections::BTreeMap;

const KIND: &str = "acceptance_exception";

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize, ts_rs::TS)]
#[serde(tag = "kind", rename_all = "snake_case")]
#[ts(export, export_to = "../../../ui/src/gen/")]
pub enum ExceptionRequirement {
    Check {
        run_id: String,
        cmd: String,
    },
    Review {
        run_id: String,
        artifact_id: String,
        reviewer: String,
    },
}
impl ExceptionRequirement {
    fn run_id(&self) -> &str {
        match self {
            Self::Check { run_id, .. } | Self::Review { run_id, .. } => run_id,
        }
    }
}

#[derive(Debug, Clone, Serialize, ts_rs::TS)]
#[ts(export, export_to = "../../../ui/src/gen/")]
pub struct ExceptionCandidate {
    pub requirement: ExceptionRequirement,
    pub label: String,
    pub accepted: bool,
}

#[derive(Debug, Clone, Serialize, ts_rs::TS)]
#[ts(export, export_to = "../../../ui/src/gen/")]
pub struct ExceptionRequest {
    pub question_id: String,
}

#[derive(Debug, Clone, Serialize, ts_rs::TS)]
#[ts(export, export_to = "../../../ui/src/gen/")]
pub struct ExceptionAcceptance {
    #[ts(type = "number")]
    pub event_id: i64,
}

fn scope_fingerprint(
    db: &Db,
    project: &str,
    pack: &PackDef,
    run: &str,
) -> Result<Option<String>, OrchError> {
    let (seq, root): (i64, String) = db.conn().query_row(
        "SELECT s.seq,p.dir FROM stage_runs s JOIN projects p ON p.id=s.project_id WHERE s.project_id=?1 AND s.id=?2",
        rusqlite::params![project,run], |r| Ok((r.get(0)?,r.get(1)?)))?;
    let stage = pack
        .stages
        .get(seq as usize)
        .ok_or(OrchError::BadSeq(seq))?;
    Ok(crate::artifacts::fingerprint::capture(
        db,
        Path::new(&root),
        project,
        run,
        &required_kinds(stage),
    )
    .ok())
}

// Each scope is revalidated independently at final acceptance. Comparing an
// earlier stage's aggregate fingerprint to the final aggregate would expire a
// decision solely because another stage opened, even with unchanged delivery.
pub(super) fn accepted_for_run(
    db: &Db,
    project: &str,
    pack: &PackDef,
    run: &str,
) -> Result<Vec<ExceptionRequirement>, OrchError> {
    let Some(current) = scope_fingerprint(db, project, pack, run)? else {
        return Ok(Vec::new());
    };
    let mut query = db.conn().prepare("SELECT payload FROM events WHERE project_id=?1 AND kind='system' AND json_extract(payload,'$.kind')=?2 ORDER BY id")?;
    let payloads = query
        .query_map(rusqlite::params![project, KIND], |r| r.get::<_, String>(0))?
        .collect::<Result<Vec<_>, _>>()?;
    let mut accepted = Vec::new();
    for text in payloads {
        let Ok(p) = serde_json::from_str::<Value>(&text) else {
            continue;
        };
        if p["by"] != "owner"
            || p["reason"].as_str().is_none_or(|s| s.trim().is_empty())
            || p["scopes"][run] != current
        {
            continue;
        }
        if let Ok(items) =
            serde_json::from_value::<Vec<ExceptionRequirement>>(p["requirements"].clone())
        {
            for item in items.into_iter().filter(|r| r.run_id() == run) {
                if let ExceptionRequirement::Check { cmd, .. } = &item {
                    let binding = quality_config::check_binding(db, project, pack, run, cmd)?;
                    if !quality_config::binding_current(
                        p["check_configurations"][run][cmd].as_str(),
                        &binding,
                    ) {
                        continue;
                    }
                }
                accepted.push(item);
            }
        }
    }
    Ok(accepted)
}

pub(super) fn candidates(
    db: &Db,
    project: &str,
    pack: &PackDef,
    active: &StageRun,
) -> Result<Vec<ExceptionCandidate>, OrchError> {
    let root: String =
        db.conn()
            .query_row("SELECT dir FROM projects WHERE id=?1", [project], |r| {
                r.get(0)
            })?;
    let mut out = Vec::new();
    for run in evidence_runs(db, project, pack, active)? {
        if declared_absence(db, project, pack, &run)? {
            continue;
        }
        let stage = pack
            .stages
            .get(run.seq as usize)
            .ok_or(OrchError::BadSeq(run.seq))?;
        let accepted = accepted_for_run(db, project, pack, &run.id)?;
        for check in check_evidence(db, project, &run.id, stage, pack)? {
            if check.state == CheckState::Passed {
                continue;
            }
            let requirement = ExceptionRequirement::Check {
                run_id: run.id.clone(),
                cmd: check.cmd.clone(),
            };
            out.push(ExceptionCandidate {
                accepted: accepted.contains(&requirement),
                requirement,
                label: format!("{} · {}", stage.name, check.cmd),
            });
        }
        for review in &stage.reviews {
            let targets = crate::artifacts::delivery::for_run(
                db,
                project,
                &run.id,
                std::slice::from_ref(&review.artifact_kind),
            )?;
            for delivery in targets.items {
                let crate::artifacts::delivery::Delivery {
                    id,
                    path,
                    stage: review_run,
                    ..
                } = delivery;
                // Missing bodies/identity cannot become a review exception.
                if crate::artifacts::evidence::capture(db, Path::new(&root), project, &id)?
                    .is_none()
                    || crate::artifacts::evidence::review_satisfied(
                        db,
                        Path::new(&root),
                        project,
                        &review_run,
                        &id,
                        &review.reviewer,
                    )?
                {
                    continue;
                }
                let requirement = ExceptionRequirement::Review {
                    run_id: run.id.clone(),
                    artifact_id: id,
                    reviewer: review.reviewer.clone(),
                };
                out.push(ExceptionCandidate {
                    accepted: accepted.contains(&requirement),
                    requirement,
                    label: format!("{} · {} · {}", stage.name, path, review.reviewer),
                });
            }
        }
    }
    Ok(out)
}

fn current_evidence(
    db: &Db,
    project: &str,
    pack: &PackDef,
    expected: &str,
) -> Result<StageEvidence, OrchError> {
    let evidence =
        stage_evidence(db, project, pack)?.ok_or(OrchError::InvalidAcceptanceException)?;
    if expected.is_empty() || evidence.fingerprint.as_deref() != Some(expected) {
        return Err(OrchError::StaleAcceptanceVersion);
    }
    if evidence.missing.iter().any(|m| m.starts_with("action:")) {
        return Err(OrchError::UnresolvedDeliveryAction);
    }
    Ok(evidence)
}

fn request_inner(
    db: &Db,
    project: &str,
    pack: &PackDef,
    expected: &str,
) -> Result<ExceptionRequest, OrchError> {
    let _lease = write_boundary(db, project)?;
    let tx =
        rusqlite::Transaction::new_unchecked(db.conn(), rusqlite::TransactionBehavior::Immediate)?;
    let evidence = current_evidence(db, project, pack, expected)?;
    if !evidence.exceptions.iter().any(|r| !r.accepted) {
        return Err(OrchError::InvalidAcceptanceException);
    }
    if let Some(card) = db.queued_questions(project)?.into_iter().find(|c| {
        c.payload["sub"] == KIND
            && c.payload["run_id"] == evidence.run_id
            && c.payload["fingerprint"] == expected
    }) {
        return Ok(ExceptionRequest {
            question_id: card.id,
        });
    }
    let question_id = crate::cards::enqueue(
        db,
        project,
        None,
        crate::cards::CardKind::Stamp,
        json!({"sub":KIND,"run_id":evidence.run_id,"fingerprint":expected}),
        None,
    )?;
    db.append_event(
        project,
        EventKind::PermissionAsked,
        json!({"kind":KIND,"question_id":question_id}),
        None,
        Some(&evidence.run_id),
    )?;
    current_evidence(db, project, pack, expected)?;
    tx.commit()?;
    Ok(ExceptionRequest { question_id })
}

fn accept_inner(
    db: &Db,
    project: &str,
    pack: &PackDef,
    question: &str,
    expected: &str,
    selected: &[ExceptionRequirement],
    reason: &str,
) -> Result<ExceptionAcceptance, OrchError> {
    let _lease = write_boundary(db, project)?;
    let tx =
        rusqlite::Transaction::new_unchecked(db.conn(), rusqlite::TransactionBehavior::Immediate)?;
    let reason = reason.trim();
    if reason.is_empty() || selected.is_empty() {
        return Err(OrchError::InvalidAcceptanceException);
    }
    let mut selected = selected.to_vec();
    selected.sort();
    selected.dedup();
    let evidence = current_evidence(db, project, pack, expected)?;
    let prior: Option<(i64,String)> = db.conn().query_row("SELECT id,payload FROM events WHERE project_id=?1 AND kind='system' AND json_extract(payload,'$.kind')=?2 AND json_extract(payload,'$.question_id')=?3 ORDER BY id DESC LIMIT 1",
        rusqlite::params![project,KIND,question], |r| Ok((r.get(0)?,r.get(1)?))).optional()?;
    if let Some((event_id, text)) = prior {
        let p: Value = serde_json::from_str(&text)?;
        if p["fingerprint"] != expected
            || p["reason"] != reason
            || p["requirements"] != json!(selected)
        {
            return Err(OrchError::InvalidAcceptanceException);
        }
        return Ok(ExceptionAcceptance { event_id });
    }
    let card = crate::cards::get_queued(db, question, crate::cards::CardKind::Stamp)?;
    if card.project_id != project
        || card.payload["sub"] != KIND
        || card.payload["run_id"] != evidence.run_id
        || card.payload["fingerprint"] != expected
        || selected.iter().any(|r| {
            !evidence
                .exceptions
                .iter()
                .any(|c| !c.accepted && &c.requirement == r)
        })
    {
        return Err(OrchError::InvalidAcceptanceException);
    }
    let mut scopes = BTreeMap::new();
    let mut configurations = BTreeMap::<String, BTreeMap<String, String>>::new();
    for requirement in &selected {
        let run = requirement.run_id();
        if let ExceptionRequirement::Check { cmd, .. } = requirement {
            configurations.entry(run.to_string()).or_default().insert(
                cmd.clone(),
                quality_config::check_binding(db, project, pack, run, cmd)?,
            );
        }
        scopes.insert(
            run.to_string(),
            scope_fingerprint(db, project, pack, run)?.ok_or(OrchError::StaleAcceptanceVersion)?,
        );
    }
    current_evidence(db, project, pack, expected)?;
    let event_id = db.append_event(project,EventKind::System,json!({"kind":KIND,"question_id":question,"fingerprint":expected,"scopes":scopes,"check_configurations":configurations,"requirements":selected,"reason":reason,"by":"owner"}),None,Some(&evidence.run_id))?;
    crate::cards::answer(db, question, "owner")?;
    tx.commit()?;
    Ok(ExceptionAcceptance { event_id })
}

/// Cancellation consumes only the exception request, never a stage stamp card.
fn cancel_inner(db: &Db, project: &str, question: &str) -> Result<(), OrchError> {
    let tx =
        rusqlite::Transaction::new_unchecked(db.conn(), rusqlite::TransactionBehavior::Immediate)?;
    let card = crate::cards::get(db, question)?;
    if card.project_id != project || card.kind != "stamp" || card.payload["sub"] != KIND {
        return Err(OrchError::InvalidAcceptanceException);
    }
    if card.state == crate::cards::CardState::Answered {
        return Ok(());
    }
    crate::cards::get_queued(db, question, crate::cards::CardKind::Stamp)?;
    crate::cards::answer(db, question, "owner")?;
    db.append_event(
        project,
        EventKind::PermissionDenied,
        json!({"kind":KIND,"question_id":question,"by":"owner"}),
        None,
        card.payload["run_id"].as_str(),
    )?;
    tx.commit()?;
    Ok(())
}

fn report<T>(
    project: &str,
    branch: &str,
    started: std::time::Instant,
    result: Result<T, OrchError>,
) -> Result<T, OrchError> {
    let code = match &result {
        Ok(_) => "accepted",
        Err(OrchError::StaleAcceptanceVersion) => "stale_version",
        Err(OrchError::UnresolvedDeliveryAction) => "unknown_action",
        Err(OrchError::InvalidAcceptanceException) => "invalid_selection",
        Err(_) => "boundary_refused",
    };
    crate::diag::note(
        if result.is_err() {
            crate::diag::CLASS_REJECT
        } else {
            crate::diag::CLASS_JUDGE
        },
        result.is_err(),
        Some(project),
        Some("owner"),
        None,
        None,
        branch,
        code,
        started,
    );
    result
}

pub fn request(
    db: &Db,
    project: &str,
    pack: &PackDef,
    expected: &str,
) -> Result<ExceptionRequest, OrchError> {
    let effective = quality_config::effective_pack(db, project, pack)?;
    let pack = &effective;
    let started = std::time::Instant::now();
    report(
        project,
        "exception_request",
        started,
        request_inner(db, project, pack, expected),
    )
}
pub fn accept(
    db: &Db,
    project: &str,
    pack: &PackDef,
    question: &str,
    expected: &str,
    selected: &[ExceptionRequirement],
    reason: &str,
) -> Result<ExceptionAcceptance, OrchError> {
    let effective = quality_config::effective_pack(db, project, pack)?;
    let pack = &effective;
    let started = std::time::Instant::now();
    report(
        project,
        "exception_accept",
        started,
        accept_inner(db, project, pack, question, expected, selected, reason),
    )
}
pub fn cancel(db: &Db, project: &str, question: &str) -> Result<(), OrchError> {
    let started = std::time::Instant::now();
    report(
        project,
        "exception_cancel",
        started,
        cancel_inner(db, project, question),
    )
}

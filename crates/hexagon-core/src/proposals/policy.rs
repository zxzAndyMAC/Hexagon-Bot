//! D13/D14: replay produces a candidate; independent quality precedes owner adoption.
use super::*;
use crate::orchestra::PackDef;
use rusqlite::OptionalExtension;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::path::{Path, PathBuf};

const TARGET: &str = ".hexagon/pack.active.json";

#[derive(Serialize, Deserialize)]
pub(crate) struct Candidate {
    pub baseline: PackDef,
    pub candidate: PackDef,
}

#[derive(Serialize, Deserialize)]
pub(super) struct Binding {
    baseline_digest: String,
    proposal_digest: String,
    resolved: String,
    author: String,
    candidate: PackDef,
    #[serde(default)]
    configuration_fingerprint: Option<String>,
}

// Freeze only a digest; provider credentials are stored separately, and no
// provider URL or role instructions are copied into proposal payloads here.
fn configuration_fingerprint(
    db: &Db,
    project: &str,
    baseline: &PackDef,
    candidate: &PackDef,
) -> Result<String, PropError> {
    // Ticket 16: pack reviewers need not have an instantiated Agent. Freezing
    // only team rows let changed reviewer instructions escape stale checks.
    let mut names: std::collections::BTreeSet<String> = crate::roles::team_roles(db, project)
        .map_err(|_| PropError::PolicyConstraint)?
        .into_iter()
        .collect();
    for pack in [baseline, candidate] {
        for stage in &pack.stages {
            names.extend(stage.roles.iter().cloned());
            names.extend(stage.reviews.iter().map(|r| r.reviewer.clone()));
            names.extend(stage.consult_wake.iter().cloned());
            for (from, to) in &stage.backfill_edges {
                names.insert(from.clone());
                names.insert(to.clone());
            }
        }
    }
    let definitions = crate::evaluation::config::collect_roles(names, |name| {
        crate::roles::role_def(db, project, name).map_err(std::io::Error::other)
    })
    .map_err(|_| PropError::PolicyConstraint)?;
    let providers = crate::provider_config::load().map_err(|_| PropError::PolicyConstraint)?;
    let reviewer_mode = crate::autonomy::reviewer_mode(db, project)?;
    // Value canonicalizes map ordering before hashing provider slot maps.
    let value = json!({"providers":providers,"roles":definitions,"reviewer_mode":reviewer_mode});
    Ok(digest(
        &serde_json::to_vec(&value).map_err(|_| PropError::PolicyConstraint)?,
    ))
}

fn digest(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}
fn target(ctx: &ToolContext) -> Result<PathBuf, PropError> {
    let path =
        crate::tools::repo_path(&ctx.repo_root, TARGET).map_err(|_| PropError::PolicyConstraint)?;
    // No alias into another host file or user source, even within this repo.
    let root = ctx.repo_root.canonicalize()?;
    if path != root.join(TARGET) {
        return Err(PropError::PolicyConstraint);
    }
    Ok(path)
}

fn constraints(db: &Db, ctx: &ToolContext, author: &str) -> Result<(), PropError> {
    let role: Option<String> = db
        .conn()
        .query_row(
            "SELECT role FROM agents WHERE project_id=?1 AND id=?2",
            params![ctx.project_id, author],
            |r| r.get(0),
        )
        .optional()?;
    if role.as_deref() != Some(crate::policydev::ROLE) {
        return Err(PropError::PolicyConstraint);
    }
    let mut current = ctx.clone();
    current.agent_id = author.into();
    current.stage_run_id = db.active_stage_run(&ctx.project_id)?.map(|r| r.id);
    for tool in ["fs_write", "fs_patch"] {
        if crate::permissions::matching_rule(db, &current, tool, &json!({"path":TARGET}), "deny")
            .map_err(|_| PropError::PolicyConstraint)?
            .is_some()
        {
            return Err(PropError::PolicyConstraint);
        }
    }
    Ok(())
}

pub(super) fn capture(
    db: &Db,
    ctx: &ToolContext,
    body: &str,
    destination: &str,
) -> Result<Binding, PropError> {
    if destination != TARGET {
        return Err(PropError::PolicyConstraint);
    }
    constraints(db, ctx, &ctx.agent_id)?;
    let pair: Candidate = fenced(body, "policy")
        .and_then(|s| serde_json::from_str(&s).ok())
        .ok_or(PropError::PolicyStale)?;
    if !crate::orchestra::non_policy_changes(&pair.baseline, &pair.candidate).is_empty()
        || crate::orchestra::policy_diff(&pair.baseline, &pair.candidate).is_empty()
    {
        return Err(PropError::PolicyConstraint);
    }
    let path = target(ctx)?;
    let bytes = std::fs::read(&path).map_err(|_| PropError::PolicyStale)?;
    let active: PackDef = serde_json::from_slice(&bytes).map_err(|_| PropError::PolicyStale)?;
    if serde_json::to_value(active).ok() != serde_json::to_value(&pair.baseline).ok() {
        return Err(PropError::PolicyStale);
    }
    Ok(Binding {
        baseline_digest: digest(&bytes),
        proposal_digest: digest(body.as_bytes()),
        resolved: path.to_string_lossy().into_owned(),
        author: ctx.agent_id.clone(),
        configuration_fingerprint: Some(configuration_fingerprint(
            db,
            &ctx.project_id,
            &pair.baseline,
            &pair.candidate,
        )?),
        candidate: pair.candidate,
    })
}

fn bound(db: &Db, ctx: &ToolContext, pid: &str, body: &str) -> Result<Binding, PropError> {
    let payload: Option<String> = db.conn().query_row("SELECT payload FROM events WHERE project_id=?1 AND kind='proposal_queued' AND json_extract(payload,'$.proposal_id')=?2 ORDER BY id DESC LIMIT 1",
        params![ctx.project_id,pid], |r|r.get(0)).optional()?;
    let binding: Binding = payload
        .and_then(|s| serde_json::from_str::<Value>(&s).ok())
        .and_then(|p| serde_json::from_value(p["policy_binding"].clone()).ok())
        .ok_or(PropError::PolicyStale)?;
    let author: String = db.conn().query_row(
        "SELECT author_agent_id FROM proposals WHERE project_id=?1 AND id=?2",
        params![ctx.project_id, pid],
        |r| r.get(0),
    )?;
    if binding.author != author || binding.proposal_digest != digest(body.as_bytes()) {
        return Err(PropError::PolicyStale);
    }
    constraints(db, ctx, &author)?;
    let path = target(ctx)?;
    if path.to_string_lossy() != binding.resolved
        || digest(&std::fs::read(&path)?) != binding.baseline_digest
    {
        return Err(PropError::PolicyStale);
    }
    // Re-run the same host constraints before applying the persisted candidate.
    let mut author_ctx = ctx.clone();
    author_ctx.agent_id = author;
    let current = capture(db, &author_ctx, body, TARGET)?;
    if current.configuration_fingerprint != binding.configuration_fingerprint {
        return Err(PropError::PolicyStale);
    }
    Ok(binding)
}

fn replace(path: &Path, bytes: &[u8]) -> Result<(), PropError> {
    use std::io::Write;
    let mut tmp =
        tempfile::NamedTempFile::new_in(path.parent().ok_or(PropError::PolicyConstraint)?)?;
    tmp.as_file_mut().write_all(bytes)?;
    if let Ok(meta) = std::fs::metadata(path) {
        tmp.as_file().set_permissions(meta.permissions())?;
    }
    tmp.as_file().sync_all()?;
    tmp.persist(path).map_err(|e| PropError::Io(e.error))?;
    std::fs::File::open(path.parent().ok_or(PropError::PolicyConstraint)?)?.sync_all()?;
    Ok(())
}

fn backup(ctx: &ToolContext, pid: &str) -> Result<PathBuf, PropError> {
    let rel = format!(".hexagon/proposals/{pid}");
    let path =
        crate::tools::repo_path(&ctx.repo_root, &rel).map_err(|_| PropError::PolicyConstraint)?;
    if path != ctx.repo_root.canonicalize()?.join(rel) {
        return Err(PropError::PolicyConstraint);
    }
    Ok(path)
}

#[cfg(test)]
thread_local! { static FAULT: std::cell::RefCell<Option<Option<String>>> = const { std::cell::RefCell::new(None) }; }
#[cfg(test)]
pub(crate) fn crash_after_replace() {
    FAULT.with(|f| *f.borrow_mut() = Some(None));
}
#[cfg(test)]
pub(crate) fn edit_after_replace(text: &str) {
    FAULT.with(|f| *f.borrow_mut() = Some(Some(text.into())));
}
fn checkpoint(_path: &Path) -> Result<(), PropError> {
    #[cfg(test)]
    if let Some(fault) = FAULT.with(|f| f.borrow_mut().take()) {
        match fault {
            None => panic!("policy crash after replacement"),
            Some(text) => {
                std::fs::write(_path, text)?;
                return Err(PropError::PolicyRecovery);
            }
        }
    }
    Ok(())
}

fn finish(
    db: &Db,
    ctx: &ToolContext,
    id: &str,
    pid: &str,
    qid: Option<&str>,
    operation: &str,
) -> Result<(), PropError> {
    if operation == "adopt" {
        let qid = qid.ok_or(PropError::PolicyRecovery)?;
        crate::cards::answer(db, qid, "owner")?;
        db.conn().execute(
            "UPDATE proposals SET status='active',decided_at=datetime('now') WHERE id=?1",
            [pid],
        )?;
        db.append_event(
            &ctx.project_id,
            EventKind::ProposalStamped,
            json!({"proposal_id":pid,"question_id":qid,"by":"owner","policy_change":id}),
            None,
            None,
        )?;
        db.append_event(
            &ctx.project_id,
            EventKind::ProposalActivated,
            json!({"proposal_id":pid,"effective_path":TARGET,"by":"owner","policy_change":id}),
            None,
            None,
        )?;
    } else {
        db.conn().execute(
            "UPDATE proposals SET status='rolled_back' WHERE id=?1",
            [pid],
        )?;
        db.append_event(
            &ctx.project_id,
            EventKind::ProposalRolledBack,
            json!({"proposal_id":pid,"restored":TARGET,"by":"owner","policy_change":id}),
            None,
            None,
        )?;
    }
    db.conn().execute(
        "UPDATE policy_changes SET state='complete' WHERE id=?1",
        [id],
    )?;
    Ok(())
}

fn conflict(
    db: &Db,
    ctx: &ToolContext,
    id: &str,
    pid: &str,
    qid: Option<&str>,
) -> Result<(), PropError> {
    let tx = db.conn().unchecked_transaction()?;
    let old: String =
        db.conn()
            .query_row("SELECT state FROM policy_changes WHERE id=?1", [id], |r| {
                r.get(0)
            })?;
    db.conn().execute(
        "UPDATE policy_changes SET state='conflict' WHERE id=?1",
        [id],
    )?;
    if let Some(qid) = qid {
        crate::cards::annotate(db, qid, &[("policy_recovery", json!(true))])?;
    }
    if old != "conflict" {
        db.append_event(&ctx.project_id,EventKind::System,json!({"kind":"policy_recovery","policy_change":id,"proposal_id":pid,"state":"conflict"}),None,None)?;
    }
    tx.commit()?;
    Ok(())
}

pub(crate) fn recover(db: &Db, root: &Path, project: &str) -> Result<(), PropError> {
    let mut ctx = ToolContext::owner(db, root);
    ctx.project_id = project.into();
    let _lease = crate::tools::writeguard::repository_lock(&ctx)?;
    let mut query=db.conn().prepare("SELECT id,proposal_id,question_id,operation,before_content,after_content FROM policy_changes WHERE project_id=?1 AND state IN ('pending','conflict') ORDER BY rowid")?;
    let rows = query
        .query_map([project], |r| {
            Ok((
                r.get::<_, String>(0)?,
                r.get::<_, String>(1)?,
                r.get::<_, Option<String>>(2)?,
                r.get::<_, String>(3)?,
                r.get::<_, String>(4)?,
                r.get::<_, String>(5)?,
            ))
        })?
        .collect::<Result<Vec<_>, _>>()?;
    for (id, pid, qid, operation, before, after) in rows {
        let started = std::time::Instant::now();
        let actual = target(&ctx).ok().and_then(|path| std::fs::read(path).ok());
        if actual.as_deref() == Some(after.as_bytes()) {
            let tx = rusqlite::Transaction::new_unchecked(
                db.conn(),
                rusqlite::TransactionBehavior::Immediate,
            )?;
            finish(db, &ctx, &id, &pid, qid.as_deref(), &operation)?;
            tx.commit()?;
            recovery_note(&ctx, false, "observed_replacement", started);
        } else if actual.as_deref() == Some(before.as_bytes()) {
            db.conn().execute(
                "UPDATE policy_changes SET state='aborted' WHERE id=?1",
                [&id],
            )?;
            if let Some(qid) = &qid {
                crate::cards::annotate(db, qid, &[("policy_recovery", json!(false))])?;
            }
            recovery_note(&ctx, false, "observed_baseline", started);
        } else {
            // Reliability 21 review: external edits survive both compensation
            // and restart. Unknown bytes never authorize a recovery overwrite.
            conflict(db, &ctx, &id, &pid, qid.as_deref())?;
            recovery_note(&ctx, true, "external_change", started);
        }
    }
    Ok(())
}

fn recovery_note(ctx: &ToolContext, refused: bool, code: &str, started: std::time::Instant) {
    crate::diag::note(
        if refused {
            crate::diag::CLASS_REJECT
        } else {
            crate::diag::CLASS_JUDGE
        },
        refused,
        Some(&ctx.project_id),
        Some("owner"),
        None,
        None,
        "policy_recovery",
        code,
        started,
    );
}

fn perform(
    db: &Db,
    ctx: &ToolContext,
    pid: &str,
    qid: Option<&str>,
    operation: &str,
    before: &[u8],
    after: &[u8],
) -> Result<(), PropError> {
    let unresolved:bool=db.conn().query_row("SELECT EXISTS(SELECT 1 FROM policy_changes WHERE project_id=?1 AND state IN ('pending','conflict'))",[&ctx.project_id],|r|r.get(0))?;
    if unresolved {
        return Err(PropError::PolicyRecovery);
    }
    let id = format!("{pid}:{operation}");
    let before_text = std::str::from_utf8(before).map_err(|_| PropError::PolicyConstraint)?;
    let after_text = std::str::from_utf8(after).map_err(|_| PropError::PolicyConstraint)?;
    // Commit owner intent before touching the file. A crash after replace can
    // then finish the original decision exactly once, without executing again.
    let inserted = db.conn().execute("INSERT INTO policy_changes(id,project_id,proposal_id,question_id,operation,before_content,after_content,state)
        VALUES (?1,?2,?3,?4,?5,?6,?7,'pending') ON CONFLICT(id) DO UPDATE SET before_content=excluded.before_content,after_content=excluded.after_content,question_id=excluded.question_id,state='pending' WHERE policy_changes.state='aborted'",
        params![id,ctx.project_id,pid,qid,operation,before_text,after_text])?;
    if inserted != 1 {
        return Err(PropError::PolicyRecovery);
    }
    let path = target(ctx)?;
    let result = (|| {
        let tx = rusqlite::Transaction::new_unchecked(
            db.conn(),
            rusqlite::TransactionBehavior::Immediate,
        )?;
        // Revalidate the queued decision and current permissions under the
        // same SQLite write transaction that records the actual replacement.
        let (status, author, artifact): (String, String, String) = db.conn().query_row(
            "SELECT status,author_agent_id,artifact_id FROM proposals WHERE id=?1 AND project_id=?2",
            params![pid,ctx.project_id], |r| Ok((r.get(0)?,r.get(1)?,r.get(2)?)))?;
        constraints(db, ctx, &author)?;
        if operation == "adopt" {
            let card = crate::cards::get_queued(
                db,
                qid.ok_or(PropError::PolicyRecovery)?,
                crate::cards::CardKind::Stamp,
            )?;
            if status != "awaiting_stamp"
                || card.project_id != ctx.project_id
                || card.payload["proposal_id"] != pid
            {
                return Err(PropError::PolicyStale);
            }
            let (_, _, _, body) = artifact_proposal_parts(db, ctx, Some(&artifact))?;
            bound(db, ctx, pid, &body)?;
        } else if status != "active" {
            return Err(PropError::BadState(status));
        }
        if std::fs::read(&path)? != before {
            return Err(PropError::PolicyStale);
        }
        replace(&path, after)?;
        checkpoint(&path)?;
        finish(db, ctx, &id, pid, qid, operation)?;
        tx.commit()?;
        Ok::<_, PropError>(())
    })();
    if let Err(error) = result {
        match std::fs::read(&path) {
            Ok(current) if current == after => {
                replace(&path, before)?;
                db.conn().execute(
                    "UPDATE policy_changes SET state='aborted' WHERE id=?1",
                    [&id],
                )?;
            }
            Ok(current) if current == before => {
                db.conn().execute(
                    "UPDATE policy_changes SET state='aborted' WHERE id=?1",
                    [&id],
                )?;
            }
            _ => {
                conflict(db, ctx, &id, pid, qid)?;
                return Err(PropError::PolicyRecovery);
            }
        }
        return Err(error);
    }
    Ok(())
}

// Evaluation 16 / D14: owner approval and replay scores are not evidence.
// False refusal costs an independent evaluation; false admission applies an
// unverified policy. Persisted historical operations are recovered separately.
fn require_independent_quality(_db: &Db, _ctx: &ToolContext, _pid: &str) -> Result<(), PropError> {
    Err(PropError::PolicyQualityUnverified)
}

fn apply_bound_adoption(
    db: &Db,
    ctx: &ToolContext,
    pid: &str,
    qid: &str,
    binding: &Binding,
) -> Result<String, PropError> {
    let path = target(ctx)?;
    let before = std::fs::read(&path)?;
    if digest(&before) != binding.baseline_digest {
        return Err(PropError::PolicyStale);
    }
    let after =
        serde_json::to_vec_pretty(&binding.candidate).map_err(|_| PropError::PolicyConstraint)?;
    let backup = backup(ctx, pid)?;
    std::fs::create_dir_all(&backup)?;
    replace(&backup.join("before"), &before)?;
    replace(
        &backup.join("policy.json"),
        json!({"after":digest(&after),"before":digest(&before)})
            .to_string()
            .as_bytes(),
    )?;
    perform(db, ctx, pid, Some(qid), "adopt", &before, &after)?;
    Ok(pid.to_string())
}

// Evaluation 16/D15: construct pre-upgrade persisted operations for recovery
// tests only. This creates no quality receipt and cannot be called in a product
// build. New adoption tests must go through activate's independent quality gate.
#[cfg(test)]
pub(crate) fn replay_pre_quality_adoption(
    db: &Db,
    ctx: &ToolContext,
    qid: &str,
) -> Result<String, PropError> {
    let _lease = crate::tools::writeguard::repository_lock(ctx)?;
    let card = crate::cards::get_queued(db, qid, crate::cards::CardKind::Stamp)?;
    let pid = card.payload["proposal_id"]
        .as_str()
        .ok_or(PropError::PolicyConstraint)?;
    let artifact: String = db.conn().query_row("SELECT artifact_id FROM proposals WHERE id=?1 AND project_id=?2 AND status='awaiting_stamp'", params![pid,ctx.project_id], |row| row.get(0))?;
    let (_, _, _, body) = artifact_proposal_parts(db, ctx, Some(&artifact))?;
    let binding = bound(db, ctx, pid, &body)?;
    apply_bound_adoption(db, ctx, pid, qid, &binding)
}

pub(super) fn activate(
    db: &Db,
    ctx: &ToolContext,
    pid: &str,
    qid: &str,
) -> Result<String, PropError> {
    let started = std::time::Instant::now();
    let result = (|| {
        let _lease = crate::tools::writeguard::repository_lock(ctx)?;
        let (status, artifact): (String, String) = db.conn().query_row(
            "SELECT status,artifact_id FROM proposals WHERE id=?1 AND project_id=?2",
            params![pid, ctx.project_id],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )?;
        if status != "awaiting_stamp" {
            return Err(PropError::BadState(status));
        }
        let (surface, destination, _, body) = artifact_proposal_parts(db, ctx, Some(&artifact))?;
        if surface != "pack_copy" || destination != TARGET {
            return Err(PropError::PolicyConstraint);
        }
        let binding = bound(db, ctx, pid, &body)?;
        require_independent_quality(db, ctx, pid)?;
        apply_bound_adoption(db, ctx, pid, qid, &binding)
    })();
    crate::diag::note(
        if result.is_err() {
            crate::diag::CLASS_REJECT
        } else {
            crate::diag::CLASS_JUDGE
        },
        result.is_err(),
        Some(&ctx.project_id),
        Some("owner"),
        None,
        None,
        "policy_adoption",
        match &result {
            Ok(_) => "owner_applied",
            Err(PropError::PolicyQualityUnverified) => "policy_quality_unverified",
            Err(_) => "revalidation_refused",
        },
        started,
    );
    result
}

pub(super) fn rollback(db: &Db, ctx: &ToolContext, pid: &str) -> Result<(), PropError> {
    let started = std::time::Instant::now();
    let result = (|| {
        let _lease = crate::tools::writeguard::repository_lock(ctx)?;
        let (status, author): (String, String) = db.conn().query_row(
            "SELECT status,author_agent_id FROM proposals WHERE id=?1 AND project_id=?2",
            params![pid, ctx.project_id],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )?;
        if status != "active" {
            return Err(PropError::BadState(status));
        }
        constraints(db, ctx, &author)?;
        let path = target(ctx)?;
        let backup = backup(ctx, pid)?;
        let manifest: Value = serde_json::from_slice(
            &std::fs::read(backup.join("policy.json")).map_err(|_| PropError::PolicyStale)?,
        )
        .map_err(|_| PropError::PolicyStale)?;
        let current = std::fs::read(&path)?;
        let before = std::fs::read(backup.join("before"))?;
        if manifest["after"] != digest(&current) || manifest["before"] != digest(&before) {
            return Err(PropError::PolicyStale);
        }
        perform(db, ctx, pid, None, "rollback", &current, &before)
    })();
    crate::diag::note(
        if result.is_err() {
            crate::diag::CLASS_REJECT
        } else {
            crate::diag::CLASS_JUDGE
        },
        result.is_err(),
        Some(&ctx.project_id),
        Some("owner"),
        None,
        None,
        "policy_rollback",
        if result.is_ok() {
            "owner_restored"
        } else {
            "revalidation_refused"
        },
        started,
    );
    result
}

/// Ticket 17: only the proposal owner reads and revalidates immutable bindings.
/// A supplied JSON digest or a pending-card summary is never a source receipt.
pub(crate) fn evaluation_binding(
    db: &Db,
    ctx: &ToolContext,
    pid: &str,
) -> Result<(String, PackDef, PackDef), PropError> {
    let artifact: String = db.conn().query_row(
        "SELECT artifact_id FROM proposals WHERE id=?1 AND project_id=?2 AND surface='pack_copy' AND status='awaiting_stamp'",
        params![pid, ctx.project_id], |r| r.get(0),
    )?;
    let (_, _, _, body) = artifact_proposal_parts(db, ctx, Some(&artifact))?;
    let binding = bound(db, ctx, pid, &body)?;
    let baseline: PackDef = serde_json::from_slice(&std::fs::read(target(ctx)?)?)
        .map_err(|_| PropError::PolicyStale)?;
    let fingerprint = crate::evaluation::config::digest(&binding)?;
    Ok((fingerprint, baseline, binding.candidate))
}

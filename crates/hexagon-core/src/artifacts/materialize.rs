//! D07 / reliability 16: filesystem replacement and SQLite are NOT one
//! transaction. Durable intent + content evidence makes restart decisions
//! provable. Ambiguity costs manual recovery, never an unreviewed overwrite.
use super::{ArtifactError, ArtifactMeta, Tier};
use crate::db::Db;
use crate::tools::{repo_path, writeguard, ToolContext};
use crate::trace::EventKind;
use rusqlite::OptionalExtension;
use serde::{Deserialize, Serialize};
use serde_json::json;
use sha2::{Digest, Sha256};
use std::io::Write;
use std::path::Path;

#[derive(Serialize, Deserialize)]
struct Intent {
    id: String,
    project: String,
    path: String,
    resolved: String,
    author: String,
    stage: Option<String>,
    kind: String,
    tier: String,
    version: i64,
    upstream: Option<String>,
    handoff: Option<String>,
    after_external: bool,
    content: String,
    before: Option<String>,
    target: String,
    action: Option<String>,
    #[serde(default)]
    review: Option<super::evidence::ReviewBinding>,
}

#[derive(Debug, PartialEq, Eq)]
enum Recovery {
    Register,
    Abort,
    Pending,
}

// D07: unknown bytes/identity never attest completion. False negative costs
// recovery; false positive would register someone else's body as our delivery.
fn classify(
    before: Option<&str>,
    target: &str,
    current: Result<Option<&str>, ()>,
    same_path: bool,
) -> Recovery {
    if !same_path {
        return Recovery::Pending;
    }
    match current {
        Ok(Some(actual)) if actual == target => Recovery::Register,
        Ok(actual) if actual == before => Recovery::Abort,
        _ => Recovery::Pending,
    }
}

fn checkpoint(_point: &str) -> std::io::Result<()> {
    #[cfg(test)]
    if FAULT.with(|cell| cell.get() == Some(_point)) {
        return Err(std::io::Error::other(format!(
            "synthetic materialization fault: {_point}"
        )));
    }
    Ok(())
}

fn note(intent: &Intent, branch: &str, rejected: bool, start: std::time::Instant) {
    crate::diag::note(
        if rejected {
            crate::diag::CLASS_REJECT
        } else {
            crate::diag::CLASS_JUDGE
        },
        rejected,
        Some(&intent.project),
        Some(&intent.author),
        intent.stage.as_deref(),
        None,
        "artifact_materialization",
        branch,
        start,
    );
}

fn matching_target(root: &Path, intent: &Intent) -> Result<bool, ArtifactError> {
    let path = repo_path(root, &format!(".hexagon/{}", intent.path))?;
    Ok(path.to_string_lossy() == intent.resolved
        && writeguard::digest(&path)?.as_deref() == Some(&intent.target))
}

fn register(db: &Db, root: &Path, intent: &Intent, recovered: bool) -> Result<(), ArtifactError> {
    // Caller holds the same repository write lease as the tool registry.
    if !matching_target(root, intent)? {
        return Err(ArtifactError::MaterializationPending);
    }
    let tx = db.conn().unchecked_transaction()?;
    let pending: bool = db.conn().query_row(
        "SELECT state='pending' FROM artifact_materializations WHERE id=?1",
        [&intent.id],
        |r| r.get(0),
    )?;
    if !pending {
        return Ok(());
    }
    db.conn().execute("UPDATE artifacts SET status='superseded' WHERE project_id=?1 AND path=?2 AND status='valid'",rusqlite::params![intent.project,intent.path])?;
    checkpoint("supersede")?;
    db.conn().execute("INSERT INTO artifacts(id,project_id,path,kind,tier,stage_run_id,author_agent_id,version,status,upstream_id,content,content_digest) VALUES (?1,?2,?3,?4,?5,?6,?7,?8,'valid',?9,?10,?11)",rusqlite::params![intent.id,intent.project,intent.path,intent.kind,intent.tier,intent.stage,intent.author,intent.version,intent.upstream,intent.content,intent.target])?;
    checkpoint("row")?;
    db.append_event(&intent.project,EventKind::ArtifactDelivered,
        json!({"artifact_id":intent.id,"path":intent.path,"kind":intent.kind,"tier":intent.tier,"version":intent.version,"handoff":intent.handoff,"after_external":intent.after_external,"content_digest":intent.target,"recovered":recovered}),Some(&intent.author),intent.stage.as_deref())?;
    checkpoint("event")?;
    // D08: production review artifacts and scenario submissions share this
    // transaction. Recovery cannot duplicate a review or invent new target bytes.
    if let Some(review) = &intent.review {
        db.append_event(&intent.project,
            if review.verdict=="pass" {EventKind::ReviewPassed} else {EventKind::ReviewRejected},
            json!({"artifact_kind":review.evidence.kind,"artifact_id":review.evidence.id,"artifact_path":review.evidence.path,"reviewer":review.reviewer,"review_artifact":intent.id,"stage_run_id":review.evidence.stage,"evidence":review.evidence}),
            Some(&intent.author),intent.stage.as_deref())?;
    }
    db.conn().execute(
        "UPDATE artifact_materializations SET state='complete',reason=NULL WHERE id=?1",
        [&intent.id],
    )?;
    tx.commit()?;
    checkpoint("committed")?;
    Ok(())
}

pub(super) fn deliver(
    db: &Db,
    ctx: &ToolContext,
    path: &str,
    content: &str,
    meta: &ArtifactMeta,
    tier: Tier,
) -> Result<String, ArtifactError> {
    let _lease = if ctx.write_lease.is_none() {
        Some(writeguard::repository_lock(ctx)?)
    } else {
        None
    };
    let root = ctx.repo_root.canonicalize()?;
    let target = repo_path(&root, &format!(".hexagon/{}", path.trim_start_matches('/')))?;
    // Reliability 16 review: /x and ./x formerly replaced the same body while
    // storing different DB identities, bypassing the pending-delivery gate.
    // Use resolved identity for both files and records, including symlink aliases.
    let artifact_root = repo_path(&root, ".hexagon")?;
    let canonical_path = target
        .strip_prefix(&artifact_root)
        .map_err(|_| crate::tools::ToolError::PathEscape(path.into()))?
        .components()
        .map(|c| c.as_os_str().to_string_lossy())
        .collect::<Vec<_>>()
        .join("/");
    let path = canonical_path.as_str();
    let review = super::evidence::prepare_review(db, ctx, meta)?;
    let tx = db.conn().unchecked_transaction()?;
    let blocked: bool = db.conn().query_row("SELECT EXISTS(SELECT 1 FROM artifact_materializations WHERE project_id=?1 AND path=?2 AND state='pending')",rusqlite::params![ctx.project_id,path],|r| r.get(0))?;
    if blocked {
        return Err(ArtifactError::MaterializationPending);
    }
    let version = db.conn().query_row(
        "SELECT COALESCE(MAX(version),0)+1 FROM artifacts WHERE project_id=?1 AND path=?2",
        rusqlite::params![ctx.project_id, path],
        |r| r.get(0),
    )?;
    let upstream = meta.upstream.as_deref().and_then(|up| db.conn().query_row("SELECT id FROM artifacts WHERE project_id=?1 AND path=?2 ORDER BY version DESC LIMIT 1",rusqlite::params![ctx.project_id,up],|r|r.get(0)).ok());
    let intent = Intent {
        id: format!("art{}", db.next_id("art")?),
        project: ctx.project_id.clone(),
        path: path.into(),
        resolved: target.to_string_lossy().into(),
        author: ctx.agent_id.clone(),
        stage: ctx.stage_run_id.clone(),
        kind: meta.kind.clone(),
        tier: tier.as_str().into(),
        version,
        upstream,
        handoff: meta.handoff.clone(),
        after_external: crate::provenance::tainted(db, &ctx.agent_id, ctx.stage_run_id.as_deref()),
        content: content.into(),
        before: writeguard::digest(&target)?,
        target: format!("{:x}", Sha256::digest(content.as_bytes())),
        action: ctx.action_key.clone(),
        review,
    };
    let encoded =
        serde_json::to_string(&intent).map_err(|e| std::io::Error::other(e.to_string()))?;
    db.conn().execute("INSERT INTO artifact_materializations(id,project_id,path,state,intent_json) VALUES (?1,?2,?3,'pending',?4)",rusqlite::params![intent.id,ctx.project_id,path,encoded])?;
    tx.commit()?;
    checkpoint("intent")?;
    let started = std::time::Instant::now();
    let parent = target
        .parent()
        .ok_or_else(|| std::io::Error::other("artifact has no parent"))?;
    std::fs::create_dir_all(parent)?;
    let temporary = parent.join(format!(".hexagon-{}.tmp", intent.id));
    let mut file = std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&temporary)?;
    file.write_all(content.as_bytes())?;
    if let Ok(metadata) = target.metadata() {
        file.set_permissions(metadata.permissions())?;
    }
    file.sync_all()?;
    drop(file);
    checkpoint("temporary")?;
    // External editors are outside our lease. Detect observed changes before
    // replacement and after it; do not claim atomicity against those processes.
    if writeguard::digest(&target)? != intent.before {
        note(&intent, "target_changed_before_replace", true, started);
        return Err(ArtifactError::MaterializationPending);
    }
    std::fs::rename(&temporary, &target)?;
    // Directory handles use platform-specific open flags on Windows; the
    // durable intent remains the recovery authority if rename durability is lost.
    #[cfg(unix)]
    std::fs::File::open(parent)?.sync_all()?;
    checkpoint("replace")?;
    register(db, &root, &intent, false)?;
    note(&intent, "registered", false, started);
    Ok(intent.id)
}

// Read-only host receipt: neither re-run materialization nor resubmit proposals.
pub(crate) fn receipt(
    db: &Db,
    ctx: &ToolContext,
    action: &str,
) -> Result<Option<serde_json::Value>, ArtifactError> {
    let record:Option<String>=db.conn().query_row(
        "SELECT m.intent_json FROM artifact_materializations m JOIN artifacts a ON a.id=m.id AND a.project_id=m.project_id
         WHERE m.project_id=?1 AND m.state='complete' AND json_extract(m.intent_json,'$.action')=?2
         AND a.content_digest=json_extract(m.intent_json,'$.target') AND a.version=json_extract(m.intent_json,'$.version')",
        rusqlite::params![ctx.project_id,action],|r|r.get(0)).optional()?;
    let Some(record) = record else {
        return Ok(None);
    };
    let intent: Intent =
        serde_json::from_str(&record).map_err(|e| std::io::Error::other(e.to_string()))?;
    // A proposal write also submits a proposal after registration. Artifact
    // evidence alone does not prove that extra step happened (D05/D07).
    if intent.kind == "改进提案" || !matching_target(&ctx.repo_root, &intent)? {
        return Ok(None);
    }
    Ok(Some(
        json!({"artifact_id":intent.id,"kind":intent.kind,"version":intent.version}),
    ))
}

pub(crate) fn recover(db: &Db, root: &Path, project: &str) -> Result<(), ArtifactError> {
    let ctx = ToolContext {
        repo_root: root.into(),
        project_id: project.into(),
        ..Default::default()
    };
    let _lease = match writeguard::repository_lock(&ctx) {
        Ok(lease) => lease,
        Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => return Ok(()),
        Err(e) => return Err(e.into()),
    };
    let mut query = db.conn().prepare("SELECT intent_json FROM artifact_materializations WHERE project_id=?1 AND state='pending' ORDER BY id")?;
    let records = query
        .query_map([project], |r| r.get::<_, String>(0))?
        .collect::<Result<Vec<_>, _>>()?;
    for record in records {
        let intent: Intent =
            serde_json::from_str(&record).map_err(|e| std::io::Error::other(e.to_string()))?;
        let start = std::time::Instant::now();
        let observed = repo_path(root, &format!(".hexagon/{}", intent.path)).and_then(|path| {
            Ok((
                path.to_string_lossy() == intent.resolved,
                writeguard::digest(&path)?,
            ))
        });
        let decision = match &observed {
            Ok((same, digest)) => classify(
                intent.before.as_deref(),
                &intent.target,
                Ok(digest.as_deref()),
                *same,
            ),
            Err(_) => classify(intent.before.as_deref(), &intent.target, Err(()), false),
        };
        match decision {
            Recovery::Register => {
                register(db, root, &intent, true)?;
                cleanup_temporary(root, &intent);
                note(&intent, "recovered_registration", false, start);
            }
            Recovery::Abort => {
                recovery_state(db, &intent, "aborted", "original_content_present")?;
                cleanup_temporary(root, &intent);
                note(&intent, "original_content_present", false, start);
            }
            Recovery::Pending => {
                recovery_state(db, &intent, "pending", "external_change_or_unreadable")?;
                note(&intent, "external_change_or_unreadable", true, start);
            }
        }
    }
    Ok(())
}

fn recovery_state(
    db: &Db,
    intent: &Intent,
    state: &str,
    reason: &str,
) -> Result<(), ArtifactError> {
    let tx = db.conn().unchecked_transaction()?;
    let changed=db.conn().execute("UPDATE artifact_materializations SET state=?2,reason=?3 WHERE id=?1 AND state='pending' AND (state!=?2 OR reason IS NOT ?3)",rusqlite::params![intent.id,state,reason])?;
    if changed > 0 {
        db.append_event(&intent.project,EventKind::System,json!({"kind":"artifact_materialization","artifact_id":intent.id,"action_id":intent.action,"path":intent.path,"state":state,"reason":reason}),Some(&intent.author),intent.stage.as_deref())?;
    }
    tx.commit()?;
    Ok(())
}

fn cleanup_temporary(root: &Path, intent: &Intent) {
    if let Ok(path) = repo_path(root, &format!(".hexagon/{}", intent.path)) {
        if path.to_string_lossy() != intent.resolved {
            return;
        }
        if let Some(parent) = path.parent() {
            let temporary = parent.join(format!(".hexagon-{}.tmp", intent.id));
            if writeguard::digest(&temporary).ok().flatten().as_deref() == Some(&intent.target) {
                let _ = std::fs::remove_file(temporary);
            }
        }
    }
}

#[cfg(test)]
thread_local! { static FAULT: std::cell::Cell<Option<&'static str>> = const { std::cell::Cell::new(None) }; }
#[cfg(test)]
pub(crate) fn with_fault<T>(point: &'static str, run: impl FnOnce() -> T) -> T {
    struct Restore(Option<&'static str>);
    impl Drop for Restore {
        fn drop(&mut self) {
            FAULT.with(|cell| cell.set(self.0));
        }
    }
    let _restore = Restore(FAULT.with(|cell| cell.replace(Some(point))));
    run()
}

#[cfg(test)]
mod properties {
    use super::*;
    use proptest::prelude::*;
    proptest! {
        #[test]
        fn lost_identity_never_attests_bytes(before in ".{0,40}",target in ".{0,40}",current in ".{0,40}") {
            prop_assert_eq!(classify(Some(&before),&target,Ok(Some(&current)),false),Recovery::Pending);
        }
        #[test]
        fn external_content_never_becomes_our_delivery(body in ".{0,80}") {
            let before=format!("before:{body}");
            let target=format!("target:{body}");
            let external=format!("external:{body}");
            prop_assert_eq!(classify(Some(&before),&target,Ok(Some(&target)),true),Recovery::Register);
            prop_assert_eq!(classify(Some(&before),&target,Ok(Some(&external)),true),Recovery::Pending);
            prop_assert_eq!(classify(Some(&before),&target,Err(()),true),Recovery::Pending);
        }
    }
}

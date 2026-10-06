//! Governance 03: host receipts, durable single-skill intent and safe loading.
use super::governance::{hash, target_path, validate_request, Prepared};
use super::{gate, ExperienceConditions, ExperienceSource};
use crate::{db::Db, proposals::PropError, tools::ToolContext};
use rusqlite::{params, OptionalExtension};
use serde::{Deserialize, Serialize};
use std::io::Write;

#[cfg(test)]
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum ExperienceFault {
    AfterIntent,
    AfterReplace,
    BeforeDirectoryPublish,
    AfterTarget(usize),
    BeforeCommit,
    AfterCommit,
}
#[cfg(test)]
thread_local! { static NEXT_FAULT: std::cell::Cell<Option<ExperienceFault>> = const { std::cell::Cell::new(None) }; }
// Ticket 07: pause a real file observation while another DB connection approves
// a new receipt. The hook changes scheduling only, never bytes or SQL results.
#[cfg(test)]
thread_local! { static AFTER_ENTRY_FILE_READ: std::cell::RefCell<Option<Box<dyn FnOnce()>>> = const { std::cell::RefCell::new(None) }; }
#[cfg(test)]
pub fn fail_next(point: ExperienceFault) {
    NEXT_FAULT.with(|f| f.set(Some(point)));
}
#[cfg(test)]
pub(super) fn fault(point: ExperienceFault) -> Result<(), PropError> {
    if NEXT_FAULT.with(|f| {
        if f.get() == Some(point) {
            f.set(None);
            true
        } else {
            false
        }
    }) {
        return Err(PropError::Io(std::io::Error::other(
            "injected experience interruption",
        )));
    }
    Ok(())
}

const OPEN: &str = "<!-- hexagon-experience:v1 -->\n";
const CLOSE: &str = "\n<!-- /hexagon-experience -->";

// Ticket 07: snapshot, invalidation CAS and loading recheck share this receipt
// acknowledgement rule. Row equality alone is not a transaction-wide snapshot.
const RECEIPT_PENDING_SQL: &str = "(EXISTS(SELECT 1 FROM experience_operations o WHERE o.project_id=e.project_id AND json_valid(o.intent_json) AND (json_extract(o.intent_json,'$.entry.entry_id')=e.entry_id OR json_extract(o.intent_json,'$.predecessor.entry_id')=e.entry_id OR EXISTS(SELECT 1 FROM json_each(o.intent_json,'$.siblings') sibling WHERE json_extract(sibling.value,'$.entry.entry_id')=e.entry_id OR json_extract(sibling.value,'$.predecessor.entry_id')=e.entry_id)) AND o.state!='complete') OR NOT EXISTS(SELECT 1 FROM proposals p WHERE p.id=e.proposal_id AND p.status='active'))";

#[derive(Debug, Clone, Serialize, Deserialize, ts_rs::TS)]
#[serde(deny_unknown_fields)]
#[ts(export, export_to = "../../../ui/src/gen/")]
pub struct ExperienceEntry {
    pub schema_version: u32,
    pub entry_id: String,
    pub revision: u32,
    pub project_id: String,
    pub skill: String,
    pub body: String,
    pub notes: String,
    pub conditions: ExperienceConditions,
    pub sources: Vec<ExperienceSource>,
}

#[derive(Debug, Clone, Serialize, ts_rs::TS)]
#[ts(export, export_to = "../../../ui/src/gen/")]
pub struct ExperienceEntryView {
    pub entry: ExperienceEntry,
    #[ts(type = "'active' | 'revoked' | 'superseded' | 'changed' | 'pending_recovery'")]
    pub state: String,
    #[ts(
        type = "'matched' | 'inactive' | 'missing_role' | 'role_mismatch' | 'missing_stage' | 'stage_mismatch' | 'missing_paths' | 'path_mismatch' | 'entry_limit' | 'load_limit'"
    )]
    pub match_reason: String,
    pub recovery_pending: bool,
    pub source_count: u32,
}

#[derive(Debug, Serialize, Deserialize)]
pub(super) struct Intent {
    prepared: Prepared,
    before: String,
    after: String,
    pub entry: ExperienceEntry,
    record: String,
    #[serde(default)]
    previous: Option<ExperienceEntry>,
    #[serde(default)]
    predecessor: Option<ExperienceEntry>,
    #[serde(default)]
    siblings: Vec<Intent>,
    #[serde(default)]
    new_file: bool,
}

pub(super) fn encode<T: Serialize>(value: &T) -> Result<String, PropError> {
    serde_json::to_string(value)
        .map(|s| s.replace('`', "\\u0060").replace('<', "\\u003c"))
        .map_err(|e| PropError::Rejected(e.to_string()))
}
pub(super) fn block(record: &str) -> String {
    format!("{OPEN}{record}{CLOSE}\n")
}
// Governance 05: exact semantic key only; internal whitespace, case and code
// are deliberately untouched. Conditions are sets, not ordered preferences.
fn content_key(
    body: &str,
    notes: &str,
    conditions: &ExperienceConditions,
) -> (String, String, ExperienceConditions) {
    fn text(value: &str) -> String {
        value
            .replace("\r\n", "\n")
            .replace('\r', "\n")
            .trim()
            .to_owned()
    }
    let mut normalized = conditions.clone();
    for values in [
        &mut normalized.roles,
        &mut normalized.stages,
        &mut normalized.paths,
    ] {
        values.sort();
        values.dedup();
    }
    (text(body), text(notes), normalized)
}

fn conflict() -> PropError {
    PropError::Rejected("experience target or approval changed; reread and resubmit".into())
}

fn bound(db: &Db, ctx: &ToolContext, proposal: &str, body: &str) -> Result<Prepared, PropError> {
    let saved: Option<(String,String)> = db.conn().query_row(
        "SELECT payload_json,artifact_digest FROM experience_proposals WHERE project_id=?1 AND proposal_id=?2",
        params![ctx.project_id,proposal], |r| Ok((r.get(0)?,r.get(1)?)),
    ).optional()?;
    let Some((payload, digest)) = saved else {
        return Err(PropError::UngovernedExperience);
    };
    if digest != hash(body) {
        return Err(conflict());
    }
    let p: Prepared =
        serde_json::from_str(&payload).map_err(|_| PropError::UngovernedExperience)?;
    if p.schema_version != 1 {
        return Err(PropError::UngovernedExperience);
    }
    validate_request(
        &p.request,
        super::limits::read(db, &ctx.project_id)?.entry_chars,
    )?;
    if p.request.targets.is_empty() {
        return Err(PropError::Rejected(
            "experience target selection is pending".into(),
        ));
    }
    let current = gate(db, ctx, Some(p.qualification.review_event))?;
    if let Some(legacy) = &p.legacy {
        super::curation::validate_range(ctx, legacy)?;
        if super::curation::historical(db, ctx, p.qualification.review_event)? != p.qualification {
            return Err(PropError::StaleExperience);
        }
    } else if current.delivered || current.qualification.as_ref() != Some(&p.qualification) {
        return Err(PropError::StaleExperience);
    }
    if let Some(role) = &p.role_skill {
        super::role_skill::revalidate(db, ctx, role)?;
    } else if p
        .request
        .targets
        .iter()
        .any(|target| !current.skills.contains(&target.skill))
    {
        return Err(conflict());
    }
    Ok(p)
}

pub(super) fn apply(
    db: &Db,
    ctx: &ToolContext,
    proposal: &str,
    body: &str,
) -> Result<bool, PropError> {
    let started = std::time::Instant::now();
    let result = apply_inner(db, ctx, proposal, body);
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
        "experience_apply",
        if result.is_ok() {
            "host_bound_entry"
        } else {
            "binding_or_write_failed"
        },
        started,
    );
    result
}

fn apply_inner(db: &Db, ctx: &ToolContext, proposal: &str, body: &str) -> Result<bool, PropError> {
    let _lease = if ctx.write_lease.is_none() {
        Some(crate::tools::writeguard::repository_lock(ctx)?)
    } else {
        None
    };
    let prepared = bound(db, ctx, proposal, body)?;
    let existing: Option<String> = db
        .conn()
        .query_row(
            "SELECT state FROM experience_operations WHERE proposal_id=?1 AND project_id=?2",
            params![proposal, ctx.project_id],
            |r| r.get(0),
        )
        .optional()?;
    if let Some(state) = existing {
        return if state == "complete" {
            Ok(true)
        } else {
            Err(PropError::Rejected(
                "experience operation needs recovery".into(),
            ))
        };
    }
    let mut changes = (0..prepared.request.targets.len())
        .map(|index| build_intent(db, ctx, prepared.clone(), index))
        .collect::<Result<Vec<_>, _>>()?;
    let mut intent = changes.remove(0);
    intent.siblings = changes;
    db.conn().execute("INSERT INTO experience_operations(proposal_id,project_id,author,state,intent_json) VALUES(?1,?2,?3,'pending',?4)",params![proposal,ctx.project_id,ctx.agent_id,encode(&intent)?])?;
    #[cfg(test)]
    fault(ExperienceFault::AfterIntent)?;
    // Governance 12: ordinal exists solely for deterministic boundary faults.
    #[cfg_attr(not(test), allow(clippy::unused_enumerate_index))]
    for (_index, change) in all_changes(&intent).enumerate() {
        replace(ctx, change)?;
        #[cfg(test)]
        {
            fault(ExperienceFault::AfterReplace)?;
            fault(ExperienceFault::AfterTarget(_index))?;
        }
    }
    finish(db, ctx, proposal, &intent)?;
    Ok(true)
}

pub(super) fn all_changes(intent: &Intent) -> impl Iterator<Item = &Intent> {
    std::iter::once(intent).chain(intent.siblings.iter())
}
fn build_intent(
    db: &Db,
    ctx: &ToolContext,
    prepared: Prepared,
    index: usize,
) -> Result<Intent, PropError> {
    let target = &prepared.request.targets[index];
    let new_file = prepared
        .role_skill
        .as_ref()
        .is_some_and(|role| role.new_file);
    let before = if new_file {
        let path = super::role_skill::prospective(ctx, &target.skill)?;
        if path.parent().is_some_and(|p| p.exists()) || target.expected_digest != "absent" {
            return Err(conflict());
        }
        String::new()
    } else {
        let before = std::fs::read_to_string(target_path(ctx, &target.skill)?)?;
        if hash(&before) != target.expected_digest {
            return Err(conflict());
        }
        before
    };
    let source = &prepared.qualification;
    let source = ExperienceSource {
        review_event: source.review_event,
        activation: source.activation,
        artifact_id: source.evidence.id.clone(),
        author: source.evidence.author.clone(),
        artifact_version: source.evidence.version,
        artifact_digest: source.evidence.digest.clone(),
    };
    let key = content_key(
        &prepared.request.body,
        &prepared.request.notes,
        &prepared.request.conditions,
    );
    let views = if new_file {
        Vec::new()
    } else {
        entries(db, ctx, &target.skill)?
    };
    let mut previous_entry = None;
    let mut predecessor = None;
    let duplicate = views
        .iter()
        .find(|v| content_key(&v.entry.body, &v.entry.notes, &v.entry.conditions) == key);
    let (entry, previous) = if let Some(change) = &target.change {
        use super::ExperienceChangeKind;
        let view = views
            .iter()
            .find(|v| v.entry.entry_id == change.entry_id)
            .ok_or_else(conflict)?;
        if view.entry.revision != change.expected_revision || view.recovery_pending {
            return Err(conflict());
        }
        if duplicate.is_some_and(|other| other.entry.entry_id != change.entry_id) {
            return Err(conflict());
        }
        match change.kind {
            ExperienceChangeKind::Revise | ExperienceChangeKind::Replace
                if !matches!(view.state.as_str(), "active" | "changed") =>
            {
                return Err(conflict())
            }
            ExperienceChangeKind::Reactivate
                if !matches!(view.state.as_str(), "revoked" | "changed") =>
            {
                return Err(conflict())
            }
            _ => {}
        }
        let old_block = current_block(&before, &change.entry_id).ok_or_else(conflict)?;
        let mut entry = view.entry.clone();
        previous_entry = Some(entry.clone());
        if change.kind == ExperienceChangeKind::Replace {
            predecessor = Some(entry.clone());
            entry.entry_id = format!("exp{}", db.next_id("experience-entry")?);
            entry.revision = 1;
        } else {
            entry.revision = entry.revision.checked_add(1).ok_or_else(conflict)?;
        }
        entry.body = prepared.request.body.clone();
        entry.notes = prepared.request.notes.clone();
        entry.conditions = prepared.request.conditions.clone();
        entry.sources = vec![source];
        (entry, Some(old_block))
    } else if let Some(view) = duplicate {
        // Governance 05: a matching inactive identity cannot be bypassed by
        // minting a new id; only an explicit reviewed change may reactivate it.
        if view.state != "active" {
            return Err(PropError::Rejected(format!(
                "experience {} is {}; reviewed reactivation required",
                view.entry.entry_id, view.state
            )));
        }
        previous_entry = Some(view.entry.clone());
        let previous = block(&encode(&view.entry)?);
        let mut entry = view.entry.clone();
        if !entry.sources.contains(&source) {
            entry.sources.push(source);
        }
        (entry, Some(previous))
    } else {
        (
            ExperienceEntry {
                schema_version: 1,
                entry_id: format!("exp{}", db.next_id("experience-entry")?),
                revision: 1,
                project_id: ctx.project_id.clone(),
                skill: target.skill.clone(),
                body: prepared.request.body.clone(),
                notes: prepared.request.notes.clone(),
                conditions: prepared.request.conditions.clone(),
                sources: vec![source],
            },
            None,
        )
    };
    let record = encode(&entry)?;
    let after = if let Some(previous) = previous {
        if before.matches(&previous).count() != 1 {
            return Err(conflict());
        }
        let replacement = if predecessor.is_some() {
            format!("{previous}{}", block(&record))
        } else {
            block(&record)
        };
        before.replacen(&previous, &replacement, 1).replace(
            &format!("<!-- hexagon-stopped-experience:{} -->\n", entry.entry_id),
            "",
        )
    } else {
        let heading = if super::legacy_loading_view(&before).1 == 0 {
            "\n## 经验\n"
        } else {
            "\n"
        };
        let base = if new_file {
            super::new_skill(&prepared.role_skill.as_ref().ok_or_else(conflict)?.role, "")
        } else {
            format!("{before}{heading}")
        };
        format!("{base}{}", block(&record))
    };
    Ok(Intent {
        prepared,
        before,
        after,
        entry,
        record,
        previous: previous_entry,
        predecessor,
        siblings: Vec::new(),
        new_file,
    })
}

fn replace(ctx: &ToolContext, intent: &Intent) -> Result<(), PropError> {
    if intent.new_file {
        let path = super::role_skill::prospective(ctx, &intent.entry.skill)?;
        let parent = path.parent().ok_or_else(conflict)?;
        std::fs::create_dir_all(parent.parent().ok_or_else(conflict)?)?;
        // Governance 14 review: publishing an empty directory first left an
        // unrecoverable crash gap. Prepare the whole directory, then publish
        // exclusively; a foreign target (even empty) must never be replaced.
        let staging = tempfile::tempdir_in(parent.parent().ok_or_else(conflict)?)?;
        let staged_path = staging.path().join("SKILL.md");
        let mut file = std::fs::File::create(&staged_path)?;
        file.write_all(intent.after.as_bytes())?;
        file.sync_all()?;
        #[cfg(test)]
        fault(ExperienceFault::BeforeDirectoryPublish)?;
        publish_directory(staging.path(), parent)?;
        return Ok(());
    }
    replace_text(ctx, &intent.entry.skill, &intent.before, &intent.after)
}

fn publish_directory(from: &std::path::Path, to: &std::path::Path) -> Result<(), PropError> {
    use std::os::unix::ffi::OsStrExt;
    let from = std::ffi::CString::new(from.as_os_str().as_bytes()).map_err(|_| conflict())?;
    let to = std::ffi::CString::new(to.as_os_str().as_bytes()).map_err(|_| conflict())?;
    // Both platform primitives atomically reject an existing destination.
    #[cfg(target_os = "macos")]
    let result = unsafe { libc::renamex_np(from.as_ptr(), to.as_ptr(), libc::RENAME_EXCL) };
    #[cfg(target_os = "linux")]
    let result = unsafe {
        libc::renameat2(
            libc::AT_FDCWD,
            from.as_ptr(),
            libc::AT_FDCWD,
            to.as_ptr(),
            libc::RENAME_NOREPLACE,
        )
    };
    #[cfg(not(any(target_os = "macos", target_os = "linux")))]
    let result = -1;
    if result != 0 {
        return Err(PropError::Io(std::io::Error::last_os_error()));
    }
    Ok(())
}

pub(super) fn replace_text(
    ctx: &ToolContext,
    skill: &str,
    before: &str,
    after: &str,
) -> Result<(), PropError> {
    let path = target_path(ctx, skill)?;
    if std::fs::read_to_string(&path)? != before {
        return Err(conflict());
    }
    let mut temp = tempfile::NamedTempFile::new_in(path.parent().ok_or_else(conflict)?)?;
    temp.as_file()
        .set_permissions(path.metadata()?.permissions())?;
    temp.write_all(after.as_bytes())?;
    temp.as_file().sync_all()?;
    // External editors do not share our lease. Recheck before replacement and
    // retain the durable before/after intent if the file changes afterward.
    if std::fs::read_to_string(&path)? != before {
        return Err(conflict());
    }
    temp.persist(&path).map_err(|e| PropError::Io(e.error))?;
    if std::fs::read_to_string(&path)? != after {
        return Err(conflict());
    }
    Ok(())
}

fn finish(db: &Db, ctx: &ToolContext, proposal: &str, intent: &Intent) -> Result<(), PropError> {
    for intent in all_changes(intent) {
        let path = target_path(ctx, &intent.entry.skill)?;
        if std::fs::read_to_string(&path)? != intent.after {
            return Err(conflict());
        }
        if let Some(previous) = &intent.previous {
            let (revision,state,record):(u32,String,String) = db.conn().query_row("SELECT revision,state,record_json FROM experience_entries WHERE project_id=?1 AND skill=?2 AND entry_id=?3",params![ctx.project_id,previous.skill,previous.entry_id],|r|Ok((r.get(0)?,r.get(1)?,r.get(2)?)))?;
            let reactivating = intent.prepared.request.targets.iter().any(|t| {
                t.skill == intent.entry.skill
                    && t.change
                        .as_ref()
                        .is_some_and(|c| c.kind == super::ExperienceChangeKind::Reactivate)
            });
            if revision != previous.revision
                || record != encode(previous)?
                || (state == "revoked" && !reactivating)
                || state == "superseded"
            {
                return Err(conflict());
            }
        }
    }
    let tx = db.conn().unchecked_transaction()?;
    for intent in all_changes(intent) {
        if let Some(predecessor) = &intent.predecessor {
            db.conn().execute("UPDATE experience_entries SET state='superseded' WHERE project_id=?1 AND skill=?2 AND entry_id=?3 AND revision=?4",params![ctx.project_id,predecessor.skill,predecessor.entry_id,predecessor.revision])?;
        }
        db.conn().execute("INSERT INTO experience_entries(project_id,skill,entry_id,revision,state,record_json,proposal_id) VALUES(?1,?2,?3,?4,'active',?5,?6) ON CONFLICT(project_id,skill,entry_id) DO UPDATE SET record_json=excluded.record_json,proposal_id=excluded.proposal_id,revision=excluded.revision,state='active',invalidated=0",params![ctx.project_id,intent.entry.skill,intent.entry.entry_id,intent.entry.revision,intent.record,proposal])?;
    }
    if let Some(role) = &intent.prepared.role_skill {
        super::role_skill::commit(db, ctx, role)?;
    }
    db.conn().execute(
        "UPDATE experience_operations SET state='complete' WHERE proposal_id=?1 AND project_id=?2",
        params![proposal, ctx.project_id],
    )?;
    #[cfg(test)]
    fault(ExperienceFault::BeforeCommit)?;
    tx.commit()?;
    #[cfg(test)]
    fault(ExperienceFault::AfterCommit)?;
    Ok(())
}

fn current_block(text: &str, id: &str) -> Option<String> {
    let identities = block_identities(text)?;
    if identities.get(id) != Some(&1) {
        return None;
    }
    let mut rest = text;
    while let Some(start) = rest.find(OPEN) {
        let raw = &rest[start + OPEN.len()..];
        let end = raw.find(CLOSE)?;
        let entry: ExperienceEntry = serde_json::from_str(&raw[..end]).ok()?;
        if entry.entry_id == id {
            let end = start + OPEN.len() + end + CLOSE.len();
            let end = end + usize::from(rest[end..].starts_with('\n'));
            return Some(rest[start..end].into());
        }
        rest = &raw[end + CLOSE.len()..];
    }
    None
}

// Governance 08: duplicate ids or uncertain block boundaries cannot retain a
// receipt merely because one old byte sequence still exists elsewhere.
fn block_identities(text: &str) -> Option<std::collections::HashMap<String, usize>> {
    let mut ids = std::collections::HashMap::new();
    let mut rest = text;
    while let Some(start) = rest.find("<!-- hexagon-experience") {
        rest = rest[start..].strip_prefix(OPEN)?;
        let end = rest.find(CLOSE)?;
        let entry: ExperienceEntry = serde_json::from_str(&rest[..end]).ok()?;
        if entry.schema_version != 1 {
            return None;
        }
        *ids.entry(entry.entry_id).or_insert(0) += 1;
        rest = &rest[end + CLOSE.len()..];
    }
    Some(ids)
}

fn stale_observation(ctx: &ToolContext) -> PropError {
    crate::diag::note(
        crate::diag::CLASS_REJECT,
        true,
        Some(&ctx.project_id),
        Some(&ctx.agent_id),
        ctx.stage_run_id.as_deref(),
        None,
        "experience_integrity",
        "observation_version_changed",
        std::time::Instant::now(),
    );
    PropError::StaleExperience
}

pub fn entries(
    db: &Db,
    ctx: &ToolContext,
    skill: &str,
) -> Result<Vec<ExperienceEntryView>, PropError> {
    // 2026-10-06 / competitor-improvements ticket 07: file-first observation
    // paired old bytes with a newly approved receipt and permanently invalidated
    // that new revision. Snapshot the receipt before reading, then CAS only that
    // exact reviewed record (same revision can gain sources/change proposal).
    // False rejection costs one reread; false acceptance injects unreviewed advice.
    // Prefer a stale-observation error, never revoke a newer receipt or silently
    // clear the sticky invalidation after externally restored bytes.
    // Governance 04 regression: a committed receipt may precede the proposal
    // acknowledgement. Keep it pending until recovery reconciles both facts.
    let mut query = db.conn().prepare(&format!("SELECT record_json,state,invalidated,{RECEIPT_PENDING_SQL},proposal_id FROM experience_entries e WHERE project_id=?1 AND skill=?2 ORDER BY entry_id"))?;
    let records = query
        .query_map(params![ctx.project_id, skill], |r| {
            Ok((
                r.get::<_, String>(0)?,
                r.get::<_, String>(1)?,
                r.get::<_, bool>(2)?,
                r.get::<_, bool>(3)?,
                r.get::<_, String>(4)?,
            ))
        })?
        .collect::<Result<Vec<_>, _>>()?;
    let candidate = super::role_skill::prospective(ctx, skill)?;
    let text = if !candidate.exists() {
        // Governance 16/P1: missing bytes are an observed integrity change,
        // not a reason to discard the host receipt or refuse an owner stop.
        // An absent never-governed skill simply has no entries.
        String::new()
    } else {
        std::fs::read_to_string(target_path(ctx, skill)?)?
    };
    let identities = block_identities(&text);
    #[cfg(test)]
    AFTER_ENTRY_FILE_READ.with(|hook| {
        if let Some(hook) = hook.borrow_mut().take() {
            hook();
        }
    });
    let context = super::matching::context(db, ctx)?;
    let mut result = Vec::new();
    for (record, state, invalidated, pending, proposal) in records {
        let entry: ExperienceEntry =
            serde_json::from_str(&record).map_err(|_| PropError::UngovernedExperience)?;
        let control_pending: bool = db.conn().query_row("SELECT EXISTS(SELECT 1 FROM experience_controls WHERE project_id=?1 AND state!='complete' AND ((skill=?2 AND entry_id=?3) OR EXISTS(SELECT 1 FROM json_each(payload_json,'$.siblings') sibling WHERE json_extract(sibling.value,'$.skill')=?2 AND json_extract(sibling.value,'$.rollback.entry.entry_id')=?3)))",params![ctx.project_id,skill,entry.entry_id],|r|r.get(0))?;
        let broken = identities
            .as_ref()
            .is_none_or(|ids| ids.get(&entry.entry_id) != Some(&1))
            || text.matches(&block(&record)).count() != 1;
        if !pending && !control_pending && !invalidated && broken {
            // Governance 08 regression: observing a mutation permanently breaks
            // this receipt; putting the original bytes back is not a new review.
            let changed = db.conn().execute(&format!("UPDATE experience_entries AS e SET invalidated=1 WHERE project_id=?1 AND skill=?2 AND entry_id=?3 AND revision=?4 AND record_json=?5 AND proposal_id=?6 AND state=?7 AND invalidated=0 AND {RECEIPT_PENDING_SQL}=?8"),params![ctx.project_id,skill,entry.entry_id,entry.revision,record,proposal,state,pending])?;
            if changed == 0 {
                return Err(stale_observation(ctx));
            }
            crate::diag::note(
                crate::diag::CLASS_REJECT,
                true,
                Some(&ctx.project_id),
                Some(&ctx.agent_id),
                ctx.stage_run_id.as_deref(),
                None,
                "experience_integrity",
                "observed_changed",
                std::time::Instant::now(),
            );
        }
        // Even a matching file must not load an obsolete receipt after another
        // connection revokes/revises it or begins an unacknowledged operation.
        // Ticket 07 pending regression: identical file/receipt bytes can still
        // have a newly pending write intent. This is a recheck, not a write retry.
        let current: bool = db.conn().query_row(
            &format!("SELECT EXISTS(SELECT 1 FROM experience_entries e WHERE project_id=?1 AND skill=?2 AND entry_id=?3 AND revision=?4 AND record_json=?5 AND proposal_id=?6 AND state=?7 AND invalidated=?8 AND {RECEIPT_PENDING_SQL}=?9)"),
            params![ctx.project_id,skill,entry.entry_id,entry.revision,record,proposal,state,invalidated || (!pending && !control_pending && broken),pending], |row| row.get(0),
        )?;
        if !current {
            return Err(stale_observation(ctx));
        }
        let state = if state == "revoked" || state == "superseded" {
            state
        } else if pending || control_pending {
            "pending_recovery".into()
        } else if invalidated || broken {
            "changed".into()
        } else {
            state
        };
        let match_reason: String = if state == "active" {
            super::matching::reason(&entry.conditions, &context)
        } else {
            "inactive"
        }
        .into();
        crate::diag::note(
            crate::diag::CLASS_JUDGE,
            false,
            Some(&ctx.project_id),
            Some(&ctx.agent_id),
            ctx.stage_run_id.as_deref(),
            None,
            "experience_match",
            &match_reason,
            std::time::Instant::now(),
        );
        result.push(ExperienceEntryView {
            source_count: entry.sources.len() as u32,
            entry,
            state,
            match_reason,
            recovery_pending: pending || control_pending,
        });
    }
    let mut pending = db.conn().prepare(
        "SELECT intent_json FROM experience_operations WHERE project_id=?1 AND state!='complete'",
    )?;
    for raw in pending.query_map([&ctx.project_id], |r| r.get::<_, String>(0))? {
        let intent: Intent =
            serde_json::from_str(&raw?).map_err(|_| PropError::UngovernedExperience)?;
        for intent in all_changes(&intent) {
            if intent.entry.skill == skill
                && !result
                    .iter()
                    .any(|r| r.entry.entry_id == intent.entry.entry_id)
            {
                result.push(ExperienceEntryView {
                    source_count: intent.entry.sources.len() as u32,
                    entry: intent.entry.clone(),
                    state: "pending_recovery".into(),
                    match_reason: "inactive".into(),
                    recovery_pending: true,
                });
            }
        }
    }
    let limits = super::limits::read(db, &ctx.project_id)?;
    let eligible = result
        .iter()
        .filter(|v| v.match_reason == "matched")
        .map(|v| v.entry.clone())
        .collect();
    let (_, selected) = super::limits::plan(eligible, &limits);
    for view in &mut result {
        if view.match_reason == "matched" && !selected.contains(&view.entry.entry_id) {
            view.match_reason = if super::limits::entry_size(
                &view.entry.body,
                &view.entry.notes,
                &view.entry.conditions,
            ) > limits.entry_chars as usize
            {
                "entry_limit"
            } else {
                "load_limit"
            }
            .into();
        }
    }
    Ok(result)
}

pub fn entries_view(
    db: &Db,
    ctx: &ToolContext,
    skill: &str,
) -> Result<Vec<ExperienceEntryView>, PropError> {
    let mut views = entries(db, ctx, skill)?;
    for view in &mut views {
        view.entry.sources.truncate(20);
    }
    Ok(views)
}

/// Strip managed blocks before constructing the ordinary loading view; a
/// malformed/unknown block must not fall back to unreviewed skill instructions.
pub(super) fn without_blocks(text: &str) -> String {
    let mut rest = text;
    let mut clean = String::new();
    while let Some(start) = rest.find("<!-- hexagon-experience") {
        clean.push_str(&rest[..start]);
        let Some(end) = rest[start..].find(CLOSE) else {
            return clean;
        };
        rest = &rest[start + end + CLOSE.len()..];
    }
    clean.push_str(rest);
    clean
}

pub(super) fn loading_view(
    db: &Db,
    ctx: &ToolContext,
    skill: &str,
    text: &str,
) -> Result<String, PropError> {
    let mut result = super::legacy_loading_view(&without_blocks(text)).0;
    // Builtin/global skills have no project receipt and still get legacy
    // filtering; do not require their project path to exist merely to read.
    if !ctx
        .repo_root
        .join(".hexagon/skills")
        .join(skill)
        .join("SKILL.md")
        .is_file()
    {
        return Ok(result);
    }
    let entries = entries(db, ctx, skill)?
        .into_iter()
        .filter(|view| {
            view.state == "active"
                && matches!(
                    view.match_reason.as_str(),
                    "matched" | "entry_limit" | "load_limit"
                )
        })
        .map(|view| view.entry)
        .collect();
    result.push_str(&super::limits::render(
        entries,
        &super::limits::read(db, &ctx.project_id)?,
    ));
    Ok(result)
}

#[derive(Debug, Clone, Serialize, ts_rs::TS)]
#[ts(export, export_to = "../../../ui/src/gen/")]
pub struct ExperienceRecovery {
    pub operation_id: String,
    pub proposal_id: Option<String>,
    #[ts(type = "'complete' | 'conflict'")]
    pub state: String,
    #[ts(
        type = "'target_changed' | 'approval_changed' | 'invalid_material' | 'file_unavailable' | 'recovery_failed' | null"
    )]
    pub reason_code: Option<String>,
}

pub(super) fn recovery_reason(error: &PropError) -> &'static str {
    match error {
        PropError::UngovernedExperience => "invalid_material",
        PropError::StaleExperience => "approval_changed",
        PropError::Io(_) => "file_unavailable",
        PropError::Rejected(_) => "target_changed",
        _ => "recovery_failed",
    }
}

pub fn recover(db: &Db, ctx: &ToolContext) -> Result<Vec<ExperienceRecovery>, PropError> {
    let _lease = if ctx.write_lease.is_none() {
        Some(crate::tools::writeguard::repository_lock(ctx)?)
    } else {
        None
    };
    let mut query=db.conn().prepare("SELECT o.proposal_id,o.author,o.state,o.intent_json,p.status,a.path FROM experience_operations o JOIN proposals p ON p.id=o.proposal_id LEFT JOIN artifacts a ON a.id=p.artifact_id WHERE o.project_id=?1 ORDER BY o.proposal_id")?;
    let rows = query
        .query_map([&ctx.project_id], |r| {
            Ok((
                r.get::<_, String>(0)?,
                r.get::<_, String>(1)?,
                r.get::<_, String>(2)?,
                r.get::<_, String>(3)?,
                r.get::<_, String>(4)?,
                r.get::<_, String>(5)?,
            ))
        })?
        .collect::<Result<Vec<_>, _>>()?;
    let mut reports = super::controls::recover(db, ctx)?;
    for (proposal, author, state, raw, status, artifact_path) in rows {
        let queued = db
            .queued_questions(&ctx.project_id)?
            .into_iter()
            .any(|q| q.payload["proposal_id"] == proposal);
        if state == "complete" && matches!(status.as_str(), "active" | "rolled_back") && !queued {
            continue;
        }
        let started = std::time::Instant::now();
        let attempt = (|| -> Result<(), PropError> {
            if matches!(status.as_str(), "rejected" | "rolled_back") {
                return Err(conflict());
            }
            let intent: Intent =
                serde_json::from_str(&raw).map_err(|_| PropError::UngovernedExperience)?;
            let mut author_ctx = ctx.clone();
            author_ctx.agent_id = author.clone();
            if state != "complete" {
                let artifact = crate::tools::readable_repo_path(
                    &ctx.repo_root,
                    &format!(".hexagon/{artifact_path}"),
                )
                .map_err(|e| PropError::Rejected(e.to_string()))?;
                bound(
                    db,
                    &author_ctx,
                    &proposal,
                    &std::fs::read_to_string(artifact)?,
                )?;
                for change in all_changes(&intent) {
                    if change.new_file
                        && !super::role_skill::prospective(&author_ctx, &change.entry.skill)?
                            .exists()
                    {
                        replace(&author_ctx, change)?;
                        continue;
                    }
                    let current =
                        std::fs::read_to_string(target_path(&author_ctx, &change.entry.skill)?)?;
                    if current == change.before {
                        replace(&author_ctx, change)?;
                    } else if current != change.after {
                        return Err(conflict());
                    }
                }
                finish(db, &author_ctx, &proposal, &intent)?;
            }
            // Governance 04: the durable completed receipt proves the write,
            // even if the process died before the proposal/card acknowledgement.
            // Reconcile bookkeeping without replaying the side effect.
            let tx = db.conn().unchecked_transaction()?;
            db.conn().execute("UPDATE proposals SET status='active',decided_at=COALESCE(decided_at,datetime('now')) WHERE id=?1 AND project_id=?2",params![proposal,ctx.project_id])?;
            for card in db.queued_questions(&ctx.project_id)? {
                if card.payload["proposal_id"] == proposal {
                    crate::cards::answer(db, &card.id, "owner")?;
                }
            }
            let logged:bool=db.conn().query_row("SELECT EXISTS(SELECT 1 FROM events WHERE project_id=?1 AND kind='proposal_activated' AND json_extract(payload,'$.proposal_id')=?2)",params![ctx.project_id,proposal],|r|r.get(0))?;
            if !logged {
                db.append_event(&ctx.project_id,crate::trace::EventKind::ProposalActivated,serde_json::json!({"proposal_id":proposal,"by":"recovery","effective_path":format!(".hexagon/skills/{}/SKILL.md",intent.entry.skill)}),None,ctx.stage_run_id.as_deref())?;
            }
            tx.commit()?;
            Ok(())
        })();
        if attempt.is_err() && state != "complete" {
            db.conn().execute("UPDATE experience_operations SET state='conflict' WHERE proposal_id=?1 AND project_id=?2",params![proposal,ctx.project_id])?;
        }
        crate::diag::note(
            if attempt.is_ok() {
                crate::diag::CLASS_JUDGE
            } else {
                crate::diag::CLASS_REJECT
            },
            attempt.is_err(),
            Some(&ctx.project_id),
            Some(&author),
            ctx.stage_run_id.as_deref(),
            None,
            "experience_recovery",
            if attempt.is_ok() {
                "completed"
            } else {
                "preserved_conflict"
            },
            started,
        );
        reports.push(ExperienceRecovery {
            operation_id: proposal.clone(),
            proposal_id: Some(proposal),
            state: if attempt.is_ok() {
                "complete".into()
            } else {
                "conflict".into()
            },
            reason_code: attempt.err().map(|e| recovery_reason(&e).into()),
        });
    }
    Ok(reports)
}

/// Governance 11: withdraw this proposal's source contribution, never restore
/// an old file or revive a superseded predecessor. Later revisions are conflicts.
pub fn rollback_contribution(db: &Db, ctx: &ToolContext, proposal: &str) -> Result<(), PropError> {
    let started = std::time::Instant::now();
    let result = rollback_inner(db, ctx, proposal);
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
        "experience_contribution_rollback",
        if result.is_ok() {
            "withdrawn"
        } else {
            "invalid_or_conflict"
        },
        started,
    );
    result
}
fn rollback_inner(db: &Db, ctx: &ToolContext, proposal: &str) -> Result<(), PropError> {
    let raw: Option<String> = db.conn().query_row("SELECT intent_json FROM experience_operations WHERE project_id=?1 AND proposal_id=?2 AND state='complete'",params![ctx.project_id,proposal],|r|r.get(0)).optional()?;
    let Some(raw) = raw else {
        return Err(PropError::UngovernedExperience);
    };
    if ctx.agent_id != "owner" {
        return Err(conflict());
    }
    let _lease = if ctx.write_lease.is_none() {
        Some(crate::tools::writeguard::repository_lock(ctx)?)
    } else {
        None
    };
    let status: String = db.conn().query_row(
        "SELECT status FROM proposals WHERE project_id=?1 AND id=?2",
        params![ctx.project_id, proposal],
        |r| r.get(0),
    )?;
    if status == "rolled_back" {
        return Ok(());
    }
    let control_id = format!("rollback-{proposal}");
    let pending: bool = db.conn().query_row(
        "SELECT EXISTS(SELECT 1 FROM experience_controls WHERE id=?1 AND project_id=?2)",
        params![control_id, ctx.project_id],
        |r| r.get(0),
    )?;
    if pending {
        let results = super::controls::recover(db, ctx)?;
        return if results
            .iter()
            .any(|r| r.operation_id == control_id && r.state == "conflict")
        {
            Err(conflict())
        } else {
            Ok(())
        };
    }
    let intent: Intent = serde_json::from_str(&raw).map_err(|_| PropError::UngovernedExperience)?;
    let mut stops = all_changes(&intent)
        .map(|change| rollback_target(db, ctx, proposal, change))
        .collect::<Result<Vec<_>, _>>()?;
    let mut stop = stops.remove(0);
    stop.siblings = stops;
    let entry = &stop.rollback.as_ref().ok_or_else(conflict)?.entry;
    db.conn().execute("INSERT INTO experience_controls(id,project_id,skill,entry_id,kind,state,reason,payload_json) VALUES(?1,?2,?3,?4,'rollback','pending','proposal contribution withdrawal',?5)",params![control_id,ctx.project_id,entry.skill,entry.entry_id,encode(&stop)?])?;
    #[cfg(test)]
    fault(ExperienceFault::AfterIntent)?;
    let results = super::controls::recover(db, ctx)?;
    if results
        .iter()
        .any(|r| r.operation_id == control_id && r.state == "conflict")
    {
        return Err(conflict());
    }
    Ok(())
}

fn rollback_target(
    db: &Db,
    ctx: &ToolContext,
    proposal: &str,
    intent: &Intent,
) -> Result<super::controls::StopIntent, PropError> {
    let view = entries(db, ctx, &intent.entry.skill)?
        .into_iter()
        .find(|v| v.entry.entry_id == intent.entry.entry_id)
        .ok_or_else(conflict)?;
    if view.entry.revision != intent.entry.revision
        || !matches!(view.state.as_str(), "active" | "revoked")
        || view.recovery_pending
    {
        return Err(conflict());
    }
    let before = std::fs::read_to_string(target_path(ctx, &view.entry.skill)?)?;
    let old = block(&encode(&view.entry)?);
    if before.matches(&old).count() != 1 {
        return Err(conflict());
    }
    let added: Vec<_> = intent
        .entry
        .sources
        .iter()
        .filter(|source| {
            !intent.previous.as_ref().is_some_and(|previous| {
                previous.entry_id == intent.entry.entry_id
                    && previous.revision == intent.entry.revision
                    && previous.sources.contains(source)
            })
        })
        .collect();
    let mut entry = view.entry;
    entry.sources.retain(|source| !added.contains(&source));
    let state = if entry.sources.is_empty() || view.state == "revoked" {
        "revoked"
    } else {
        "active"
    };
    let retained:Option<String>=db.conn().query_row("SELECT o.proposal_id FROM experience_operations o JOIN proposals p ON p.id=o.proposal_id WHERE o.project_id=?1 AND o.proposal_id!=?2 AND o.state='complete' AND p.status='active' AND (json_extract(o.intent_json,'$.entry.entry_id')=?3 OR EXISTS(SELECT 1 FROM json_each(o.intent_json,'$.siblings') sibling WHERE json_extract(sibling.value,'$.entry.entry_id')=?3)) ORDER BY o.proposal_id DESC LIMIT 1",params![ctx.project_id,proposal,entry.entry_id],|r|r.get(0)).optional()?;
    let after = before.replacen(&old, &block(&encode(&entry)?), 1);
    Ok(super::controls::StopIntent {
        skill: entry.skill.clone(),
        before,
        after,
        synchronizable: true,
        siblings: Vec::new(),
        rollback: Some(super::controls::RollbackReceipt {
            proposal: proposal.into(),
            retained_proposal: retained.unwrap_or_else(|| proposal.into()),
            entry: entry.clone(),
            state: state.into(),
        }),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use proptest::prelude::*;

    fn approve(
        wb: &mut crate::api::Workbench,
        request: &super::super::ExperienceRequest,
    ) -> String {
        let id = wb.propose_experience_entry("a0", request).unwrap();
        let status: String = wb
            .db
            .conn()
            .query_row("SELECT status FROM proposals WHERE id=?1", [&id], |row| {
                row.get(0)
            })
            .unwrap();
        if status == "in_review" {
            wb.review_proposal(&id, true, "review exact experience version")
                .unwrap();
        }
        let card = wb
            .db
            .queued_questions(&wb.project_id)
            .unwrap()
            .into_iter()
            .find(|card| card.payload["proposal_id"] == id)
            .unwrap();
        wb.confirm_proposal(&card.id).unwrap();
        id
    }

    fn approved_fixture() -> (tempfile::TempDir, crate::api::Workbench, String) {
        let dir = tempfile::tempdir().unwrap();
        crate::git::init(dir.path(), "main").unwrap();
        std::fs::write(dir.path().join("AGENTS.md"), "Owner instructions\n").unwrap();
        crate::git::commit_all(dir.path(), "seed").unwrap();
        let mut wb =
            crate::api::Workbench::for_test(dir.path(), &["worker", "reviewer"], None).unwrap();
        let path = dir.path().join(".hexagon/skills/alpha/SKILL.md");
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        let original = "---\nname: alpha\ndescription: fixture\n---\n# Owner\nKeep owner text\n";
        std::fs::write(&path, original).unwrap();
        wb.db
            .conn()
            .execute(
                "INSERT INTO role_defs(project_id,name,skills) VALUES('p1','worker','[\"alpha\"]')",
                [],
            )
            .unwrap();
        let ctx = ToolContext::for_agent(&wb.db, dir.path(), "a0");
        let crate::tools::CallOutcome::Done(out) = wb.registry.call(&wb.db, &ctx, "artifact_write",
            serde_json::json!({"path":"reviewed-work.md","kind":"结构说明","content":"Reviewed actual work"})).unwrap()
            else { panic!("artifact write refused") };
        crate::review::submit_review(
            &wb.db,
            &ToolContext::for_agent(&wb.db, dir.path(), "a1"),
            out["artifact_id"].as_str().unwrap(),
            crate::review::Verdict::Pass,
            "Reviewed actual work",
        )
        .unwrap();
        let request = super::super::ExperienceRequest {
            create_role_skill: false,
            body: "APPROVED_FIRST_VERSION".into(),
            notes: String::new(),
            conditions: Default::default(),
            review_event: None,
            targets: vec![super::super::ExperienceTarget {
                skill: "alpha".into(),
                change: None,
                expected_digest: hash(original),
                reason: "Relevant to reviewed work".into(),
            }],
        };
        let proposal = approve(&mut wb, &request);
        let db_path = dir.path().join(".hexagon/state.db");
        wb.db
            .conn()
            .execute("VACUUM INTO ?1", [db_path.to_str().unwrap()])
            .unwrap();
        wb.db = Db::open(&db_path).unwrap();
        (dir, wb, proposal)
    }

    #[test]
    fn experience_old_file_observation_cannot_invalidate_new_approved_revision() {
        use std::sync::mpsc;
        let (dir, mut writer, proposal) = approved_fixture();
        let first = writer.experience_entries("alpha").unwrap().remove(0).entry;
        let reader_root = dir.path().to_path_buf();
        let (observed_tx, observed_rx) = mpsc::channel();
        let (continue_tx, continue_rx) = mpsc::channel();
        let reader = std::thread::spawn(move || {
            let mut reader =
                crate::api::Workbench::for_test(&reader_root, &["worker", "reviewer"], None)
                    .unwrap();
            reader.db = Db::open(reader_root.join(".hexagon/state.db")).unwrap();
            AFTER_ENTRY_FILE_READ.with(|hook| {
                *hook.borrow_mut() = Some(Box::new(move || {
                    observed_tx.send(()).unwrap();
                    continue_rx
                        .recv_timeout(std::time::Duration::from_secs(10))
                        .unwrap();
                }))
            });
            reader.experience_entries("alpha")
        });
        observed_rx
            .recv_timeout(std::time::Duration::from_secs(10))
            .unwrap();
        let mut request = writer
            .experience_proposal(&proposal)
            .unwrap()
            .unwrap()
            .request;
        request.body = "APPROVED_NEW_VERSION".into();
        request.targets[0].expected_digest = hash(
            &std::fs::read_to_string(dir.path().join(".hexagon/skills/alpha/SKILL.md")).unwrap(),
        );
        request.targets[0].change = Some(super::super::ExperienceChange {
            kind: super::super::ExperienceChangeKind::Revise,
            entry_id: first.entry_id.clone(),
            expected_revision: first.revision,
            reason: "Reviewed correction".into(),
        });
        approve(&mut writer, &request);
        continue_tx.send(()).unwrap();
        let observed = reader.join().unwrap();
        let current = writer.experience_entries("alpha").unwrap().remove(0);
        assert_eq!(current.entry.entry_id, first.entry_id);
        assert_eq!(current.entry.revision, 2);
        assert_eq!(current.entry.body, "APPROVED_NEW_VERSION");
        assert_eq!(
            current.state, "active",
            "an older file observation must not revoke the newly reviewed receipt"
        );
        assert!(matches!(
            observed,
            Err(error) if crate::errcode::ErrorCode::code(&error) == "stale_experience"
        ));
    }
    // Workbench seam with real target bytes and two SQLite connections. Vary
    // both revised records and duplicate approvals that retain the revision.
    fn assert_new_receipt_survives_observation(
        suffix: &str,
        broken: bool,
        revise: bool,
        new_source: bool,
    ) {
        use std::sync::mpsc;
        let (dir, mut writer, proposal) = approved_fixture();
        let first = writer.experience_entries("alpha").unwrap().remove(0).entry;
        let path = dir.path().join(".hexagon/skills/alpha/SKILL.md");
        if broken {
            let original = std::fs::read_to_string(&path).unwrap();
            std::fs::write(
                &path,
                original.replace("APPROVED_FIRST_VERSION", "UNREVIEWED_EXTERNAL_EDIT"),
            )
            .unwrap();
        }
        let reader_root = dir.path().to_path_buf();
        let (observed_tx, observed_rx) = mpsc::channel();
        let (continue_tx, continue_rx) = mpsc::channel();
        let reader = std::thread::spawn(move || {
            let mut reader =
                crate::api::Workbench::for_test(&reader_root, &["worker", "reviewer"], None)
                    .unwrap();
            reader.db = Db::open(reader_root.join(".hexagon/state.db")).unwrap();
            AFTER_ENTRY_FILE_READ.with(|hook| {
                *hook.borrow_mut() = Some(Box::new(move || {
                    observed_tx.send(()).unwrap();
                    continue_rx
                        .recv_timeout(std::time::Duration::from_secs(10))
                        .unwrap();
                }))
            });
            reader.experience_entries("alpha")
        });
        observed_rx
            .recv_timeout(std::time::Duration::from_secs(10))
            .unwrap();
        if new_source {
            let ctx = ToolContext::for_agent(&writer.db, dir.path(), "a0");
            let crate::tools::CallOutcome::Done(work) = writer.registry.call(&writer.db, &ctx, "artifact_write",
                serde_json::json!({"path":"second-reviewed-work.md","kind":"结构说明","content":"A distinct reviewed source"})).unwrap()
                else { panic!("second artifact refused") };
            crate::review::submit_review(
                &writer.db,
                &ToolContext::for_agent(&writer.db, dir.path(), "a1"),
                work["artifact_id"].as_str().unwrap(),
                crate::review::Verdict::Pass,
                "Reviewed second source",
            )
            .unwrap();
        }
        let mut request = writer
            .experience_proposal(&proposal)
            .unwrap()
            .unwrap()
            .request;
        request.targets[0].expected_digest = hash(&std::fs::read_to_string(&path).unwrap());
        if revise {
            request.body = format!("APPROVED_NEW_{suffix}");
            request.targets[0].change = Some(super::super::ExperienceChange {
                kind: super::super::ExperienceChangeKind::Revise,
                entry_id: first.entry_id.clone(),
                expected_revision: first.revision,
                reason: "Reviewed correction".into(),
            });
        }
        approve(&mut writer, &request);
        continue_tx.send(()).unwrap();
        let observed = reader.join().unwrap();
        let current = writer.experience_entries("alpha").unwrap().remove(0);
        assert_eq!(current.entry.entry_id, first.entry_id);
        assert_eq!(current.entry.revision, if revise { 2 } else { 1 });
        assert_eq!(current.entry.body, request.body);
        assert_eq!(
            current.entry.sources.len(),
            if new_source && !revise { 2 } else { 1 }
        );
        assert_eq!(
            current.state, "active",
            "stale integrity observations must not invalidate an approved receipt"
        );
        assert!(matches!(
            observed,
            Err(error) if crate::errcode::ErrorCode::code(&error) == "stale_experience"
        ));
        let ctx = ToolContext::for_agent(&writer.db, dir.path(), "a0");
        let crate::tools::CallOutcome::Done(loaded) = writer
            .registry
            .call(
                &writer.db,
                &ctx,
                "load_skill",
                serde_json::json!({"name":"alpha"}),
            )
            .unwrap()
        else {
            panic!("skill load refused")
        };
        let instructions = loaded["instructions"].as_str().unwrap();
        assert!(instructions.contains(&request.body));
        assert!(!instructions.contains("UNREVIEWED_EXTERNAL_EDIT"));
    }

    #[test]
    fn experience_broken_snapshot_cannot_invalidate_reapproved_receipt() {
        assert_new_receipt_survives_observation("replacement", true, true, false);
    }

    #[test]
    fn experience_same_revision_approval_requires_a_current_receipt_observation() {
        for new_source in [false, true] {
            assert_new_receipt_survives_observation("duplicate", false, false, new_source);
        }
    }

    fn assert_pending_observation(suffix: &str, replaced: bool) {
        use std::sync::mpsc;
        let (dir, mut writer, proposal) = approved_fixture();
        let first = writer.experience_entries("alpha").unwrap().remove(0).entry;
        let reader_root = dir.path().to_path_buf();
        let (observed_tx, observed_rx) = mpsc::channel();
        let (continue_tx, continue_rx) = mpsc::channel();
        let reader = std::thread::spawn(move || {
            let mut reader =
                crate::api::Workbench::for_test(&reader_root, &["worker", "reviewer"], None)
                    .unwrap();
            reader.db = Db::open(reader_root.join(".hexagon/state.db")).unwrap();
            AFTER_ENTRY_FILE_READ.with(|hook| {
                *hook.borrow_mut() = Some(Box::new(move || {
                    observed_tx.send(()).unwrap();
                    continue_rx
                        .recv_timeout(std::time::Duration::from_secs(10))
                        .unwrap();
                }))
            });
            reader.experience_entries("alpha")
        });
        observed_rx
            .recv_timeout(std::time::Duration::from_secs(10))
            .unwrap();
        let mut request = writer
            .experience_proposal(&proposal)
            .unwrap()
            .unwrap()
            .request;
        request.body = format!("NEW_PENDING_{suffix}");
        request.targets[0].expected_digest = hash(
            &std::fs::read_to_string(dir.path().join(".hexagon/skills/alpha/SKILL.md")).unwrap(),
        );
        request.targets[0].change = Some(super::super::ExperienceChange {
            kind: super::super::ExperienceChangeKind::Revise,
            entry_id: first.entry_id,
            expected_revision: first.revision,
            reason: "Reviewed correction".into(),
        });
        let id = writer.propose_experience_entry("a0", &request).unwrap();
        let status: String = writer
            .db
            .conn()
            .query_row("SELECT status FROM proposals WHERE id=?1", [&id], |row| {
                row.get(0)
            })
            .unwrap();
        if status == "in_review" {
            writer
                .review_proposal(&id, true, "review pending version")
                .unwrap();
        }
        let card = writer
            .db
            .queued_questions(&writer.project_id)
            .unwrap()
            .into_iter()
            .find(|card| card.payload["proposal_id"] == id)
            .unwrap();
        writer.fail_next_experience(if replaced {
            ExperienceFault::AfterReplace
        } else {
            ExperienceFault::AfterIntent
        });
        assert!(writer.confirm_proposal(&card.id).is_err());
        continue_tx.send(()).unwrap();
        let observed = reader.join().unwrap();
        assert_eq!(
            writer.experience_entries("alpha").unwrap()[0].state,
            "pending_recovery"
        );
        assert!(matches!(
            observed,
            Err(error) if crate::errcode::ErrorCode::code(&error) == "stale_experience"
        ), "a pending intent changed the loading contract even though receipt and file still match");
    }

    #[test]
    fn experience_observation_cannot_load_a_receipt_after_a_new_pending_intent() {
        assert_pending_observation("version", false);
    }

    proptest! {
        #![proptest_config(proptest::test_runner::Config {
            cases: 4,
            failure_persistence: Some(Box::new(proptest::test_runner::FileFailurePersistence::Direct("proptest-regressions/experience-observation.txt"))),
            ..proptest::test_runner::Config::default()
        })]
        #[test]
        fn experience_pending_operations_require_a_current_loading_observation(
            suffix in "[a-z]{1,10}", replaced in proptest::bool::ANY,
        ) {
            assert_pending_observation(&suffix, replaced);
        }
    }

    proptest! {
        #![proptest_config(proptest::test_runner::Config {
            cases: 8,
            failure_persistence: Some(Box::new(proptest::test_runner::FileFailurePersistence::Direct("proptest-regressions/experience-observation.txt"))),
            ..proptest::test_runner::Config::default()
        })]
        #[test]
        fn experience_receipt_observations_bind_revision_record_and_proposal(
            suffix in "[a-z]{1,10}", broken in proptest::bool::ANY, revise in proptest::bool::ANY, new_source in proptest::bool::ANY,
        ) {
            assert_new_receipt_survives_observation(&suffix, broken, revise || broken, new_source);
        }
    }
    proptest! {
        #[test]
        fn duplicate_block_identities_never_remain_unique(body in "[a-zA-Z]{1,30}") {
            let entry = ExperienceEntry { schema_version:1,entry_id:"e1".into(),revision:1,project_id:"p1".into(),skill:"alpha".into(),body,notes:String::new(),conditions:Default::default(),sources:vec![] };
            let text = block(&encode(&entry).unwrap());
            let single = block_identities(&text).unwrap();
            prop_assert_eq!(single.get("e1"),Some(&1));
            let duplicate = format!("{text}{text}");
            let twice = block_identities(&duplicate).unwrap();
            prop_assert_eq!(twice.get("e1"),Some(&2));
            prop_assert!(block_identities(&text.replace("schema_version\":1", "schema_version\":99")).is_none());
        }
    }
    proptest! {
        #[test]
        fn exact_key_is_idempotent_and_conditions_are_sets(body in "[a-zA-Z \n]{1,80}", note in "[a-zA-Z ]{0,30}") {
            let conditions = ExperienceConditions { roles: vec!["b".into(), "a".into(), "b".into()], stages: vec![], paths: vec![] };
            let key = content_key(&body, &note, &conditions);
            prop_assert_eq!(&key, &content_key(&key.0, &key.1, &key.2));
            let mut reordered = conditions;
            reordered.roles.reverse();
            prop_assert_eq!(&key, &content_key(&body.replace('\n', "\r\n"), &note, &reordered));
        }
    }
}

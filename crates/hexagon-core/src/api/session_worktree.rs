//! SessionHost physical worktree through the `worktree` confirmation card
//! (Wayfinder-07 Addendum A, owner ruling 2026-10-12).
//!
//! Request → card → confirm/reject. Only the confirmation, after rechecking
//! every bound fact and answering the card, mints the proof that `git.rs`
//! requires; the card id is the action identity in trace, diag and intent.
//! Rejected alternatives: a model-registry tool behind a permission card
//! (needed a forged agent identity and was model-reachable), and treating an
//! empty pending queue as approval. Cost model: a false refusal costs the
//! owner one more request; a false confirm writes or deletes a directory
//! nobody approved, so every unprovable fact refuses.
use super::*;
use crate::session_host::{
    SessionRecord, WorktreeAttachment, WorktreeIntent, WorktreeIntentState, WorktreeOperation,
};
use std::path::{Path, PathBuf};

/// Leaf error whose variant names are the IPC codes and the diag reason
/// codes (`errcode::variant_code`), so UI, trace and diag share one word.
#[derive(Debug, thiserror::Error)]
pub enum WorktreeError {
    #[error("session is not active: {0}")]
    WorktreeSessionInactive(String),
    #[error("worktree path is not allowed: {0}")]
    WorktreePathInvalid(String),
    #[error("worktree binding conflict: {0}")]
    WorktreeBindingConflict(String),
    #[error("worktree binding drifted: {0}")]
    WorktreeBindingDrift(String),
    #[error("session has no worktree binding")]
    WorktreeMissing,
    #[error("worktree was not created by this session")]
    WorktreeNotOwned,
    #[error("worktree has uncommitted or untracked files: {0}")]
    WorktreeDirty(String),
    #[error("worktree outcome is unknown; verify before continuing")]
    WorktreeOutcomeUnknown,
    #[error("unknown worktree card: {0}")]
    WorktreeCardUnknown(String),
    #[error("worktree create failed: {0}")]
    WorktreeCreateFailed(String),
    #[error("worktree delete failed: {0}")]
    WorktreeDeleteFailed(String),
}

impl WorktreeError {
    pub fn reason_code(&self) -> String {
        crate::errcode::variant_code(self)
    }
}

/// Receipt of a worktree request: the queued card the owner must answer.
#[derive(Debug, Clone, serde::Serialize, ts_rs::TS)]
#[ts(export, export_to = "../../../ui/src/gen/")]
pub struct WorktreeRequest {
    pub question_id: String,
    pub operation: WorktreeOperation,
    pub path: String,
    pub baseline: String,
    pub baseline_commit: String,
    /// A queued card for the same session/operation/path/commit already
    /// existed and was returned instead of a second card.
    pub reused: bool,
}

/// Facts bound into a card and rechecked at confirmation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct WorktreeBound {
    pub session_id: String,
    pub root: PathBuf,
    pub path: PathBuf,
    pub baseline: String,
    pub commit: String,
}

fn drift(detail: impl std::fmt::Display) -> WorktreeError {
    WorktreeError::WorktreeBindingDrift(detail.to_string())
}

impl Workbench {
    fn worktree_session(&self) -> Result<SessionRecord, WorktreeError> {
        let session = crate::session_host::current(&self.db, &self.project_id)
            .map_err(|e| WorktreeError::WorktreeSessionInactive(e.to_string()))?
            .ok_or_else(|| WorktreeError::WorktreeSessionInactive("no session".into()))?;
        if session.status.terminal() {
            return Err(WorktreeError::WorktreeSessionInactive(format!(
                "{:?}",
                session.status
            )));
        }
        Ok(session)
    }

    /// The repository root must still be the one the session was bound to;
    /// the stored root may be a non-canonical spelling of the same directory.
    fn worktree_root(&self, session: &SessionRecord) -> Result<PathBuf, WorktreeError> {
        let root = crate::git::canonical_root(&self.repo_root).map_err(drift)?;
        let bound = Path::new(&session.project_root)
            .canonicalize()
            .map_err(drift)?;
        if root != bound {
            return Err(drift("project root changed"));
        }
        Ok(root)
    }

    fn bind_worktree_create(
        &self,
        session: &SessionRecord,
        path: &Path,
    ) -> Result<WorktreeBound, WorktreeError> {
        if session.worktree.is_some() {
            return Err(WorktreeError::WorktreeBindingConflict(
                "the session already has a worktree".into(),
            ));
        }
        let root = self.worktree_root(session)?;
        if !path.is_absolute() {
            return Err(WorktreeError::WorktreePathInvalid(
                "path must be absolute".into(),
            ));
        }
        let path = crate::git::canonical_worktree_path(path)
            .map_err(|e| WorktreeError::WorktreePathInvalid(e.to_string()))?;
        // Inside the project tree the checkout would show up as untracked
        // content of the main tree and enter delivery fingerprints.
        if path == root || path.starts_with(&root) {
            return Err(WorktreeError::WorktreePathInvalid(
                "path must be outside the project root".into(),
            ));
        }
        if path.exists() {
            return Err(WorktreeError::WorktreePathInvalid(
                "path already exists".into(),
            ));
        }
        if crate::git::worktree_registered(&root, &path)
            .map_err(drift)?
            .is_some()
        {
            return Err(WorktreeError::WorktreeBindingConflict(
                "path is already registered with git".into(),
            ));
        }
        let commit = crate::git::baseline_commit(&root, &session.baseline).map_err(drift)?;
        Ok(WorktreeBound {
            session_id: session.session_id.clone(),
            root,
            path,
            baseline: session.baseline.clone(),
            commit,
        })
    }

    fn bind_worktree_cleanup(
        &self,
        session: &SessionRecord,
    ) -> Result<WorktreeBound, WorktreeError> {
        let attachment = session
            .worktree
            .as_ref()
            .ok_or(WorktreeError::WorktreeMissing)?;
        if !attachment.created_by_session {
            return Err(WorktreeError::WorktreeNotOwned);
        }
        let commit = attachment
            .baseline_commit
            .clone()
            .ok_or_else(|| drift("binding has no baseline commit"))?;
        let root = self.worktree_root(session)?;
        let path =
            crate::git::canonical_worktree_path(Path::new(&attachment.path)).map_err(drift)?;
        let entry = crate::git::worktree_registered(&root, &path)
            .map_err(drift)?
            .ok_or_else(|| drift("worktree is no longer registered"))?;
        if entry.head != commit {
            return Err(drift("worktree HEAD moved from the bound commit"));
        }
        let current = crate::git::baseline_commit(&root, &session.baseline).map_err(drift)?;
        if current != commit {
            return Err(drift("baseline moved since the worktree was created"));
        }
        match crate::git::worktree_clean(&path) {
            Ok(true) => {}
            Ok(false) => return Err(WorktreeError::WorktreeDirty(path.display().to_string())),
            Err(error) => return Err(WorktreeError::WorktreeDirty(error.to_string())),
        }
        Ok(WorktreeBound {
            session_id: session.session_id.clone(),
            root,
            path,
            baseline: session.baseline.clone(),
            commit,
        })
    }

    fn bind_worktree(
        &self,
        session: &SessionRecord,
        operation: WorktreeOperation,
        path: Option<&Path>,
    ) -> Result<WorktreeBound, WorktreeError> {
        if session
            .worktree_intent
            .as_ref()
            .is_some_and(|intent| intent.state == WorktreeIntentState::Executing)
        {
            return Err(WorktreeError::WorktreeOutcomeUnknown);
        }
        match operation {
            WorktreeOperation::Create => {
                let path = path.ok_or_else(|| {
                    WorktreeError::WorktreePathInvalid("a path is required".into())
                })?;
                self.bind_worktree_create(session, path)
            }
            WorktreeOperation::Cleanup => self.bind_worktree_cleanup(session),
        }
    }

    #[allow(clippy::too_many_arguments)]
    fn worktree_event(
        &self,
        class: &'static str,
        kind: &str,
        branch: &str,
        reason: &str,
        session_id: &str,
        question_id: Option<&str>,
        path: &str,
        commit: &str,
        started: std::time::Instant,
    ) -> Result<(), ApiError> {
        crate::diag::note(
            class,
            class == crate::diag::CLASS_REJECT,
            Some(&self.project_id),
            None,
            question_id,
            Some(session_id),
            branch,
            reason,
            started,
        );
        self.db.append_event(
            &self.project_id,
            crate::trace::EventKind::System,
            serde_json::json!({
                "kind": kind,
                "session_id": session_id,
                "question_id": question_id,
                "path": path,
                "baseline_commit": commit,
                "branch": branch,
                "reason_code": reason,
            }),
            None,
            None,
        )?;
        Ok(())
    }

    fn worktree_refused(
        &self,
        session_id: &str,
        branch: &str,
        question_id: Option<&str>,
        error: WorktreeError,
        started: std::time::Instant,
    ) -> ApiError {
        let reason = error.reason_code();
        let kind = if matches!(error, WorktreeError::WorktreeBindingDrift(_)) {
            "worktree_binding_drift"
        } else {
            "worktree_request_refused"
        };
        if let Err(trace) = self.worktree_event(
            crate::diag::CLASS_REJECT,
            kind,
            branch,
            &reason,
            session_id,
            question_id,
            "",
            "",
            started,
        ) {
            log::warn!("worktree refusal trace failed: {trace}");
        }
        error.into()
    }

    /// Validate the request and enqueue one `worktree` card. No filesystem
    /// write happens here. A repeated request returns the queued card.
    pub fn request_session_worktree(
        &self,
        operation: WorktreeOperation,
        path: Option<&str>,
    ) -> Result<WorktreeRequest, ApiError> {
        let started = std::time::Instant::now();
        let session = self.worktree_session()?;
        let bound = match self.bind_worktree(&session, operation, path.map(Path::new)) {
            Ok(bound) => bound,
            Err(error) => {
                return Err(self.worktree_refused(
                    &session.session_id,
                    "worktree_request",
                    None,
                    error,
                    started,
                ))
            }
        };
        let path_text = bound.path.to_string_lossy().into_owned();
        let key = format!(
            "worktree:{}:{}:{}:{}",
            bound.session_id,
            operation.as_str(),
            path_text,
            bound.commit
        );
        // agent_id is NULL on these cards, so the partial unique index does
        // not deduplicate them; the key lookup is the idempotence boundary.
        if let Some(question_id) = crate::cards::queued_by_key(&self.db, &self.project_id, &key)? {
            return Ok(WorktreeRequest {
                question_id,
                operation,
                path: path_text,
                baseline: bound.baseline,
                baseline_commit: bound.commit,
                reused: true,
            });
        }
        let question_id = crate::cards::enqueue(
            &self.db,
            &self.project_id,
            None,
            crate::cards::CardKind::Worktree,
            serde_json::json!({
                "operation": operation.as_str(),
                "session_id": bound.session_id,
                "project_root": bound.root.to_string_lossy(),
                "path": path_text,
                "baseline": bound.baseline,
                "baseline_commit": bound.commit,
            }),
            Some(&key),
        )?;
        self.worktree_event(
            crate::diag::CLASS_HOST,
            "worktree_requested",
            "worktree_request",
            operation.as_str(),
            &bound.session_id,
            Some(&question_id),
            &path_text,
            &bound.commit,
            started,
        )?;
        Ok(WorktreeRequest {
            question_id,
            operation,
            path: path_text,
            baseline: bound.baseline,
            baseline_commit: bound.commit,
            reused: false,
        })
    }

    fn worktree_card(
        &self,
        question_id: &str,
    ) -> Result<(crate::cards::Card, WorktreeOperation), WorktreeError> {
        let card =
            crate::cards::get_queued(&self.db, question_id, crate::cards::CardKind::Worktree)
                .map_err(|_| WorktreeError::WorktreeCardUnknown(question_id.into()))?;
        if card.project_id != self.project_id {
            return Err(WorktreeError::WorktreeCardUnknown(question_id.into()));
        }
        let operation = card.payload["operation"]
            .as_str()
            .and_then(WorktreeOperation::parse)
            .ok_or_else(|| WorktreeError::WorktreeCardUnknown(question_id.into()))?;
        Ok((card, operation))
    }

    /// Recheck a queued card against the live repository and session. Used by
    /// the confirmation itself and by the managed route's host-forced facts.
    pub(crate) fn recheck_worktree_card(
        &self,
        question_id: &str,
    ) -> Result<(WorktreeOperation, WorktreeBound), WorktreeError> {
        let (card, operation) = self.worktree_card(question_id)?;
        let p = &card.payload;
        let session = self.worktree_session()?;
        if p["session_id"].as_str() != Some(session.session_id.as_str()) {
            return Err(drift("card belongs to another session"));
        }
        let requested = p["path"].as_str().map(Path::new);
        let bound = self.bind_worktree(&session, operation, requested)?;
        if Some(bound.path.to_string_lossy().as_ref()) != p["path"].as_str()
            || Some(bound.commit.as_str()) != p["baseline_commit"].as_str()
            || Some(bound.baseline.as_str()) != p["baseline"].as_str()
            || Some(bound.root.to_string_lossy().as_ref()) != p["project_root"].as_str()
        {
            return Err(drift("facts changed since the request"));
        }
        Ok((operation, bound))
    }

    /// Owner confirmation from the pending dock.
    pub fn confirm_session_worktree(&self, question_id: &str) -> Result<SessionRecord, ApiError> {
        self.confirm_session_worktree_by(question_id, "owner")
    }

    /// Confirm one card as `by` (owner or the managed decider). The card is
    /// answered before git runs, so a failure is recorded, never re-sent.
    pub(crate) fn confirm_session_worktree_by(
        &self,
        question_id: &str,
        by: &str,
    ) -> Result<SessionRecord, ApiError> {
        let started = std::time::Instant::now();
        let session_id = crate::session_host::current(&self.db, &self.project_id)?
            .map(|s| s.session_id)
            .unwrap_or_default();
        let (operation, bound) = match self.recheck_worktree_card(question_id) {
            Ok(checked) => checked,
            Err(WorktreeError::WorktreeCardUnknown(id)) => {
                return Err(WorktreeError::WorktreeCardUnknown(id).into())
            }
            Err(error) => {
                // Settle the card so the stale decision cannot be replayed.
                crate::cards::annotate(
                    &self.db,
                    question_id,
                    &[
                        ("outcome", serde_json::json!("conflict")),
                        ("reason_code", serde_json::json!(error.reason_code())),
                    ],
                )?;
                crate::cards::answer(&self.db, question_id, by)?;
                return Err(self.worktree_refused(
                    &session_id,
                    "worktree_confirm",
                    Some(question_id),
                    error,
                    started,
                ));
            }
        };
        let path_text = bound.path.to_string_lossy().into_owned();
        let mut intent = WorktreeIntent {
            operation,
            question_id: question_id.to_owned(),
            path: path_text.clone(),
            baseline_commit: bound.commit.clone(),
            state: WorktreeIntentState::Executing,
            reason_code: None,
        };
        crate::session_host::set_worktree_intent(&self.db, &bound.session_id, Some(&intent))?;
        crate::cards::annotate(
            &self.db,
            question_id,
            &[("outcome", serde_json::json!("confirmed"))],
        )?;
        crate::cards::answer(&self.db, question_id, by)?;
        let proof = crate::session_host::PermissionRecheck::confirmed_for_worktree(
            operation.proof_operation(),
            question_id,
            &bound.session_id,
            &bound.root.to_string_lossy(),
            &path_text,
            &bound.commit,
        );
        let result = match operation {
            WorktreeOperation::Create => self.execute_worktree_create(&bound, &proof),
            WorktreeOperation::Cleanup => self.execute_worktree_cleanup(&bound, &proof),
        };
        match result {
            Ok(()) => {
                crate::session_host::set_worktree_intent(&self.db, &bound.session_id, None)?;
                let kind = match operation {
                    WorktreeOperation::Create => "worktree_created",
                    WorktreeOperation::Cleanup => "worktree_deleted",
                };
                self.worktree_event(
                    crate::diag::CLASS_HOST,
                    kind,
                    "worktree_confirm",
                    by,
                    &bound.session_id,
                    Some(question_id),
                    &path_text,
                    &bound.commit,
                    started,
                )?;
                Ok(crate::session_host::get(&self.db, &bound.session_id)?)
            }
            Err(error) => {
                let reason = error.reason_code();
                intent.state = WorktreeIntentState::Failed;
                intent.reason_code = Some(reason.clone());
                crate::session_host::set_worktree_intent(
                    &self.db,
                    &bound.session_id,
                    Some(&intent),
                )?;
                let kind = match operation {
                    WorktreeOperation::Create => "worktree_create_failed",
                    WorktreeOperation::Cleanup => "worktree_delete_failed",
                };
                self.worktree_event(
                    crate::diag::CLASS_REJECT,
                    kind,
                    "worktree_confirm",
                    &reason,
                    &bound.session_id,
                    Some(question_id),
                    &path_text,
                    &bound.commit,
                    started,
                )?;
                Err(error.into())
            }
        }
    }

    fn execute_worktree_create(
        &self,
        bound: &WorktreeBound,
        proof: &crate::session_host::PermissionRecheck,
    ) -> Result<(), WorktreeError> {
        crate::git::worktree_add_verified(
            &bound.root,
            &bound.path,
            &bound.commit,
            &bound.session_id,
            proof,
        )
        .map_err(|e| WorktreeError::WorktreeCreateFailed(e.to_string()))?;
        let rollback = |why: String| match crate::git::worktree_rollback_created(
            &bound.root,
            &bound.path,
            &bound.commit,
            &bound.session_id,
            proof,
        ) {
            Ok(()) => WorktreeError::WorktreeCreateFailed(why),
            Err(error) => WorktreeError::WorktreeCreateFailed(format!(
                "{why}; rollback refused, explicit cleanup required: {error}"
            )),
        };
        match crate::git::worktree_registered(&bound.root, &bound.path) {
            Ok(Some(entry)) if entry.head == bound.commit => {}
            Ok(_) => return Err(rollback("registration did not match after create".into())),
            Err(error) => return Err(rollback(error.to_string())),
        }
        let attachment = WorktreeAttachment {
            path: bound.path.to_string_lossy().into_owned(),
            baseline: bound.baseline.clone(),
            baseline_commit: Some(bound.commit.clone()),
            created_by_session: true,
            cleanup_requested: false,
        };
        let session = crate::session_host::get(&self.db, &bound.session_id)
            .map_err(|e| rollback(e.to_string()))?;
        crate::session_host::attach_worktree(
            &self.db,
            &bound.session_id,
            &session.project_root,
            &bound.baseline,
            attachment,
        )
        .map_err(|e| rollback(e.to_string()))?;
        Ok(())
    }

    fn execute_worktree_cleanup(
        &self,
        bound: &WorktreeBound,
        proof: &crate::session_host::PermissionRecheck,
    ) -> Result<(), WorktreeError> {
        let session = crate::session_host::get(&self.db, &bound.session_id)
            .map_err(|e| WorktreeError::WorktreeDeleteFailed(e.to_string()))?;
        crate::session_host::request_worktree_cleanup(
            &self.db,
            &bound.session_id,
            &session.project_root,
            &bound.baseline,
            proof.clone(),
        )
        .map_err(|e| WorktreeError::WorktreeDeleteFailed(e.to_string()))?;
        crate::git::worktree_remove_verified(
            &bound.root,
            &bound.path,
            &bound.commit,
            &bound.session_id,
            proof,
        )
        .map_err(|e| WorktreeError::WorktreeDeleteFailed(e.to_string()))?;
        crate::session_host::clear_worktree(
            &self.db,
            &bound.session_id,
            &session.project_root,
            &bound.baseline,
        )
        .map_err(|e| WorktreeError::WorktreeDeleteFailed(e.to_string()))?;
        Ok(())
    }

    /// Owner refusal from the pending dock: nothing is written.
    pub fn reject_session_worktree(&self, question_id: &str) -> Result<(), ApiError> {
        self.reject_session_worktree_by(question_id, "owner")
    }

    pub(crate) fn reject_session_worktree_by(
        &self,
        question_id: &str,
        by: &str,
    ) -> Result<(), ApiError> {
        let started = std::time::Instant::now();
        let (card, _) = self.worktree_card(question_id)?;
        crate::cards::annotate(
            &self.db,
            question_id,
            &[("outcome", serde_json::json!("rejected"))],
        )?;
        crate::cards::answer(&self.db, question_id, by)?;
        self.worktree_event(
            crate::diag::CLASS_REJECT,
            "worktree_rejected",
            "worktree_confirm",
            "owner_rejected",
            card.payload["session_id"].as_str().unwrap_or_default(),
            Some(question_id),
            card.payload["path"].as_str().unwrap_or_default(),
            card.payload["baseline_commit"].as_str().unwrap_or_default(),
            started,
        )?;
        Ok(())
    }

    /// Read-only settlement of an interrupted or failed intent, plus a
    /// binding check when there is none. Never runs git add/remove.
    pub fn verify_session_worktree(&self) -> Result<SessionRecord, ApiError> {
        let started = std::time::Instant::now();
        let session = self.worktree_session()?;
        let Some(intent) = session.worktree_intent.clone() else {
            let reason = match self.validate_session_worktree() {
                Ok(()) if session.worktree.is_some() => "worktree_binding_ok",
                Ok(()) => "worktree_absent",
                Err(error) => return Err(error),
            };
            self.worktree_event(
                crate::diag::CLASS_HOST,
                "worktree_verified",
                "worktree_verify",
                reason,
                &session.session_id,
                None,
                session
                    .worktree
                    .as_ref()
                    .map(|w| w.path.as_str())
                    .unwrap_or_default(),
                "",
                started,
            )?;
            return Ok(session);
        };
        let root = self.worktree_root(&session).map_err(ApiError::from)?;
        let path = PathBuf::from(&intent.path);
        let entry = crate::git::worktree_registered(&root, &path)?;
        let reason = match intent.operation {
            WorktreeOperation::Create => match entry {
                Some(entry) if entry.head == intent.baseline_commit => {
                    if session.worktree.is_none() {
                        crate::session_host::attach_worktree(
                            &self.db,
                            &session.session_id,
                            &session.project_root,
                            &session.baseline,
                            WorktreeAttachment {
                                path: intent.path.clone(),
                                baseline: session.baseline.clone(),
                                baseline_commit: Some(intent.baseline_commit.clone()),
                                created_by_session: true,
                                cleanup_requested: false,
                            },
                        )?;
                    }
                    "worktree_create_confirmed"
                }
                _ => "worktree_create_absent",
            },
            WorktreeOperation::Cleanup => {
                if entry.is_none() && !path.exists() {
                    if session.worktree.is_some() {
                        crate::session_host::clear_worktree(
                            &self.db,
                            &session.session_id,
                            &session.project_root,
                            &session.baseline,
                        )?;
                    }
                    "worktree_delete_confirmed"
                } else {
                    "worktree_delete_absent"
                }
            }
        };
        crate::session_host::set_worktree_intent(&self.db, &session.session_id, None)?;
        self.worktree_event(
            crate::diag::CLASS_HOST,
            "worktree_verified",
            "worktree_verify",
            reason,
            &session.session_id,
            Some(&intent.question_id),
            &intent.path,
            &intent.baseline_commit,
            started,
        )?;
        Ok(crate::session_host::get(&self.db, &session.session_id)?)
    }
}

//! Explicit owner resume of a paused SessionHost (state table: paused →
//! explicit resume → running, with root/baseline/permission recheck).
//!
//! Found in the 2026-10-13 golden path: a window-close pause also pauses
//! orchestration, and the only resume entry needed an interrupted run, which
//! a close never creates — the owner could answer cards but nothing ran
//! ("project or instance stopped"). Rejected: auto-resuming on reopen, which
//! would run work the owner has not looked at yet.
use super::*;

impl Workbench {
    /// Resume the current paused host. With `run_id`, also re-enter that
    /// interrupted run through the existing recovery seam; a continuation
    /// failure is recorded as a runtime failure and returned.
    pub fn resume_session(
        &self,
        run_id: Option<&str>,
    ) -> Result<crate::session_host::SessionRecord, ApiError> {
        let session = crate::session_host::current(&self.db, &self.project_id)?
            .ok_or_else(|| crate::session_host::SessionError::NotFound(self.project_id.clone()))?;
        let baseline =
            crate::git::baseline_branch(&self.repo_root).unwrap_or_else(|_| "main".to_string());
        let root = self.repo_root.to_string_lossy();
        crate::session_host::validate_binding(
            &self.db,
            &session.session_id,
            &root,
            &baseline,
            "resume",
        )?;
        crate::session_host::validate_worktree(&self.db, &session.session_id, &root, &baseline)?;
        self.validate_session_worktree()?;
        let proof = self.recheck_session_permissions_for("resume")?;
        let resumed =
            crate::session_host::resume(&self.db, &session.session_id, &root, &baseline, 0, proof)?;
        if orchestra::is_paused(&self.db, &self.project_id)? {
            orchestra::resume(&self.db, &self.project_id)?;
        }
        let Some(run_id) = run_id else {
            return Ok(resumed);
        };
        match self.recover_run(run_id) {
            Ok(_) => Ok(crate::session_host::get(&self.db, &resumed.session_id)?),
            Err(error) => {
                self.session_note("session_resume", self.session_runtime_failed());
                Err(error)
            }
        }
    }
}

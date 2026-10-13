//! P1-2 "continue later": the owner schedules one trigger for the current
//! session. Re-audit 2026-10-13: the shell used to pass model slot, permission
//! package and trace id straight from IPC, so the audit fields were whatever
//! the UI claimed. Core now derives them from the session's current turn;
//! IPC supplies only the idempotence key and the fire time.
use super::*;

impl Workbench {
    pub fn schedule_session_trigger(
        &self,
        trigger_id: &str,
        fire_at: u64,
    ) -> Result<crate::session_host::OneTimeTrigger, ApiError> {
        self.validate_session_worktree()?;
        let session = crate::session_host::current(&self.db, &self.project_id)?
            .ok_or_else(|| crate::session_host::SessionError::NotFound(self.project_id.clone()))?;
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_secs())
            .unwrap_or_default();
        if fire_at <= now {
            return Err(crate::session_host::SessionError::TriggerNotReady(
                "fire time must be in the future".into(),
            )
            .into());
        }
        let turn_id = session
            .current_turn_id
            .clone()
            .ok_or_else(|| ApiError::BadInput("session has no recoverable turn".into()))?;
        let agent: Option<String> = self
            .db
            .conn()
            .query_row(
                "SELECT agent_id FROM events WHERE project_id=?1 AND id=?2 AND kind='turn_started'",
                rusqlite::params![self.project_id, turn_id],
                |row| row.get(0),
            )
            .optional()?;
        let agent =
            agent.ok_or_else(|| ApiError::BadInput("recoverable turn event missing".into()))?;
        let model_slot: Option<String> = self
            .db
            .conn()
            .query_row(
                "SELECT model_slot FROM agents WHERE project_id=?1 AND id=?2",
                rusqlite::params![self.project_id, agent],
                |row| row.get(0),
            )
            .optional()?
            .flatten();
        let mode = crate::approval_mode::read(&self.db, &self.project_id)?.mode;
        Ok(crate::session_host::schedule_once(
            &self.db,
            &session.session_id,
            &session.project_root,
            &session.baseline,
            trigger_id,
            model_slot.as_deref().unwrap_or("default"),
            vec![format!("approval_mode:{}", mode.as_str())],
            fire_at,
            &format!("turn:{turn_id}"),
        )?)
    }
}

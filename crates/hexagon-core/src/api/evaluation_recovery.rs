//! D10: recovery observation never replays an evaluation.
use super::*;
impl Workbench {
    #[cfg(test)]
    pub(super) fn break_evaluation_terminal_links_fixture(&self, id: &str) -> Result<(), ApiError> {
        self.db.conn().execute(
            "UPDATE evaluation_plan_runs SET state='started' WHERE run_id=?1",
            [id],
        )?;
        self.db.conn().execute(
            "UPDATE evaluation_budget_runs SET closed=0 WHERE run_id=?1",
            [id],
        )?;
        self.db.conn().execute(
            "UPDATE evaluation_controls SET state='running',phase_started_ms=0 WHERE run_id=?1",
            [id],
        )?;
        Ok(())
    }

    #[cfg(test)]
    pub(super) fn arm_evaluation_crash_fixture(&self, point: &str) -> Result<(), ApiError> {
        Ok(crate::evaluation::recovery::arm_crash(point)?)
    }

    pub fn reconcile_evaluation_run(
        &self,
        id: &str,
    ) -> Result<crate::evaluation::RecoveryEntry, ApiError> {
        Ok(crate::evaluation::recovery::reconcile(
            &self.db,
            &self.repo_root,
            id,
        )?)
    }
    #[cfg(test)]
    pub(super) fn hold_evaluation_driver_fixture(
        &self,
        id: &str,
    ) -> Result<std::fs::File, ApiError> {
        Ok(crate::evaluation::recovery::resume_driver(
            &self.db,
            &self.repo_root,
            id,
        )?)
    }

    pub fn inspect_evaluation_recovery(
        &self,
    ) -> Result<crate::evaluation::RecoveryReport, ApiError> {
        Ok(crate::evaluation::recovery::inspect(
            &self.db,
            &self.repo_root,
        )?)
    }
}

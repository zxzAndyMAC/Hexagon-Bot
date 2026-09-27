//! D06: observe current evidence without rewriting historical completion.
use super::*;
use crate::evaluation as eval;
impl Workbench {
    pub fn inspect_evaluation_outcome(
        &self,
        run_id: &str,
    ) -> Result<eval::OutcomeObservation, ApiError> {
        Ok(eval::outcome::inspect(&self.db, run_id)?)
    }
    /// Historical observation, explicitly not a fresh qualification check.
    pub fn evaluation_outcome_history(
        &self,
        run_id: &str,
    ) -> Result<Vec<eval::OutcomeObservation>, ApiError> {
        let mut q = self.db.conn().prepare(
            "SELECT observation_json FROM evaluation_outcomes WHERE run_id=?1 ORDER BY rowid",
        )?;
        let rows = q
            .query_map([run_id], |r| r.get::<_, String>(0))?
            .collect::<Result<Vec<_>, _>>()?;
        rows.into_iter()
            .map(|v| serde_json::from_str(&v).map_err(Into::into))
            .collect()
    }
    /// D06: uses the host validator in a fresh OS-sandboxed copy, with no model
    /// calls or external services. Even a pass cannot change a terminal run.
    pub fn recheck_evaluation_delivery(&self, run_id: &str) -> Result<eval::Acceptance, ApiError> {
        let run = eval::read(&self.db, run_id)?;
        let json: String = self.db.conn().query_row(
            "SELECT task_json FROM evaluation_runs WHERE id=?1",
            [run_id],
            |r| r.get(0),
        )?;
        let task: eval::EvaluationTask = serde_json::from_str(&json)?;
        let accepted = eval::accept(
            &self.db,
            Path::new(&run.workspace),
            &task,
            run.git_baseline.as_ref(),
        )?;
        let id = format!("recheck-{}", self.db.next_id("evaluation_recheck")?);
        self.db.conn().execute(
            "INSERT INTO evaluation_readonly_checks(id,run_id,acceptance_json) VALUES (?1,?2,?3)",
            rusqlite::params![id, run_id, serde_json::to_string(&accepted)?],
        )?;
        Ok(accepted)
    }
}

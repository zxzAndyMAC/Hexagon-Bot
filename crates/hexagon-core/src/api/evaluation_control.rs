//! D09: control state is separate from success/acceptance.
use super::*;
use crate::evaluation as eval;
impl Workbench {
    pub fn pause_evaluation_run(&self, run: &str) -> Result<eval::EvaluationControl, ApiError> {
        Ok(eval::control::pause(&self.db, run, true)?)
    }
    pub fn resume_evaluation_run(&self, run: &str) -> Result<eval::EvaluationControl, ApiError> {
        Ok(eval::control::pause(&self.db, run, false)?)
    }

    pub fn stop_evaluation_run(&self, run: &str) -> Result<eval::EvaluationControl, ApiError> {
        Ok(eval::control::stop(&self.db, run)?)
    }
    pub fn evaluation_control(&self, run: &str) -> Result<eval::EvaluationControl, ApiError> {
        Ok(eval::control::read(&self.db, run)?)
    }
}

#[cfg(test)]
impl Workbench {
    pub(super) fn hold_evaluation_work_fixture(&self, id: &str) -> Result<std::fs::File, ApiError> {
        let run = eval::read(&self.db, id)?;
        eval::control::work_lease(Path::new(&run.workspace))?
            .ok_or_else(|| ApiError::BadInput("fixture must be a controlled worker".into()))
    }

    pub(super) fn resume_evaluation_fixture(
        &self,
        id: &str,
    ) -> Result<eval::EvaluationResult, ApiError> {
        // A budget test exercises native requests during an active driver phase;
        // it is never a human-attention or real-model completion fixture.
        let mut run = eval::read(&self.db, id)?;
        if run.state != "waiting_human" {
            return Err(ApiError::BadInput("fixture must be waiting".into()));
        }
        run.state = "started".into();
        eval::update_started(&self.db, &run)?;
        Ok(run)
    }
}

//! D07: owner interactions use the same business facade as ordinary projects.
use super::evaluation_runner::{drive_scripted, ScriptCursor};
use super::*;
use crate::evaluation::{self as eval, AttentionHandle, EvaluationActor, EvaluationDecision};

impl Workbench {
    pub fn evaluation_pending(
        &self,
        run_id: &str,
    ) -> Result<Vec<crate::cards::QueuedCard>, ApiError> {
        let run = eval::read(&self.db, run_id)?;
        let db = Db::open(Path::new(&run.workspace).join(".hexagon/state.db"))?;
        Ok(crate::cards::queued(&db, crate::PROJECT_ID)?)
    }
    pub fn begin_evaluation_attention(
        &self,
        run_id: &str,
        actor: EvaluationActor,
    ) -> Result<AttentionHandle, ApiError> {
        Ok(eval::human::begin(
            &self.db,
            &self.repo_root,
            run_id,
            actor,
        )?)
    }
    pub fn end_evaluation_attention(
        &self,
        handle: &AttentionHandle,
        reason: eval::AttentionEnd,
    ) -> Result<eval::HumanInterval, ApiError> {
        // Ticket09 follow-up: closing attention and advancing its fingerprint
        // must be atomic, or a concurrent begin falsely reports unmeasured work.
        let tx = rusqlite::Transaction::new_unchecked(
            self.db.conn(),
            rusqlite::TransactionBehavior::Immediate,
        )?;
        let interval = eval::human::end(&self.db, &self.repo_root, handle, reason)?;
        tx.commit()?;
        Ok(interval)
    }
    pub fn evaluation_timing(&self, run_id: &str) -> Result<eval::RunTiming, ApiError> {
        Ok(eval::human::timing(&self.db, run_id)?)
    }
    pub fn add_evaluation_guidance(
        &self,
        handle: &AttentionHandle,
        body: &str,
    ) -> Result<(), ApiError> {
        eval::human::validate_handle(&self.db, &self.repo_root, handle)?;
        if body.trim().is_empty() {
            return Err(owner_refusal("guidance_empty"));
        }
        let run = eval::read(&self.db, &handle.run_id)?;
        let worker =
            Workbench::open_scoped(Path::new(&run.workspace), &run.task_id, &[], None, false)?;
        // D07: put actual extra input on the ordinary owner-message history;
        // count it inside the already-open attention interval, never add a
        // second duration for the same owner's concurrent work.
        worker
            .db
            .append_message(&worker.project_id, "owner", body, &[], &[], None, None)?;
        self.db.conn().execute("UPDATE evaluation_human_intervals SET guidance_count=guidance_count+1 WHERE id=?1 AND ended_at_ms IS NULL",[handle.id()])?;
        Ok(())
    }

    pub fn submit_evaluation_decision(
        &self,
        handle: &AttentionHandle,
        decision: EvaluationDecision,
    ) -> Result<eval::EvaluationResult, ApiError> {
        eval::human::validate_handle(&self.db, &self.repo_root, handle)?;
        let mut result = eval::read(&self.db, &handle.run_id)?;
        if result.state != "waiting_human" {
            return Err(owner_refusal("run_not_waiting"));
        }
        let cursor_json: String = self.db.conn().query_row(
            "SELECT cursor_json FROM evaluation_cursors WHERE run_id=?1",
            [&result.id],
            |r| r.get(0),
        )?;
        let mut cursor: ScriptCursor = serde_json::from_str(&cursor_json)?;
        let task_json: String = self.db.conn().query_row(
            "SELECT task_json FROM evaluation_runs WHERE id=?1",
            [&result.id],
            |r| r.get(0),
        )?;
        let task: eval::EvaluationTask = serde_json::from_str(&task_json)?;
        let mut worker = Workbench::open_scoped(
            Path::new(&result.workspace),
            &task.id,
            &cursor.roles,
            cursor.pack.clone(),
            false,
        )?;
        if cursor.permission.is_some()
            && !matches!(&decision, EvaluationDecision::Permission { .. })
        {
            return Err(owner_refusal("permission_decision_required"));
        }
        match &decision {
            EvaluationDecision::ContinueRework if !cursor.rework => {
                return Err(owner_refusal("run_not_in_rework"))
            }
            EvaluationDecision::FinishReview if cursor.pack.is_some() => {
                return Err(owner_refusal("full_workflow_requires_business_decision"))
            }
            EvaluationDecision::Permission { question_id, .. }
                if cursor.permission.as_ref().map(|(id, _)| id.as_str())
                    != Some(question_id.as_str()) =>
            {
                return Err(owner_refusal("permission_not_current"))
            }
            _ => {}
        }
        // D07: submission ends human work before an approved tool executes.
        // Charging tool/network latency as human attention biases the comparison.
        let tx = rusqlite::Transaction::new_unchecked(
            self.db.conn(),
            rusqlite::TransactionBehavior::Immediate,
        )?;
        eval::human::end(
            &self.db,
            &self.repo_root,
            handle,
            eval::AttentionEnd::Submitted,
        )?;
        self.db.conn().execute("UPDATE evaluation_cursors SET waiting_ms=waiting_ms+MAX(0,?2-COALESCE(waiting_since_ms,?2)),waiting_since_ms=NULL WHERE run_id=?1",rusqlite::params![result.id,eval::now_ms()? as i64])?;
        let started = std::time::Instant::now();
        result.state = "started".into();
        eval::control::apply_stop(&self.db, &mut result)?;
        eval::update_started(&self.db, &result)?;
        tx.commit()?;
        let action = (|| -> Result<Option<orchestra::StageAction>, ApiError> {
            let _watch = eval::control::watch(
                Path::new(&result.workspace),
                worker.sessions.clone(),
                worker.tasks.clone(),
            )?;
            eval::control::checkpoint(Path::new(&result.workspace))?;
            Ok(match decision {
                EvaluationDecision::ContinueRework => {
                    // D07: manual edits and guidance are already in this workspace.
                    // Recheck the reopened stage through the normal business gate;
                    // do not replay the old activation to get back to a stamp.
                    Some(worker.advance()?)
                }
                EvaluationDecision::FinishReview => Some(orchestra::StageAction::PackFinished),
                EvaluationDecision::ApproveStamp => Some(worker.stamp()?),
                EvaluationDecision::RejectStamp => Some(worker.reject_stamp()?),
                EvaluationDecision::RejectFinal { stage, note } => {
                    Some(worker.reject_final(&stage, &note)?)
                }
                EvaluationDecision::Permission { question_id, allow } => {
                    worker.answer_permission(&question_id, allow, None, "once")?;
                    None
                }
            })
        })();
        let action = match action {
            Ok(action) => action,
            Err(error) => {
                result.state = "waiting_human".into();
                result.error = Some(error.to_string());
                result.elapsed_ms = result
                    .elapsed_ms
                    .saturating_add(started.elapsed().as_millis().min(u64::MAX as u128) as u64);
                self.save_evaluation_cursor(&cursor, &result)?;
                eval::control::apply_stop(&self.db, &mut result)?;
                eval::update_started(&self.db, &result)?;
                if result.state != "waiting_human" {
                    self.finish_evaluation_owner_plan(&result)?;
                }
                return Err(error);
            }
        };
        result.error = None;
        let execution = (|| -> Result<(), ApiError> {
            match action {
                None => drive_scripted(&mut worker, &task, &mut cursor, &mut result)?,
                Some(orchestra::StageAction::PackFinished) => {
                    result.flow_completed = cursor.pack.is_some();
                    result.state = "completed".into();
                    let acceptance = eval::accept(
                        &worker.db,
                        Path::new(&result.workspace),
                        &task,
                        result.git_baseline.as_ref(),
                    )?;
                    result.independent_passed = acceptance.passed;
                    result.acceptance = Some(acceptance);
                }
                Some(orchestra::StageAction::StageOpened { seq, .. }) => {
                    cursor.stage = seq;
                    if cursor.rework {
                        // Ticket09 review: the original script has been consumed.
                        // Keep each reopened stage available for measured manual
                        // continuation; resetting the old cursor replays side effects.
                        result.state = "waiting_human".into();
                    } else {
                        drive_scripted(&mut worker, &task, &mut cursor, &mut result)?;
                    }
                }
                Some(
                    orchestra::StageAction::Rewound { to_seq, .. }
                    | orchestra::StageAction::StampRejected {
                        reopened_seq: to_seq,
                        ..
                    },
                ) => {
                    // D07: a rejection is rework, never replay the original tools.
                    cursor.stage = to_seq;
                    cursor.role_index = 0;
                    cursor.rework = true;
                    result.state = "waiting_human".into();
                }
                Some(
                    orchestra::StageAction::AwaitingStamp { .. }
                    | orchestra::StageAction::WaitingStamp { .. },
                ) => {
                    cursor.rework = false;
                    result.state = "waiting_human".into();
                }
                Some(orchestra::StageAction::Incomplete { missing, .. }) => {
                    cursor.rework = true;
                    result.state = "waiting_human".into();
                    result.error = Some(format!("rework incomplete: {}", missing.join(", ")));
                }
            }
            Ok(())
        })();
        if let Err(error) = execution {
            result.state = "failed".into();
            result.error = Some(error.to_string());
        }
        result.elapsed_ms = result
            .elapsed_ms
            .saturating_add(started.elapsed().as_millis().min(u64::MAX as u128) as u64);
        self.save_evaluation_cursor(&cursor, &result)?;
        eval::control::apply_stop(&self.db, &mut result)?;
        eval::update_started(&self.db, &result)?;
        if result.state != "waiting_human" {
            self.finish_evaluation_owner_plan(&result)?;
        }
        Ok(result)
    }
    fn finish_evaluation_owner_plan(
        &self,
        result: &eval::EvaluationResult,
    ) -> Result<(), ApiError> {
        let (plan, position): (String, i64) = self.db.conn().query_row(
            "SELECT plan_id,position FROM evaluation_plan_runs WHERE run_id=?1",
            [&result.id],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )?;
        eval::plan::finish(
            &self.db,
            &plan,
            usize::try_from(position).map_err(std::io::Error::other)?,
            &result.id,
        )?;
        Ok(())
    }
}

fn owner_refusal(reason: &str) -> ApiError {
    crate::diag::note(
        crate::diag::CLASS_REJECT,
        true,
        Some(crate::PROJECT_ID),
        None,
        None,
        None,
        "evaluation_owner_decision",
        reason,
        std::time::Instant::now(),
    );
    ApiError::BadInput(format!("evaluation owner decision: {reason}"))
}

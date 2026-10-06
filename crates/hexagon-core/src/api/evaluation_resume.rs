//! Restore the observations of an evaluation activation, never its side effects.
use super::*;
use crate::provider::{ContentBlock, Message, Role};
use crate::tools::CallOutcome;
use serde_json::Value;

type RestoredObservations = (Vec<Message>, bool, Vec<(Value, Value)>);

pub(super) struct ActivationResume {
    pub turn_id: i64,
    pub root_turn_id: i64,
    pub instruction: String,
    pub history: Vec<Message>,
    pub source_reviewed: bool,
    pub task_operations: Vec<(Value, Value)>,
}

impl Workbench {
    /// Owner 2026-10-06 / I2: refusal is final even without a model. Reuse the
    /// existing Stall retry control for the original activation, never turn the
    /// answered permission back into an approvable card or interrupt peer work.
    pub(super) fn queue_activation_resume(
        &self,
        aid: &str,
        run: Option<&str>,
        resume: &ActivationResume,
    ) -> Result<(), ApiError> {
        let started = std::time::Instant::now();
        let key = format!(
            "activation_resume:{}:{}:{}",
            self.project_id, resume.root_turn_id, resume.turn_id
        );
        if crate::cards::find_by_idem(&self.db, aid, &key)?.is_some_and(|card| {
            card.project_id == self.project_id
                && card.kind == "stall"
                && card.state == crate::cards::CardState::Queued
        }) {
            return Ok(());
        }
        let role = self.instance_role(aid)?;
        let qid = crate::cards::enqueue(
            &self.db,
            &self.project_id,
            Some(aid),
            crate::cards::CardKind::Stall,
            json!({"branch":"no_reply","retry":true,"role":role,"instruction":resume.instruction,
                "run_id":run,"source":"activation_resume","reason":"resume_provider_unavailable",
                "trigger_turn_id":resume.turn_id,"activation_root_turn_id":resume.root_turn_id}),
            Some(&key),
        )?;
        // Do not use the generic stall owner note: it claims that a retrigger
        // already ran. Admission never started a model request in this branch.
        self.db.append_event(
            &self.project_id,
            EventKind::System,
            json!({"kind":"stall_carded","branch":"no_reply","question_id":qid,"retry":true,
                "source":"activation_resume","reason":"resume_provider_unavailable",
                "turn_id":resume.turn_id,"activation_root_turn_id":resume.root_turn_id}),
            Some(aid),
            run,
        )?;
        crate::diag::note(
            crate::diag::CLASS_REJECT,
            true,
            Some(&self.project_id),
            Some(aid),
            run,
            Some(&resume.turn_id.to_string()),
            "activation_resume",
            "provider_retry_retained",
            started,
        );
        Ok(())
    }

    /// Strict sibling of permission/recovery/context continuation. The old
    /// generic Stall route has no history contract; host-marked packets may not
    /// silently fall back to it when their identity is malformed or superseded.
    pub(super) fn retry_activation_resume(
        &self,
        card: &crate::cards::Card,
    ) -> Result<StallTick, ApiError> {
        let started = std::time::Instant::now();
        let reject = |reason: &str| {
            crate::diag::note(
                crate::diag::CLASS_REJECT,
                true,
                Some(&self.project_id),
                card.agent_id.as_deref(),
                card.payload["run_id"].as_str(),
                Some(&card.id),
                "activation_resume_retry",
                reason,
                started,
            );
            ApiError::BadInput(format!("cannot retry activation: {reason}"))
        };
        let aid = card
            .agent_id
            .as_deref()
            .ok_or_else(|| reject("instance_missing"))?;
        if card.project_id != self.project_id
            || card.payload["source"] != "activation_resume"
            || card.payload["branch"] != "no_reply"
            || card.payload["retry"] != true
        {
            return Err(reject("card_scope_invalid"));
        }
        let run = match card.payload.get("run_id") {
            Some(Value::Null) => None,
            Some(Value::String(run)) if !run.is_empty() => Some(run.as_str()),
            _ => return Err(reject("run_scope_missing")),
        };
        if self.active_run()?.as_ref().map(|run| run.id.as_str()) != run {
            return Err(reject("run_superseded"));
        }
        let parent = card.payload["trigger_turn_id"]
            .as_i64()
            .filter(|id| *id > 0)
            .ok_or_else(|| reject("parent_missing"))?;
        self.instance_role(aid)?;
        let resume = self.activation_resume(aid, run, Some(parent))?;
        if card.payload["activation_root_turn_id"].as_i64() != Some(resume.root_turn_id)
            || card.payload["instruction"].as_str() != Some(resume.instruction.as_str())
        {
            return Err(reject("packet_identity_mismatch"));
        }
        self.turn_provider(aid)?;
        // Missing configuration keeps the card queued and does not consume the
        // ordinary stall episode retry. Only admitted execution closes this card.
        crate::cards::answer(&self.db, &card.id, "owner")?;
        self.run_turn_agent_with_history(
            aid,
            &resume.instruction,
            &[],
            false,
            &resume.history,
            Some(&resume),
            true,
        )?;
        Ok(StallTick::Retriggered {
            agent_id: aid.to_string(),
        })
    }

    /// I2 / V05 (2026-10-06): cursors prove delivery, not retained task state.
    /// False rejection costs a manual instruction; false continuation can revive
    /// an obsolete task with side effects. Missing/cross-scope parents fail closed.
    pub(super) fn activation_resume(
        &self,
        aid: &str,
        run: Option<&str>,
        turn_id: Option<i64>,
    ) -> Result<ActivationResume, ApiError> {
        use rusqlite::OptionalExtension;
        let started = std::time::Instant::now();
        let reject = |reason: &str| {
            crate::diag::note(
                crate::diag::CLASS_REJECT,
                true,
                Some(&self.project_id),
                Some(aid),
                run,
                turn_id.as_ref().map(|id| id.to_string()).as_deref(),
                "activation_resume",
                reason,
                started,
            );
            ApiError::BadInput(format!("cannot resume activation: {reason}"))
        };
        let mut current = self
            .db
            .conn()
            .query_row(
                "SELECT id,payload FROM events WHERE project_id=?1 AND agent_id=?2
             AND stage_run_id IS ?3 AND kind='turn_started'
             AND COALESCE(json_extract(payload,'$.subagent'),0)=0
             AND (?4 IS NULL OR id=?4) ORDER BY id DESC LIMIT 1",
                rusqlite::params![self.project_id, aid, run, turn_id],
                |r| Ok((r.get::<_, i64>(0)?, r.get::<_, String>(1)?)),
            )
            .optional()?
            .ok_or_else(|| reject("parent_turn_missing"))?;
        let parent = current.0;
        let mut turns = std::collections::HashSet::new();
        let mut claimed_roots = Vec::new();
        loop {
            turns.insert(current.0);
            let payload: Value = serde_json::from_str(&current.1)?;
            if let Some(claimed) = payload["activation_root_turn_id"].as_i64() {
                claimed_roots.push(claimed);
            }
            let Some(link) = payload
                .get("resume_from_turn_id")
                .filter(|link| !link.is_null())
            else {
                break;
            };
            let previous = link
                .as_i64()
                .filter(|id| *id > 0 && *id < current.0)
                .ok_or_else(|| reject("parent_chain_invalid"))?;
            current = self
                .db
                .conn()
                .query_row(
                    "SELECT id,payload FROM events WHERE project_id=?1 AND agent_id=?2
                 AND stage_run_id IS ?3 AND kind='turn_started' AND id=?4
                 AND COALESCE(json_extract(payload,'$.subagent'),0)=0",
                    rusqlite::params![self.project_id, aid, run, previous],
                    |r| Ok((r.get::<_, i64>(0)?, r.get::<_, String>(1)?)),
                )
                .optional()?
                .ok_or_else(|| reject("parent_chain_scope_mismatch"))?;
        }
        let root = current.0;
        if claimed_roots.iter().any(|claimed| *claimed != root) {
            return Err(reject("root_chain_mismatch"));
        }
        let payload: Value = serde_json::from_str(&current.1)?;
        let instruction = payload["instruction"]
            .as_str()
            .filter(|s| !s.trim().is_empty())
            .ok_or_else(|| reject("instruction_missing"))?
            .to_string();
        if run.is_some() {
            let floor: Option<i64> = self.db.conn().query_row(
                "SELECT MAX(id) FROM events WHERE project_id=?1 AND kind='stage_started'
                 AND stage_run_id IS ?2 AND id<=?3",
                rusqlite::params![self.project_id, run, root],
                |r| r.get(0),
            )?;
            if floor.is_none() {
                return Err(reject("stage_boundary_missing"));
            }
        }
        let superseded: bool = self.db.conn().query_row(
            "SELECT EXISTS(SELECT 1 FROM events WHERE project_id=?1 AND agent_id=?2
             AND id>?3 AND (kind='fastpath_dispatched' OR
               (kind='turn_started' AND COALESCE(json_extract(payload,'$.subagent'),0)=0
                AND json_extract(payload,'$.resume_from_turn_id') IS NULL)))",
            rusqlite::params![self.project_id, aid, root],
            |r| r.get(0),
        )?;
        if superseded {
            return Err(reject("dispatch_superseded"));
        }
        let (history, source_reviewed, task_operations) = self
            .exact_activation_history(aid, root, run, &turns)
            .inspect_err(|_| {
                crate::diag::note(
                    crate::diag::CLASS_REJECT,
                    true,
                    Some(&self.project_id),
                    Some(aid),
                    run,
                    Some(&parent.to_string()),
                    "activation_resume",
                    "history_evidence_rejected",
                    started,
                );
            })?;
        crate::diag::note(
            crate::diag::CLASS_JUDGE,
            false,
            Some(&self.project_id),
            Some(aid),
            run,
            Some(&parent.to_string()),
            "activation_resume",
            "exact_parent_restored",
            started,
        );
        Ok(ActivationResume {
            turn_id: parent,
            root_turn_id: root,
            instruction,
            history,
            source_reviewed,
            task_operations,
        })
    }

    /// Project confirmed protocol observations from the validated parent chain.
    /// No calls are executed here. Unknown outcomes stop before a model request.
    fn exact_activation_history(
        &self,
        aid: &str,
        root: i64,
        run: Option<&str>,
        turns: &std::collections::HashSet<i64>,
    ) -> Result<RestoredObservations, ApiError> {
        use rusqlite::OptionalExtension;
        let end: i64 = self.db.conn().query_row(
            "SELECT COALESCE(MIN(id),9223372036854775807) FROM events
             WHERE project_id=?1 AND agent_id=?2 AND kind='fastpath_dispatched' AND id>?3",
            rusqlite::params![self.project_id, aid, root],
            |r| r.get(0),
        )?;
        let owning_turn = |id: i64| -> Result<Option<i64>, ApiError> {
            Ok(self
                .db
                .conn()
                .query_row(
                    "SELECT id FROM events WHERE project_id=?1 AND agent_id=?2
                 AND stage_run_id IS ?3 AND kind='turn_started' AND id<?4
                 AND COALESCE(json_extract(payload,'$.subagent'),0)=0 ORDER BY id DESC LIMIT 1",
                    rusqlite::params![self.project_id, aid, run, id],
                    |r| r.get(0),
                )
                .optional()?)
        };
        let mut timeline: Vec<(i64, Vec<Message>)> = Vec::new();
        let mut source_reviewed = false;
        let mut task_operations = Vec::new();
        let mut observed_requests = std::collections::HashSet::new();
        let mut st = self.db.conn().prepare(
            "SELECT id,payload FROM events WHERE project_id=?1 AND agent_id=?2
             AND stage_run_id IS ?3 AND id>?4 AND id<?5 AND kind='system'
             AND json_extract(payload,'$.kind')='activation_observation'
             AND COALESCE(json_extract(payload,'$.subagent'),0)=0 ORDER BY id",
        )?;
        for row in st.query_map(
            rusqlite::params![self.project_id, aid, run, root, end],
            |r| Ok((r.get::<_, i64>(0)?, r.get::<_, String>(1)?)),
        )? {
            let (id, payload) = row?;
            let payload: Value = serde_json::from_str(&payload)?;
            let Some(turn) = payload["turn_id"].as_i64().filter(|id| turns.contains(id)) else {
                continue;
            };
            if payload["activation_root_turn_id"].as_i64() != Some(root) {
                return Err(ApiError::BadInput(
                    "observation root identity mismatch".into(),
                ));
            }
            let request_id = payload["request_id"]
                .as_i64()
                .ok_or_else(|| ApiError::BadInput("observation request missing".into()))?;
            let valid: bool = self.db.conn().query_row(
                "SELECT EXISTS(SELECT 1 FROM events WHERE id=?1 AND project_id=?2
                 AND agent_id=?3 AND stage_run_id IS ?4 AND kind='system'
                 AND json_extract(payload,'$.kind')='request_envelope'
                 AND json_extract(payload,'$.turn_id')=?5
                 AND COALESCE(json_extract(payload,'$.subagent'),0)=0)",
                rusqlite::params![request_id, self.project_id, aid, run, turn],
                |r| r.get(0),
            )?;
            if !valid {
                return Err(ApiError::BadInput(
                    "observation request scope mismatch".into(),
                ));
            }
            // Source fresh-review clears protocol history in the live loop.
            // Reopening must retain that host reset, not revive the positive
            // draft or its native reasoning from older durable observations.
            if payload["source_reviewed"] == true {
                source_reviewed = true;
                if payload["reset_history"] == true {
                    for (_, messages) in &mut timeline {
                        messages.retain(|message| message.role == Role::User);
                    }
                }
            }
            let content: Vec<ContentBlock> = serde_json::from_value(payload["content"].clone())?;
            let mut assistant = Vec::new();
            let mut results = Vec::new();
            for block in content {
                match &block {
                    // ADR 0066 / source native contract: the adapter's boolean
                    // is protocol provenance; messages.thinking is presentation.
                    ContentBlock::Thinking {
                        replay_as_reasoning_content: false,
                        ..
                    } => continue,
                    ContentBlock::ToolUse {
                        id: call_id,
                        name,
                        input,
                    } => {
                        let action: Option<String> = self
                            .db
                            .conn()
                            .query_row(
                                "SELECT id FROM tool_actions WHERE project_id=?1 AND agent_id=?2
                             AND stage_run_id IS ?3 AND request_id=?4 AND tool_call_id=?5",
                                rusqlite::params![self.project_id, aid, run, request_id, call_id],
                                |r| r.get(0),
                            )
                            .optional()?;
                        let result = if let Some(action_id) = action {
                            let action =
                                crate::actions::get(&self.db, &self.project_id, &action_id)?;
                            if action.tool != *name || action.input != *input {
                                return Err(ApiError::BadInput(
                                    "native tool/action identity mismatch".into(),
                                ));
                            }
                            let result = self.activation_action_result(&action, call_id)?;
                            if let Some(operation) = confirmed_task_operation(&action) {
                                task_operations.push(operation);
                            }
                            result
                        } else {
                            // I2 owner ruling 2026-10-06: an unprepared batch
                            // tail was never started; do not fabricate success or
                            // run it during restore. Keep its factual parameters.
                            crate::diag::note(
                                crate::diag::CLASS_JUDGE,
                                false,
                                Some(&self.project_id),
                                Some(aid),
                                run,
                                Some(&request_id.to_string()),
                                "activation_resume",
                                "tool_tail_not_started",
                                std::time::Instant::now(),
                            );
                            resume_error(call_id,"Host did not prepare or start this call. No execution result exists; do not assume success.".into())
                        };
                        results.push(result);
                    }
                    _ => {}
                }
                assistant.push(block);
            }
            let mut messages = Vec::new();
            if !assistant.is_empty() {
                messages.push(Message {
                    role: Role::Assistant,
                    content: assistant,
                });
            }
            if !results.is_empty() {
                messages.push(Message {
                    role: Role::Tool,
                    content: results,
                });
            }
            timeline.push((id, messages));
            observed_requests.insert(request_id);
        }
        // Upgrade path: old request envelopes contain hashes, not native blocks.
        // Restore only their durable tool receipts, with an exact parent boundary.
        let mut st = self.db.conn().prepare(
            "SELECT a.id,a.request_id,r.payload FROM tool_actions a JOIN events r ON r.id=a.request_id
             WHERE a.project_id=?1 AND a.agent_id=?2 AND a.stage_run_id IS ?3
             AND a.request_id>?4 AND a.request_id<?5 AND r.project_id=a.project_id
             AND r.agent_id=a.agent_id AND r.stage_run_id IS a.stage_run_id
             AND json_extract(r.payload,'$.kind')='request_envelope'
             AND COALESCE(json_extract(r.payload,'$.subagent'),0)=0 ORDER BY a.request_id,a.rowid")?;
        for row in st.query_map(
            rusqlite::params![self.project_id, aid, run, root, end],
            |r| {
                Ok((
                    r.get::<_, String>(0)?,
                    r.get::<_, i64>(1)?,
                    r.get::<_, String>(2)?,
                ))
            },
        )? {
            let (id, request, envelope) = row?;
            if observed_requests.contains(&request) {
                continue;
            }
            let envelope: Value = serde_json::from_str(&envelope)?;
            let owner =
                match envelope["turn_id"].as_i64() {
                    Some(turn) => Some(turn),
                    None => {
                        // I2 legacy regression (2026-10-06): filtering out children
                        // guessed a parent for a child request. A false rejection costs
                        // one owner retry; false acceptance imports another worker's
                        // authorization/results. Only the immediately preceding turn
                        // can identify an old envelope without host identity.
                        let previous: Option<(i64, bool)> = self.db.conn().query_row(
                        "SELECT id,COALESCE(json_extract(payload,'$.subagent'),0) FROM events
                         WHERE project_id=?1 AND agent_id=?2 AND stage_run_id IS ?3
                         AND kind='turn_started' AND id<?4 ORDER BY id DESC LIMIT 1",
                        rusqlite::params![self.project_id,aid,run,request],
                        |r| Ok((r.get(0)?,r.get(1)?)),
                    ).optional()?;
                        match previous {
                            Some((_, true)) => {
                                return Err(ApiError::BadInput(
                                    "legacy request parent is ambiguous after child turn".into(),
                                ))
                            }
                            Some((turn, false)) => Some(turn),
                            None => None,
                        }
                    }
                };
            if !owner.is_some_and(|turn| turns.contains(&turn)) {
                continue;
            }
            let action = crate::actions::get(&self.db, &self.project_id, &id)?;
            let result = self.activation_action_result(&action, &id)?;
            if let Some(operation) = confirmed_task_operation(&action) {
                task_operations.push(operation);
            }
            timeline.push((
                request,
                vec![
                    Message {
                        role: Role::Assistant,
                        content: vec![ContentBlock::ToolUse {
                            id,
                            name: action.tool,
                            input: action.input,
                        }],
                    },
                    Message {
                        role: Role::Tool,
                        content: vec![result],
                    },
                ],
            ));
        }
        let mut st=self.db.conn().prepare(
            "SELECT e.id,m.body FROM messages m JOIN events e ON m.id=json_extract(e.payload,'$.message_id')
             WHERE m.project_id=?1 AND e.project_id=?1 AND m.author='owner'
             AND e.kind='owner_message' AND e.id>?2 AND e.id<?3 ORDER BY e.id")?;
        for row in st.query_map(rusqlite::params![self.project_id, root, end], |r| {
            Ok((r.get::<_, i64>(0)?, r.get::<_, String>(1)?))
        })? {
            let (id, body) = row?;
            if owning_turn(id)?.is_some_and(|turn| turns.contains(&turn))
                && crate::commands::parse_command(&body).is_none()
            {
                timeline.push((
                    id,
                    vec![Message {
                        role: Role::User,
                        content: vec![ContentBlock::Text {
                            text: json!({"steering":body}).to_string(),
                        }],
                    }],
                ));
            }
        }
        // I3: validate before a recovery unlock or card answer. This is a pure
        // projection of confirmed receipts, not a guessed todo list or tool run.
        crate::subagent::TaskBoard::validate_operations(&task_operations)?;
        timeline.sort_by_key(|(id, _)| *id);
        Ok((
            timeline
                .into_iter()
                .flat_map(|(_, messages)| messages)
                .collect(),
            source_reviewed,
            task_operations,
        ))
    }

    fn activation_action_result(
        &self,
        action: &crate::actions::Action,
        id: &str,
    ) -> Result<ContentBlock, ApiError> {
        Ok(match crate::actions::replay(action) {
            Ok(Some(CallOutcome::Done(value)))=>turn::tool_result_block(id.to_string(),&action.tool,&value),
            Ok(Some(CallOutcome::Denied(reason)))=>resume_error(id,format!("denied: {reason}")),
            Err(crate::tools::ToolError::Exec(reason)) if action.state=="failed"=>resume_error(id,format!("error: {reason}")),
            Err(crate::tools::ToolError::OutcomeUnknown(_)) if crate::actions::has_resolution(&self.db,&action.id)?=>
                resume_error(id,"Outcome remains unknown; the owner resolved this action. Do not replay it or assume success.".into()),
            Err(error)=>{
                crate::diag::note(crate::diag::CLASS_REJECT,true,Some(&self.project_id),Some(&action.agent_id),
                    action.stage_run_id.as_deref(),Some(&action.id),"activation_resume","action_result_unconfirmed",std::time::Instant::now());
                return Err(error.into());
            }
            _=>resume_error(id,format!("No execution result recorded; action state: {}. This is not evidence of success.",action.state)),
        })
    }

    // Test-only durable receipt projection for legacy action fixtures (including
    // request_id=1 without a turn). Production always uses the validated chain;
    // an arbitrary zero boundary is never an automatic continuation fallback.
    #[cfg(test)]
    pub(super) fn recorded_activation_history(
        &self,
        aid: &str,
        boundary: i64,
    ) -> Result<Vec<Message>, ApiError> {
        let run = self.active_run()?.map(|run| run.id);
        let mut st = self.db.conn().prepare(
            "SELECT id FROM tool_actions WHERE project_id=?1
            AND agent_id=?2 AND stage_run_id IS ?3 AND request_id>?4 ORDER BY request_id,rowid",
        )?;
        let ids = st
            .query_map(
                rusqlite::params![self.project_id, aid, run, boundary],
                |r| r.get::<_, String>(0),
            )?
            .collect::<Result<Vec<_>, _>>()?;
        let mut history = Vec::new();
        for id in ids {
            let action = crate::actions::get(&self.db, &self.project_id, &id)?;
            let result = self.activation_action_result(&action, &id)?;
            history.push(Message {
                role: Role::Assistant,
                content: vec![ContentBlock::ToolUse {
                    id,
                    name: action.tool,
                    input: action.input,
                }],
            });
            history.push(Message {
                role: Role::Tool,
                content: vec![result],
            });
        }
        Ok(history)
    }
}

// Host provenance is separate from model input: overwrite the marker rather
// than trusting a tool parameter. Completed subagent receipts restore metadata,
// never worker execution. Unknown/unprepared actions never reach this collector.
fn confirmed_task_operation(action: &crate::actions::Action) -> Option<(Value, Value)> {
    if action.state != "succeeded" || !matches!(action.tool.as_str(), "tasks" | "subagent") {
        return None;
    }
    let Ok(Some(CallOutcome::Done(output))) = crate::actions::replay(action) else {
        return None;
    };
    let mut input = action.input.clone();
    input["host_tool"] = json!(action.tool);
    Some((input, output))
}

fn resume_error(id: &str, content: String) -> ContentBlock {
    ContentBlock::ToolResult {
        tool_use_id: id.into(),
        content,
        is_error: true,
        images: vec![],
    }
}

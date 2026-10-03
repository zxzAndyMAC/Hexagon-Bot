//! Restore the observations of an evaluation activation, never its side effects.
use super::*;
use crate::provider::{ContentBlock, Message, Role};
use crate::tools::CallOutcome;

impl Workbench {
    pub(super) fn evaluation_resume_history(&self, aid: &str) -> Result<Vec<Message>, ApiError> {
        // 2026-09-28 live pilot: a generic "continue" lost the original task,
        // successful interpreter result and three owner guidance messages. Do
        // not replay calls or insert trace text as owner instructions. Rebuild
        // paired assistant/tool observations from durable action identities.
        // The last dispatch bounds this activation, even across CLI reopen or
        // repeated permission waits. Other instances/older activations stay out.
        let dispatch: i64 = self.db.conn().query_row(
            "SELECT id FROM events WHERE project_id=?1 AND agent_id=?2
             AND kind='fastpath_dispatched' ORDER BY id DESC LIMIT 1",
            rusqlite::params![self.project_id, aid],
            |r| r.get(0),
        )?;
        self.recorded_activation_history(aid, dispatch)
    }

    pub(super) fn recorded_activation_history(
        &self,
        aid: &str,
        dispatch: i64,
    ) -> Result<Vec<Message>, ApiError> {
        let started = std::time::Instant::now();
        let ids = {
            let mut st = self.db.conn().prepare(
                "SELECT id FROM tool_actions WHERE project_id=?1 AND agent_id=?2
                 AND request_id>?3 ORDER BY request_id,rowid",
            )?;
            let ids = st
                .query_map(rusqlite::params![self.project_id, aid, dispatch], |r| {
                    r.get::<_, String>(0)
                })?
                .collect::<Result<Vec<_>, _>>()?;
            ids
        };
        let mut history = Vec::new();
        for id in ids {
            let action = crate::actions::get(&self.db, &self.project_id, &id)?;
            let result = match crate::actions::replay(&action) {
                Ok(Some(CallOutcome::Done(value))) => turn::tool_result_block(id.clone(), &action.tool, &value),
                Ok(Some(CallOutcome::Denied(reason))) => resume_error(&id, format!("denied: {reason}")),
                // A failed action has a recorded error; replay never executes it.
                Err(crate::tools::ToolError::Exec(reason)) if action.state == "failed" =>
                    resume_error(&id, format!("error: {reason}")),
                // Live acceptance 2026-10-01: an owner resolution unblocks the
                // chain but deliberately keeps the original effect unknown.
                // Restoring observations must not re-block or replay that effect.
                Err(crate::tools::ToolError::OutcomeUnknown(_))
                    if crate::actions::has_resolution(&self.db, &id)? =>
                    resume_error(&id, "Outcome remains unknown; the owner resolved this action. Do not replay it or assume success.".into()),
                Err(error) => return Err(error.into()),
                _ => resume_error(&id, format!("No execution result recorded; action state: {}. This is not evidence of success.", action.state)),
            };
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
        // Guidance entered while waiting predates the new turn's steering
        // watermark. Explicitly deliver it as owner input, in chronological
        // order within this activation. Earlier stages remain out (ADR 0021);
        // querying only @mentions silently dropped ordinary guidance.
        let mut st = self.db.conn().prepare(
            "SELECT m.body FROM messages m JOIN events e
             ON m.id=json_extract(e.payload,'$.message_id')
             WHERE m.project_id=?1 AND e.project_id=?1 AND m.author='owner'
             AND e.kind='owner_message' AND e.id>?2 ORDER BY e.id",
        )?;
        for body in st.query_map(rusqlite::params![self.project_id, dispatch], |r| {
            r.get::<_, String>(0)
        })? {
            let body = body?;
            if crate::commands::parse_command(&body).is_none() {
                history.push(Message {
                    role: Role::User,
                    content: vec![ContentBlock::Text {
                        text: json!({"steering":body}).to_string(),
                    }],
                });
            }
        }
        crate::diag::note(
            crate::diag::CLASS_JUDGE,
            false,
            Some(&self.project_id),
            Some(aid),
            None,
            Some(&dispatch.to_string()),
            "evaluation_resume",
            "recorded_context_restored",
            started,
        );
        Ok(history)
    }
}

fn resume_error(id: &str, content: String) -> ContentBlock {
    ContentBlock::ToolResult {
        tool_use_id: id.into(),
        content,
        is_error: true,
        images: vec![],
    }
}

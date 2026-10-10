use super::*;

/// Short owner control over the same registered tools and model slots. It owns
/// no execution connection and never selects a provider from global settings.
#[derive(Clone)]
pub struct PermissionControl {
    registry: Arc<Registry>,
    slots: Arc<std::sync::RwLock<HashMap<String, ()>>>,
    project_id: String,
    root: PathBuf,
}

impl Workbench {
    pub fn permission_control(&self) -> PermissionControl {
        PermissionControl {
            registry: self.registry.clone(),
            slots: self.permission_slots.clone(),
            project_id: self.project_id.clone(),
            root: self.repo_root.clone(),
        }
    }

    /// Execute only the already saved action, then continue its exact original
    /// task. A repeated background delivery never creates a second child turn.
    pub fn continue_saved_permission(&self, question_id: &str) -> Result<(), ApiError> {
        let started = std::time::Instant::now();
        let result = self.continue_saved_permission_inner(question_id);
        let card = crate::cards::get(&self.db, question_id).ok();
        let reason = result
            .as_ref()
            .err()
            .map(crate::errcode::ErrorCode::code)
            .unwrap_or_else(|| result.as_ref().copied().unwrap().into());
        let refused = result.is_err() || reason == "original_task_superseded_or_missing";
        crate::diag::note(
            if refused {
                crate::diag::CLASS_REJECT
            } else {
                crate::diag::CLASS_JUDGE
            },
            refused,
            Some(&self.project_id),
            card.as_ref().and_then(|card| card.agent_id.as_deref()),
            None,
            Some(question_id),
            "saved_permission_dispatch",
            &reason,
            started,
        );
        result.map(|_| ())
    }

    fn continue_saved_permission_inner(&self, question_id: &str) -> Result<&'static str, ApiError> {
        let card = crate::cards::get(&self.db, question_id)?;
        if card.project_id != self.project_id
            || card.kind != "permission"
            || card.state != crate::cards::CardState::Answered
        {
            return Err(ApiError::BadInput(
                "permission decision is not saved".into(),
            ));
        }
        let aid = card
            .agent_id
            .as_deref()
            .ok_or_else(|| ApiError::BadInput("permission has no owner".into()))?;
        let id = card.payload["action_id"]
            .as_str()
            .ok_or_else(|| ApiError::BadInput("permission action missing".into()))?;
        let action = crate::actions::get(&self.db, &self.project_id, id)?;
        if action.agent_id != aid
            || action.tool != card.payload["tool"]
            || action.input != card.payload["raw_input"]
            || action.question_id.as_deref() != Some(question_id)
        {
            return Err(ApiError::BadInput("permission action mismatch".into()));
        }
        let mut ctx = self.ctx_for(aid, action.stage_run_id.clone());
        let stopped = orchestra::is_paused(&self.db, &self.project_id)?
            || self.db.conn().query_row(
                "SELECT status='sleeping' FROM agents WHERE project_id=?1 AND id=?2",
                rusqlite::params![self.project_id, aid],
                |row| row.get::<_, bool>(0),
            )?;
        if stopped {
            crate::actions::expose_ready(&self.db, &ctx, id)?;
            if matches!(action.state.as_str(), "succeeded" | "failed" | "denied") {
                if let Some(resume) = self.permission_activation(&card)? {
                    self.queue_activation_resume(
                        aid,
                        action.stage_run_id.as_deref(),
                        &resume,
                        "task_paused_or_sleeping",
                    )?;
                }
            }
            return Err(
                crate::tools::ToolError::NotExecuted("project or instance stopped".into()).into(),
            );
        }
        // False refusal leaves explicit recovery; guessing the original task
        // could run an obsolete effect (Q2/Q7, test3/04).
        let Some(resume) = self.permission_activation(&card)? else {
            if action.state != "authorized" {
                return Ok("original_task_already_settled");
            }
            crate::actions::fail_authorized(
                &self.db,
                &ctx,
                id,
                "original task superseded or missing",
            )?;
            return Ok("original_task_superseded_or_missing");
        };
        ctx.saved_permission_admission = Some(crate::tools::SavedPermissionAdmission {
            root_turn_id: resume.root_turn_id,
            slots: self.permission_slots.clone(),
        });
        if action.state == "authorized" {
            if let Err(error) = self.permission_resume_admission(&card) {
                crate::actions::expose_ready(&self.db, &ctx, id)?;
                return Err(error);
            }
        }
        let effect = if action.state == "authorized" {
            self.registry
                .resume_action(&self.db, &ctx, id)
                .map(|_| ())
                .map_err(ApiError::from)
        } else {
            Ok(())
        };
        if crate::actions::get(&self.db, &self.project_id, id)?.state == "authorized" {
            crate::actions::expose_ready(&self.db, &ctx, id)?;
            return effect.map(|_| "action_not_started");
        }
        // Ordinary failed/denied results are feedback. Unknown effects keep the
        // existing recovery boundary and stop before another model request.
        let continuation = self.continue_permission_card(card);
        effect
            .and(continuation)
            .map(|_| "recorded_original_task_result")
    }
}

impl PermissionControl {
    pub fn save_project_permission(
        &self,
        db: &Db,
        question_id: &str,
    ) -> Result<Vec<String>, ApiError> {
        let proposal = permission_shape_suggestion(db, &self.project_id, question_id)?
            .ok_or_else(|| ApiError::BadInput("project permission unavailable".into()))?;
        self.save(
            db,
            question_id,
            true,
            Some(&proposal.shape),
            "project_shared",
        )?;
        let rule_id: String = db.conn().query_row(
            "SELECT json_extract(payload,'$.rule_id') FROM events WHERE project_id=?1
             AND kind='permission_shape_remembered' AND json_extract(payload,'$.question_id')=?2
             ORDER BY id DESC LIMIT 1",
            rusqlite::params![self.project_id, question_id],
            |row| row.get(0),
        )?;
        let mut saved = vec![question_id.to_string()];
        for id in
            crate::cards::queued_ids(db, &self.project_id, crate::cards::CardKind::Permission)?
        {
            let card = crate::cards::get(db, &id)?;
            let Some(aid) = card.agent_id.as_deref() else {
                continue;
            };
            let Some(action_id) = card.payload["action_id"].as_str() else {
                continue;
            };
            let action = crate::actions::get(db, &self.project_id, action_id)?;
            let ctx = ToolContext::for_agent(db, &self.root, aid);
            if !crate::permissions::matches_project_permission(
                db,
                &ctx,
                &action.tool,
                &action.input,
                &rule_id,
            )? {
                continue;
            }
            // ADR0079/test3/04: each peer gets current model/scope admission;
            // one refusal does not revoke the durable grant for independent work.
            if self
                .save_with_origin(
                    db,
                    &id,
                    true,
                    None,
                    "activation",
                    &format!("project_permission:{rule_id}"),
                )
                .is_ok()
            {
                saved.push(id);
            }
        }
        Ok(saved)
    }

    /// MR-06: a live action-review allow after a re-judge. Same actor and
    /// no shape memory as the in-turn reviewer allow (ticket 05 rule).
    pub(super) fn save_by_reviewer(&self, db: &Db, question_id: &str) -> Result<(), ApiError> {
        self.save_with_origin(db, question_id, true, None, "activation", "reviewer")
    }

    /// Test3/17: a managed decision under the current delegation grant. Never
    /// remembered as a shape (one decision, not a standing rule); the actor
    /// is the grant (`managed:<id>:<generation>`), never `owner`.
    pub(super) fn save_by_managed(
        &self,
        db: &Db,
        question_id: &str,
        allow: bool,
        actor: &str,
    ) -> Result<(), ApiError> {
        self.save_with_origin(db, question_id, allow, None, "activation", actor)
    }

    pub fn save(
        &self,
        db: &Db,
        question_id: &str,
        allow: bool,
        remember_shape: Option<&str>,
        scope: &str,
    ) -> Result<(), ApiError> {
        self.save_with_origin(db, question_id, allow, remember_shape, scope, "owner")
    }

    #[allow(clippy::too_many_arguments)]
    fn save_with_origin(
        &self,
        db: &Db,
        question_id: &str,
        allow: bool,
        remember_shape: Option<&str>,
        scope: &str,
        origin: &str,
    ) -> Result<(), ApiError> {
        let started = std::time::Instant::now();
        let result =
            self.save_inner_with_origin(db, question_id, allow, remember_shape, scope, origin);
        let card = crate::cards::get(db, question_id).ok();
        let reason = result
            .as_ref()
            .err()
            .map(crate::errcode::ErrorCode::code)
            .unwrap_or_else(|| "decision_saved".into());
        crate::diag::note(
            if result.is_err() {
                crate::diag::CLASS_REJECT
            } else {
                crate::diag::CLASS_JUDGE
            },
            result.is_err(),
            Some(&self.project_id),
            card.as_ref().and_then(|card| card.agent_id.as_deref()),
            None,
            Some(question_id),
            "permission_control",
            &reason,
            started,
        );
        result
    }

    #[allow(clippy::too_many_arguments)]
    fn save_inner_with_origin(
        &self,
        db: &Db,
        question_id: &str,
        allow: bool,
        remember_shape: Option<&str>,
        scope: &str,
        origin: &str,
    ) -> Result<(), ApiError> {
        let root: String = db.conn().query_row(
            "SELECT dir FROM projects WHERE id=?1",
            [&self.project_id],
            |row| row.get(0),
        )?;
        if Path::new(&root).canonicalize()? != self.root.canonicalize()? {
            return Err(ApiError::BadInput("permission project mismatch".into()));
        }
        let card = crate::cards::get_queued(db, question_id, crate::cards::CardKind::Permission)?;
        if card.project_id != self.project_id {
            return Err(ApiError::BadInput("permission project mismatch".into()));
        }
        let aid = card
            .agent_id
            .as_deref()
            .ok_or_else(|| ApiError::BadInput("permission has no owner".into()))?;
        let slot: Option<String> = db.conn().query_row(
            "SELECT model_slot FROM agents WHERE project_id=?1 AND id=?2",
            rusqlite::params![self.project_id, aid],
            |row| row.get(0),
        )?;
        if allow
            && crate::provider_config::resolve_slot(
                &self.slots.read().unwrap(),
                slot.as_deref().unwrap_or("default"),
            )
            .is_none()
        {
            return Err(ApiError::NoProvider(slot.unwrap_or_default()));
        }
        let mut ctx = ToolContext::for_agent(db, &self.root, aid);
        ctx.project_id = self.project_id.clone();
        if allow {
            ctx.caps = crate::provider_config::caps_for_slot(slot.as_deref().unwrap_or("default"));
        }
        let pack = (allow
            && remember_shape.is_some()
            && self.root.join(".hexagon/pack.active.json").is_file())
        .then(|| PackDef::pinned(&self.root))
        .transpose()?;
        self.registry.decide_permission(
            db,
            &ctx,
            question_id,
            allow,
            remember_shape,
            scope,
            pack.as_ref(),
            origin,
            true,
        )?;
        Ok(())
    }
}

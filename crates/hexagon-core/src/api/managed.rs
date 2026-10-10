//! Test3/17–23 (spec 托管委托的附加合同; d-batch-design.md): the managed
//! decision processor. For every pending card the host lists the closed set
//! of actions; a model only picks within it; anything else — no model, an
//! error, an ambiguous or out-of-set answer, an exhausted budget, an
//! unrouted card type — ends in that type's fixed automatic terminal action,
//! never back in needs-human and never an implicit "yes".
//!
//! Three steps so model reasoning holds no workbench lock (the shell drives
//! them with short locks): `managed_plan` reads candidates, `managed_decide`
//! asks the model on its own ledger connection, `managed_commit` re-verifies
//! the grant generation and card state in the committing step and acts
//! through the card owner's facade with the real managed actor.
use super::*;
use serde::Serialize;

/// One card the processor will decide. Data only: safe to carry across the
/// unlocked model step.
#[derive(Debug, Clone, Serialize)]
pub struct ManagedCandidate {
    pub card_id: String,
    pub kind: String,
    pub decision_key: String,
    pub actor: String,
    /// The role whose card this is; the request ledger books the decision's
    /// cost to it (the ledger's agent column is a foreign key).
    pub agent_id: String,
    /// The closed set: (action token, short meaning).
    pub options: Vec<(String, String)>,
    /// Plain-language state the model decides on (no secrets).
    pub state: String,
    /// The action taken without a model, and why (host-decided).
    pub forced: Option<(String, String)>,
    /// The action used when the model cannot decide.
    pub fallback: String,
}

/// Model access and project identity carried across the unlocked step.
#[derive(Clone)]
pub struct ManagedRoute {
    pub providers: HashMap<String, Arc<dyn ModelProvider>>,
    pub root: PathBuf,
    pub project_id: String,
}

/// What was decided and through which path.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ManagedDecision {
    pub action: String,
    pub reason: String,
    pub via: String,
}

/// One processed managed decision as the owner sees it (D06 DTO). Codes, not
/// prose: the UI renders them through i18n.
#[derive(Debug, Clone, Serialize, ts_rs::TS)]
#[ts(export, export_to = "../../../ui/src/gen/")]
pub struct ManagedRecord {
    #[ts(type = "number")]
    pub event_id: i64,
    pub card_id: String,
    pub card_kind: String,
    pub action: String,
    pub reason: String,
    pub via: String,
    pub outcome: String,
    pub actor: String,
}

/// Newest first, at most `limit` records.
pub fn managed_history(
    db: &Db,
    project_id: &str,
    limit: usize,
) -> Result<Vec<ManagedRecord>, ApiError> {
    let text = |v: &serde_json::Value| v.as_str().unwrap_or_default().to_string();
    let mut records: Vec<ManagedRecord> = db
        .events(project_id, Some(&[EventKind::System]))?
        .into_iter()
        .filter(|event| event.payload["kind"] == "managed_decision")
        .map(|event| ManagedRecord {
            event_id: event.id,
            card_id: text(&event.payload["card_id"]),
            card_kind: text(&event.payload["card_kind"]),
            action: text(&event.payload["action"]),
            reason: text(&event.payload["reason"]),
            via: text(&event.payload["via"]),
            outcome: text(&event.payload["outcome"]),
            actor: text(&event.payload["actor"]),
        })
        .collect();
    records.reverse();
    records.truncate(limit);
    Ok(records)
}

/// Q19–Q21 (spec 托管外部边界): entries where the host cannot prove an
/// action will not delete an external file, directory, project or cloud
/// document. Under delegation they are refused and reported as a capability
/// gap; a model label is never proof. Cost model: a false refusal leaves one
/// task incomplete with a visible reason; a false allow is an unreviewed
/// external deletion. Entries with host-provable contracts (project tools,
/// host_fs_read/host_fs_write, which have no delete operation) are not here.
pub(crate) fn unprovable_external(tool: &str) -> Option<&'static str> {
    match tool {
        "host_bash" => Some("host_shell_cannot_exclude_external_deletion"),
        "browser_action" => Some("browser_page_action_cannot_exclude_deletion"),
        "computer_action" | "computer_navigate" => Some("native_input_cannot_exclude_deletion"),
        _ if tool.starts_with("mcp:") => Some("mcp_semantics_cannot_exclude_deletion"),
        _ => None,
    }
}

/// Card kinds the processor takes over while delegation is on. The owner's
/// pending view hides exactly these, so a taken-over card never counts as
/// waiting for a human; turning delegation off returns them unchanged.
pub(crate) fn takes_over(kind: &str) -> bool {
    kind == "permission"
}

/// The owner's pending read: while delegation is on, cards the processor
/// owns are shown in the delegation panel as processing, not as owner cards.
pub fn owner_pending(db: &Db, project_id: &str) -> Result<Vec<crate::cards::QueuedCard>, ApiError> {
    let cards = crate::cards::queued(db, project_id)?;
    if crate::delegation::current(db, project_id).is_none() {
        return Ok(cards);
    }
    Ok(cards
        .into_iter()
        .filter(|card| !takes_over(&card.kind))
        .collect())
}

/// Cards currently held by the processor (empty while delegation is off).
pub fn managed_processing(
    db: &Db,
    project_id: &str,
) -> Result<Vec<crate::cards::QueuedCard>, ApiError> {
    if crate::delegation::current(db, project_id).is_none() {
        return Ok(vec![]);
    }
    Ok(crate::cards::queued(db, project_id)?
        .into_iter()
        .filter(|card| takes_over(&card.kind))
        .collect())
}

/// Cost model for the closed-set parser: an unrecognised answer becomes the
/// fallback (a refusal); accepting a near-miss could run an action nobody
/// chose. Exact token match only.
pub(crate) fn parse_choice(text: &str, options: &[(String, String)]) -> Option<String> {
    let first = text
        .trim()
        .lines()
        .next()
        .unwrap_or_default()
        .trim()
        .trim_matches(['"', '`', '.']);
    options
        .iter()
        .find(|(token, _)| token == first)
        .map(|(token, _)| token.clone())
}

impl Workbench {
    /// Candidates for every pending card while delegation is on.
    pub fn managed_plan(&self) -> Result<Vec<ManagedCandidate>, ApiError> {
        let Some(grant) = crate::delegation::current(&self.db, &self.project_id) else {
            return Ok(vec![]);
        };
        // The owner's pause stops new automatic decisions; delegation never
        // lifts it. Unreadable pause state counts as paused.
        if orchestra::is_paused(&self.db, &self.project_id).unwrap_or(true) {
            return Ok(vec![]);
        }
        let mut out = vec![];
        for card in crate::cards::queued(&self.db, &self.project_id)? {
            if let Some(candidate) = self.candidate_for(&card, &grant)? {
                out.push(candidate);
            }
        }
        Ok(out)
    }

    fn candidate_for(
        &self,
        card: &crate::cards::QueuedCard,
        grant: &crate::delegation::Grant,
    ) -> Result<Option<ManagedCandidate>, ApiError> {
        if !takes_over(&card.kind) {
            // Tickets 18–23 route the remaining kinds through this processor.
            return Ok(None);
        }
        let p = &card.payload;
        let tool = p["tool"].as_str().unwrap_or_default().to_string();
        let key = format!("permission:{}", p["action_id"].as_str().unwrap_or(&card.id));
        let mut forced = None;
        if crate::delegation::rounds(&self.db, &self.project_id, &key)
            >= crate::delegation::MAX_ROUNDS
        {
            forced = Some(("deny".to_string(), "budget_exhausted".to_string()));
        } else if let Some(gap) = unprovable_external(&tool) {
            forced = Some(("deny".to_string(), gap.to_string()));
        }
        let input = serde_json::to_string(&p["raw_input"]).unwrap_or_default();
        let state = format!(
            "You act for the project owner under project delegation. Decide one pending permission.\n\
             Tool: {tool}\nInput: {}\nWhy it needs approval: {}\nSafety-net action: {}\n\
             Allow it only if it is needed for the project task and does not delete files outside the project, \
             manage accounts, pay, or publish to a new target. Answer with exactly one token: allow or deny.",
            input.chars().take(2000).collect::<String>(),
            p["reason"].as_str().unwrap_or_default(),
            p["safety_net"] == true,
        );
        Ok(Some(ManagedCandidate {
            card_id: card.id.clone(),
            kind: card.kind.clone(),
            decision_key: key,
            actor: grant.actor(),
            agent_id: card.agent_id.clone().unwrap_or_default(),
            options: vec![
                ("allow".into(), "run the action as requested".into()),
                ("deny".into(), "refuse the action".into()),
            ],
            state,
            forced,
            fallback: "deny".into(),
        }))
    }

    /// Everything the unlocked decision step needs, plus the project identity
    /// the commit step re-checks (a project switch between steps never lands).
    pub fn managed_route(&self) -> ManagedRoute {
        ManagedRoute {
            providers: self.providers.clone(),
            root: self.repo_root.clone(),
            project_id: self.project_id.clone(),
        }
    }

    /// Commit only into the project the decision was prepared for.
    pub fn managed_commit_for(
        &self,
        route: &ManagedRoute,
        candidate: &ManagedCandidate,
        decision: ManagedDecision,
    ) -> Result<Option<ManagedDecision>, ApiError> {
        if route.root != self.repo_root || route.project_id != self.project_id {
            return Ok(None);
        }
        self.managed_commit(candidate, decision)
    }

    /// Plan, decide and commit every candidate (tests and simple drivers).
    pub fn managed_tick(&self) -> Result<Vec<ManagedDecision>, ApiError> {
        let providers = self.providers.clone();
        let mut done = vec![];
        for candidate in self.managed_plan()? {
            let decision = managed_decide_on(
                &self.db,
                &providers,
                &self.repo_root,
                &self.project_id,
                &candidate,
            );
            if let Some(decision) = self.managed_commit(&candidate, decision)? {
                done.push(decision);
            }
        }
        Ok(done)
    }

    /// Re-verify and act. Returns None when the grant or card moved on (a late
    /// decision never lands).
    pub fn managed_commit(
        &self,
        candidate: &ManagedCandidate,
        decision: ManagedDecision,
    ) -> Result<Option<ManagedDecision>, ApiError> {
        let started = std::time::Instant::now();
        let fresh = !orchestra::is_paused(&self.db, &self.project_id).unwrap_or(true)
            && crate::delegation::actor_is_current(&self.db, &self.project_id, &candidate.actor)
            && crate::cards::get(&self.db, &candidate.card_id)
                .is_ok_and(|card| card.state == crate::cards::CardState::Queued);
        if !fresh {
            crate::diag::note(
                crate::diag::CLASS_REJECT,
                true,
                Some(&self.project_id),
                None,
                None,
                Some(&candidate.card_id),
                "managed_decision",
                "stale_grant_or_card",
                started,
            );
            return Ok(None);
        }
        crate::delegation::spend_round(&self.db, &self.project_id, &candidate.decision_key)?;
        let result = match candidate.kind.as_str() {
            "permission" => self.managed_permission(candidate, &decision),
            _ => Ok("unrouted".to_string()),
        };
        let outcome = match &result {
            Ok(outcome) => outcome.clone(),
            Err(error) => format!("failed: {}", crate::errcode::ErrorCode::code(error)),
        };
        let card = crate::cards::get(&self.db, &candidate.card_id)?;
        self.db.append_event(
            &self.project_id,
            EventKind::System,
            json!({"kind":"managed_decision","card_id":candidate.card_id,"card_kind":candidate.kind,
                "action":decision.action,"reason":decision.reason,"via":decision.via,
                "actor":candidate.actor,"outcome":outcome}),
            card.agent_id.as_deref(),
            None,
        )?;
        crate::diag::note(
            crate::diag::CLASS_JUDGE,
            false,
            Some(&self.project_id),
            card.agent_id.as_deref(),
            None,
            Some(&candidate.card_id),
            "managed_decision",
            &format!("{}:{}", candidate.kind, decision.action),
            started,
        );
        Ok(Some(decision))
    }

    fn managed_permission(
        &self,
        candidate: &ManagedCandidate,
        decision: &ManagedDecision,
    ) -> Result<String, ApiError> {
        let allow = decision.action == "allow";
        // The saved-decision path executes the action in the background and
        // continues the original task (ticket 04); the actor is the grant.
        self.permission_control().save_by_managed(
            &self.db,
            &candidate.card_id,
            allow,
            &candidate.actor,
        )?;
        if allow {
            self.continue_saved_permission(&candidate.card_id)?;
            Ok("allowed_and_continued".into())
        } else {
            Ok("denied".into())
        }
    }
}

/// The unlocked model step for the shell: its own ledger connection, so the
/// workbench lock is not held while the model reasons.
pub fn managed_decide(
    providers: &HashMap<String, Arc<dyn ModelProvider>>,
    root: &Path,
    project_id: &str,
    candidate: &ManagedCandidate,
) -> ManagedDecision {
    if candidate.forced.is_none() {
        let Ok(db) = Db::open(root.join(".hexagon/state.db")) else {
            return route_failed(candidate, "ledger_unavailable");
        };
        return managed_decide_on(&db, providers, root, project_id, candidate);
    }
    managed_decide_on_forced(candidate)
}

fn route_failed(candidate: &ManagedCandidate, code: &str) -> ManagedDecision {
    ManagedDecision {
        action: candidate.fallback.clone(),
        reason: format!("managed_route_failed:{code}"),
        via: "fallback".into(),
    }
}

fn managed_decide_on_forced(candidate: &ManagedCandidate) -> ManagedDecision {
    let (action, reason) = candidate.forced.clone().unwrap_or_default();
    ManagedDecision {
        action,
        reason,
        via: "host".into(),
    }
}

/// Closed-set choice: the decision slot (Jev, decision API only) first, then
/// the project's chat slot. Every send keeps its ledger row; failures and
/// out-of-set answers end in the card type's fallback, never in "yes".
pub(crate) fn managed_decide_on(
    db: &Db,
    providers: &HashMap<String, Arc<dyn ModelProvider>>,
    root: &Path,
    project_id: &str,
    candidate: &ManagedCandidate,
) -> ManagedDecision {
    if candidate.forced.is_some() {
        return managed_decide_on_forced(candidate);
    }
    let route = crate::provider_config::resolve_exact(providers, crate::provider_config::JEV_SLOT)
        .filter(|p| p.uses_decision_api())
        .map(|p| (p.clone(), crate::provider_config::JEV_SLOT.to_string()))
        .or_else(|| {
            crate::provider_config::resolve_slot(providers, "default")
                .map(|p| (p.clone(), "default".to_string()))
        });
    let Some((provider, slot)) = route else {
        return route_failed(candidate, "no_model");
    };
    let ctx = crate::tools::ToolContext {
        project_id: project_id.into(),
        agent_id: candidate.agent_id.clone(),
        repo_root: root.to_path_buf(),
        ..Default::default()
    };
    let options: Vec<(&str, &str)> = candidate
        .options
        .iter()
        .map(|(a, b)| (a.as_str(), b.as_str()))
        .collect();
    let request = crate::provider::ChatRequest {
        model_slot: slot.clone(),
        messages: vec![crate::provider::Message {
            role: crate::provider::Role::User,
            content: vec![crate::provider::ContentBlock::Text {
                text: candidate.state.clone(),
            }],
        }],
        tools: vec![],
    };
    let response = crate::model_failure::bounded_call(db, &ctx, || {
        let chat = (!provider.uses_decision_api()).then_some(&request);
        crate::usage::request(
            db,
            &ctx,
            &slot,
            "managed_decision",
            provider.as_ref(),
            chat,
            || {
                if provider.uses_decision_api() {
                    provider.decide(&candidate.state, &options)
                } else {
                    provider.complete(&request)
                }
            },
        )
    });
    match response {
        Ok(resp) => match parse_choice(&crate::pm_route::choice_text(&resp), &candidate.options) {
            Some(action) => ManagedDecision {
                action,
                reason: "model_choice".into(),
                via: "model".into(),
            },
            None => route_failed(candidate, "out_of_set"),
        },
        Err(error) => route_failed(candidate, crate::model_failure::failure_code(&error)),
    }
}

#[cfg(test)]
#[path = "managed_tests.rs"]
mod managed_tests;

#[cfg(test)]
mod tests {
    use super::*;
    use proptest::prelude::*;

    fn opts() -> Vec<(String, String)> {
        vec![("allow".into(), "a".into()), ("deny".into(), "d".into())]
    }

    #[test]
    fn only_exact_tokens_are_choices() {
        assert_eq!(parse_choice("allow", &opts()), Some("allow".into()));
        assert_eq!(
            parse_choice(" deny.\nbecause", &opts()),
            Some("deny".into())
        );
        for bad in ["Allow", "allow it", "yes", "", "allow/deny"] {
            assert_eq!(parse_choice(bad, &opts()), None, "{bad}");
        }
    }

    #[test]
    fn unprovable_entries_are_named_and_project_tools_are_not() {
        for tool in [
            "host_bash",
            "browser_action",
            "computer_action",
            "computer_navigate",
            "mcp:figma:delete_node",
        ] {
            assert!(unprovable_external(tool).is_some(), "{tool}");
        }
        for tool in [
            "bash",
            "fs_write",
            "host_fs_write",
            "host_fs_read",
            "browser_observe",
            "dependency_fetch",
        ] {
            assert!(unprovable_external(tool).is_none(), "{tool}");
        }
    }

    proptest! {
        // Any third-party MCP tool, whatever it claims to be, is refused.
        #[test]
        fn every_mcp_tool_is_unprovable(server in "[a-z]{1,8}", tool in "\\PC{0,20}") {
            let name = format!("mcp:{server}:{tool}");
            prop_assert!(unprovable_external(&name).is_some());
        }

        // Whatever the model says, the parsed action is inside the closed set.
        #[test]
        fn any_answer_maps_into_the_closed_set_or_nothing(text in "\\PC{0,40}") {
            if let Some(action) = parse_choice(&text, &opts()) {
                prop_assert!(opts().iter().any(|(token, _)| *token == action));
            }
        }
    }
}

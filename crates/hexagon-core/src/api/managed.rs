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

mod act;
mod external;
mod report;
mod routes;
pub use external::{managed_external_ops, ExternalOperation};
pub use report::{
    classify as classify_managed, export_managed_report, managed_summary, ManagedEvidence,
    ManagedScope, ManagedSummary,
};
// Glob re-export keeps each item's own visibility (pub for the IPC reads).
pub use routes::*;

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
    /// done | incomplete | failed — computed here from one closed table so
    /// the UI never guesses from outcome strings (test3/23 review).
    pub result: String,
    /// Gap A5: closed classification from (action, reason, via) — see `report::classify`.
    pub scope: ManagedScope,
    pub evidence: ManagedEvidence,
}

/// Outcomes where the decision was made but the business item did not
/// complete. Everything else that did not fail is a landed effect.
const INCOMPLETE_OUTCOMES: &[&str] = &[
    "incomplete",
    "still_incomplete",
    "checks_ran_still_incomplete",
    "kept_checks_required",
    "ended_unexecuted",
    "not_published",
    "new_attempt_awaits_permission",
    "ended",
];

/// An empty pending list is never business success: only a landed effect is
/// "done" (spec ticket 23).
pub(crate) fn result_of(action: &str, outcome: &str) -> &'static str {
    if outcome.starts_with("failed") {
        "failed"
    } else if action == "incomplete" || INCOMPLETE_OUTCOMES.contains(&outcome) {
        "incomplete"
    } else {
        "done"
    }
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
        .map(|event| {
            let (scope, evidence) = report::classify(
                event.payload["action"].as_str().unwrap_or_default(),
                event.payload["reason"].as_str().unwrap_or_default(),
                event.payload["via"].as_str().unwrap_or_default(),
            );
            ManagedRecord {
                event_id: event.id,
                card_id: text(&event.payload["card_id"]),
                card_kind: text(&event.payload["card_kind"]),
                action: text(&event.payload["action"]),
                reason: text(&event.payload["reason"]),
                via: text(&event.payload["via"]),
                outcome: text(&event.payload["outcome"]),
                actor: text(&event.payload["actor"]),
                result: result_of(
                    event.payload["action"].as_str().unwrap_or_default(),
                    event.payload["outcome"].as_str().unwrap_or_default(),
                )
                .into(),
                scope,
                evidence,
            }
        })
        .collect();
    records.reverse();
    records.truncate(limit);
    Ok(records)
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
        if !takes_over(card) {
            return Ok(None);
        }
        let Some(key) = decision_key(card) else {
            return Ok(None);
        };
        // One unreadable card (e.g. its action row is gone) must not stall
        // every other decision: it becomes its own incomplete item.
        let route = match self.route_for(card) {
            Ok(Some(route)) => route,
            Ok(None) => return Ok(None),
            Err(_) => Route {
                options: opts(&[("incomplete", "end the unreadable item")]),
                state: String::new(),
                forced: Some(("incomplete", "route_unavailable")),
                fallback: "incomplete",
            },
        };
        let exhausted = crate::delegation::rounds(&self.db, &self.project_id, &key)
            >= crate::delegation::MAX_ROUNDS;
        let forced = if exhausted {
            Some((route.fallback.to_string(), "budget_exhausted".to_string()))
        } else {
            route.forced.map(|(a, r)| (a.to_string(), r.to_string()))
        };
        Ok(Some(ManagedCandidate {
            card_id: card.id.clone(),
            kind: card.kind.clone(),
            decision_key: key,
            actor: grant.actor(),
            agent_id: card.agent_id.clone().unwrap_or_default(),
            options: route.options,
            state: route.state,
            forced,
            fallback: route.fallback.into(),
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
        &mut self,
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
    pub fn managed_tick(&mut self) -> Result<Vec<ManagedDecision>, ApiError> {
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
        &mut self,
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
        // Every owner facade below records the grant actor, never `owner`.
        let result = {
            let _acting = self.acting_as(&candidate.actor);
            self.managed_act(candidate, &decision)
        };
        let mut outcome = match &result {
            Ok(outcome) => outcome.clone(),
            Err(error) => format!("failed: {}", crate::errcode::ErrorCode::code(error)),
        };
        // The forced fallback itself failed after the budget: end the item as
        // incomplete instead of leaving it for an owner (no needs-human).
        if result.is_err()
            && crate::delegation::rounds(&self.db, &self.project_id, &candidate.decision_key)
                > crate::delegation::MAX_ROUNDS
        {
            let _acting = self.acting_as(&candidate.actor);
            let card = crate::cards::get(&self.db, &candidate.card_id)?;
            if card.state == crate::cards::CardState::Queued {
                self.end_card_incomplete(&card)?;
                outcome.push_str("; ended_incomplete");
            }
        }
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
        // Refusals, fallbacks and failures are Warn; landed choices Debug.
        let refused = result_of(&decision.action, &outcome) != "done"
            || decision.via == "fallback"
            || matches!(
                decision.action.as_str(),
                "deny" | "reject" | "abandon" | "refuse" | "end"
            );
        crate::diag::note(
            if refused {
                crate::diag::CLASS_REJECT
            } else {
                crate::diag::CLASS_JUDGE
            },
            refused,
            Some(&self.project_id),
            card.agent_id.as_deref(),
            None,
            Some(&candidate.card_id),
            "managed_decision",
            &format!("{}:{}:{}", candidate.kind, decision.action, decision.reason),
            started,
        );
        Ok(Some(decision))
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

/// A delegated decision that fell to the type's fallback: Warn, code only.
fn note_route(
    project_id: &str,
    candidate: &ManagedCandidate,
    code: &str,
    started: std::time::Instant,
) {
    crate::diag::note(
        crate::diag::CLASS_SLOT,
        true,
        Some(project_id),
        Some(&candidate.agent_id),
        None,
        Some(&candidate.card_id),
        "managed_route",
        code,
        started,
    );
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
    let started = std::time::Instant::now();
    let Some((provider, slot)) = route else {
        note_route(project_id, candidate, "no_model", started);
        return route_failed(candidate, "no_model");
    };
    if slot != crate::provider_config::JEV_SLOT {
        // Normal slot fallback (no decision model configured): Debug.
        crate::diag::note(
            crate::diag::CLASS_SLOT,
            false,
            Some(project_id),
            Some(&candidate.agent_id),
            None,
            Some(&candidate.card_id),
            "managed_route",
            "chat_slot_without_decision_model",
            started,
        );
    }
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
            None => {
                note_route(project_id, candidate, "out_of_set", started);
                route_failed(candidate, "out_of_set")
            }
        },
        Err(error) => {
            let code = crate::model_failure::failure_code(&error);
            note_route(project_id, candidate, code, started);
            route_failed(candidate, code)
        }
    }
}

#[cfg(test)]
#[path = "managed_tests.rs"]
mod managed_tests;

#[cfg(test)]
#[path = "managed_acceptance_tests.rs"]
mod managed_acceptance_tests;

#[cfg(test)]
#[path = "managed_proposal_tests.rs"]
mod managed_proposal_tests;

#[cfg(test)]
#[path = "managed_install_tests.rs"]
mod managed_install_tests;

#[cfg(test)]
#[path = "managed_publish_tests.rs"]
mod managed_publish_tests;

#[cfg(test)]
#[path = "managed_external_tests.rs"]
mod managed_external_tests;

#[cfg(test)]
#[path = "managed_golden_tests.rs"]
mod managed_golden_tests;

#[cfg(test)]
#[path = "managed_budget_tests.rs"]
mod managed_budget_tests;

#[cfg(test)]
#[path = "managed_capability_tests.rs"]
mod managed_capability_tests;

#[cfg(test)]
#[path = "managed_report_tests.rs"]
mod managed_report_tests;

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
    fn only_landed_effects_count_as_done() {
        assert_eq!(result_of("allow", "allowed_and_continued"), "done");
        assert_eq!(result_of("deny", "denied"), "done");
        assert_eq!(
            result_of("run_checks", "checks_ran_still_incomplete"),
            "incomplete"
        );
        assert_eq!(
            result_of("new_attempt", "new_attempt_awaits_permission"),
            "incomplete"
        );
        assert_eq!(result_of("incomplete", "incomplete"), "incomplete");
        assert_eq!(result_of("publish", "failed: git"), "failed");
    }

    #[test]
    fn only_the_unfenced_host_shell_stays_unprovable() {
        // Ticket 01 (ADR0087) changed this: MCP, browser page actions and
        // native input used to be force-refused here; they now take the
        // delegated permission decision and are reported as unverified.
        for tool in [
            "browser_action",
            "computer_action",
            "computer_navigate",
            "mcp:figma:delete_node",
            "bash",
            "fs_write",
            "host_fs_write",
            "host_fs_read",
            "host_fs_trash",
            "browser_observe",
            "dependency_fetch",
        ] {
            assert!(unprovable_external(tool).is_none(), "{tool}");
        }
        assert_eq!(
            unprovable_external("host_bash").is_some(),
            !crate::sandbox::fence_available()
        );
    }

    fn check_state() -> impl Strategy<Value = orchestra::CheckState> {
        prop_oneof![
            Just(orchestra::CheckState::Missing),
            Just(orchestra::CheckState::Passed),
            Just(orchestra::CheckState::Failed),
            Just(orchestra::CheckState::Stale),
            Just(orchestra::CheckState::Unavailable),
        ]
    }

    fn quality() -> impl Strategy<Value = Option<crate::proposals::PolicyQualityState>> {
        use crate::proposals::PolicyQualityState as Q;
        proptest::option::of(prop_oneof![
            Just(Q::Unverified),
            Just(Q::Incomplete),
            Just(Q::Failed),
            Just(Q::Stale),
            Just(Q::Qualified),
            Just(Q::Historical),
        ])
    }

    proptest! {
        // Test3/22: publish iff every host fact holds.
        #[test]
        fn publish_needs_every_host_fact(bits in proptest::collection::vec(any::<bool>(), 5)) {
            let facts = PublishFacts {
                target_configured: bits[0],
                credential_present: bits[1],
                delivery_accepted: bits[2],
                baseline_unchanged: bits[3],
                commit_unchanged: bits[4],
            };
            prop_assert_eq!(publish_forced(&facts).0 == "publish", bits.iter().all(|b| *b));
        }

        // Test3/21: any non-empty secret slot (env or headers) means the host
        // cannot complete the install by itself.
        #[test]
        fn any_secret_slot_needs_the_owner_login(key in "[A-Z_]{1,12}", headers in any::<bool>()) {
            let field = if headers { "headers" } else { "env" };
            let plan = serde_json::json!({"plan_kind":"config","spec":{field:{key:""}}});
            prop_assert!(install_needs_credentials(&plan));
            let plain = serde_json::json!({"plan_kind":"skill-dir","creds":false});
            prop_assert!(!install_needs_credentials(&plain));
        }

        // Test3/20: a policy candidate is adopted only with qualified
        // independent quality; never left to model advice.
        #[test]
        fn policy_adoption_needs_qualified_independent_quality(q in quality(), review_missing in any::<bool>()) {
            let facts = ProposalFacts { policy_quality: q, role_review_missing: review_missing };
            let forced = proposal_forced(&facts);
            if q.is_some() {
                prop_assert!(forced.is_some(), "policy is always host-decided");
            }
            if matches!(forced, Some(("adopt", _))) {
                prop_assert_eq!(q, Some(crate::proposals::PolicyQualityState::Qualified));
            }
            if q.is_none() && review_missing {
                prop_assert_eq!(forced.map(|f| f.0), Some("incomplete"));
            }
        }

        // Test3/19: acceptance never lowers the bar — "accept" iff nothing is
        // missing; a failed check is never "re-run until lucky", it is rework.
        #[test]
        fn delegated_acceptance_takes_only_complete_evidence(
            states in proptest::collection::vec(check_state(), 0..5),
            other_missing in any::<bool>(),
        ) {
            let checks: Vec<orchestra::CheckEvidence> = states.iter().map(|state| orchestra::CheckEvidence {
                run_id: "r".into(), stage: "s".into(), cmd: "c".into(), event_id: None,
                exit_code: None, state: state.clone(), quality: None,
            }).collect();
            let mut missing: Vec<String> = states.iter()
                .filter(|s| **s != orchestra::CheckState::Passed)
                .map(|_| "check".to_string()).collect();
            if other_missing { missing.push("review".into()); }
            let evidence = orchestra::StageEvidence {
                run_id: "r".into(), fingerprint: None, checks, exceptions: vec![], missing: missing.clone(),
            };
            let (action, _) = stamp_forced(Some(&evidence));
            prop_assert_eq!(action == "accept", missing.is_empty());
            if states.contains(&orchestra::CheckState::Failed) {
                prop_assert_eq!(action, "rework");
            }
        }

        // Test3/18: a fresh new attempt is only ever the model's choice for an
        // unresolved, non-linked, provable action; the host itself only
        // continues an existing attempt, never starts a second one.
        #[test]
        fn the_host_never_starts_a_second_new_attempt(
            linked in any::<bool>(),
            resolution in proptest::option::of(prop_oneof![Just("new_attempt"), Just("abandoned")]),
            gap in any::<bool>(),
        ) {
            let facts = UnknownFacts {
                linked_attempt: linked,
                resolution: resolution.map(str::to_string),
                external_gap: gap.then_some("host_shell_cannot_exclude_external_deletion"),
            };
            match unknown_forced(&facts) {
                Some(("new_attempt", _)) => prop_assert_eq!(facts.resolution.as_deref(), Some("new_attempt")),
                Some(_) => prop_assert!(facts.resolution.is_some() || linked || gap),
                None => prop_assert!(facts.resolution.is_none() && !linked && !gap),
            }
        }

        // Ticket 01 (ADR0087) rewrote "every MCP tool is refused": a
        // delegated MCP permission is now the model's closed-set choice, and
        // its prompt always carries the host's fixed constraints.
        #[test]
        fn mcp_permissions_are_decided_within_the_constraints(server in "[a-z]{1,8}", tool in "[a-z_]{1,20}") {
            let route = permission_route(&serde_json::json!({"tool": format!("mcp:{server}:{tool}")}));
            prop_assert!(route.forced.is_none());
            prop_assert_eq!(route.fallback, "deny");
            prop_assert!(route.state.contains(DELEGATED_CONSTRAINTS));
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

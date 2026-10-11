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

/// The stable business key a card's decisions are budgeted under, if the
/// processor takes this card over. Same key across new card ids, restarts and
/// off/on, so three rounds stay three rounds (spec: 同一业务决定最多三轮).
pub(crate) fn decision_key(card: &crate::cards::QueuedCard) -> Option<String> {
    let p = &card.payload;
    let action = || p["action_id"].as_str().map(str::to_string);
    match (card.kind.as_str(), p["sub"].as_str()) {
        ("permission", _) => Some(format!(
            "permission:{}",
            action().unwrap_or_else(|| card.id.clone())
        )),
        ("recovery", Some("tool_outcome_unknown")) => action().map(|a| format!("unknown:{a}")),
        ("recovery", Some("tool_action_ready")) => action().map(|a| format!("ready:{a}")),
        // An in-flight retry is the processor's own work, not a new decision.
        ("recovery", Some("model_request")) if p["state"] != "retrying" => {
            Some(format!("model_request:{}", card.id))
        }
        ("stall", _) if p["source"] == "activation_resume" => Some(format!(
            "resume:{}:{}",
            p["activation_root_turn_id"], p["trigger_turn_id"]
        )),
        ("stall", _) => Some(format!("stall:{}", card.id)),
        // Proposal confirmations share kind=stamp (cards.rs overload note).
        ("stamp", _) if !p["proposal_id"].is_null() => {
            p["proposal_id"].as_str().map(|id| format!("proposal:{id}"))
        }
        // One budget per stage across reworked runs and new cards.
        ("stamp", None) => p["stage"].as_str().map(|s| format!("stage:{s}")),
        ("stamp", Some("quality_revalidation")) => {
            p["run_id"].as_str().map(|r| format!("revalidation:{r}"))
        }
        ("stamp", Some("acceptance_exception")) => Some(format!("exception:{}", card.id)),
        ("escalation", Some("design_direction")) => Some(format!("design:{}", p["revision"])),
        ("escalation", Some("context_overflow")) => {
            Some(format!("context:{}", p["trigger_turn_id"]))
        }
        // Back-and-forth bounces on one artifact share one budget.
        ("escalation", _) => Some(format!("flag:{}", p["target"].as_str().unwrap_or(&card.id))),
        _ => None,
    }
}

/// Taken over = routed and still inside its budget plus the one forced
/// fallback round. A card whose fallback also failed returns to the owner:
/// a host defect must stay visible rather than be hidden forever.
pub(crate) fn takes_over(db: &Db, project_id: &str, card: &crate::cards::QueuedCard) -> bool {
    decision_key(card).is_some_and(|key| {
        crate::delegation::rounds(db, project_id, &key) <= crate::delegation::MAX_ROUNDS
    })
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
        .filter(|card| !takes_over(db, project_id, card))
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
        .filter(|card| takes_over(db, project_id, card))
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

/// One card type's closed set (token, meaning), host-decided action and
/// fixed automatic terminal action.
struct Route {
    options: Vec<(String, String)>,
    state: String,
    forced: Option<(&'static str, &'static str)>,
    fallback: &'static str,
}

fn opts(pairs: &[(&str, &str)]) -> Vec<(String, String)> {
    pairs
        .iter()
        .map(|(a, b)| (a.to_string(), b.to_string()))
        .collect()
}

fn permission_route(p: &serde_json::Value) -> Route {
    let tool = p["tool"].as_str().unwrap_or_default();
    let input = serde_json::to_string(&p["raw_input"]).unwrap_or_default();
    Route {
        options: opts(&[("allow", "run the action as requested"), ("deny", "refuse the action")]),
        state: format!(
            "You act for the project owner under project delegation. Decide one pending permission.\n\
             Tool: {tool}\nInput: {}\nWhy it needs approval: {}\nSafety-net action: {}\n\
             Allow it only if it is needed for the project task and does not delete files outside the project, \
             manage accounts, pay, or publish to a new target. Answer with exactly one token: allow or deny.",
            input.chars().take(2000).collect::<String>(),
            p["reason"].as_str().unwrap_or_default(),
            p["safety_net"] == true,
        ),
        forced: unprovable_external(tool).map(|gap| ("deny", gap)),
        fallback: "deny",
    }
}

/// Facts that decide what the processor may do with an unknown action.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct UnknownFacts {
    /// This action is itself the one linked new attempt of another.
    pub linked_attempt: bool,
    /// An existing resolution of this action, if any.
    pub resolution: Option<String>,
    /// The tool cannot prove it will not delete external content.
    pub external_gap: Option<&'static str>,
}

/// Spec 托管 Q14/Q18: abandon or at most one linked new attempt; the old
/// unknown stays. Cost model: a false "abandon" leaves the task to continue
/// without that effect (visible); a false "new attempt" may repeat a real
/// external effect, so a second attempt and unprovable entries are refused.
pub(crate) fn unknown_forced(facts: &UnknownFacts) -> Option<(&'static str, &'static str)> {
    match facts.resolution.as_deref() {
        // Continue the one existing attempt idempotently, never a second.
        Some("new_attempt") => return Some(("new_attempt", "linked_attempt_in_flight")),
        Some(_) => return Some(("abandon", "already_abandoned")),
        None => {}
    }
    if facts.linked_attempt {
        return Some(("abandon", "new_attempt_already_used"));
    }
    facts.external_gap.map(|gap| ("abandon", gap))
}

fn unknown_route(tool: &str, facts: &UnknownFacts) -> Route {
    Route {
        options: opts(&[
            ("abandon", "accept the unknown outcome and continue without it"),
            ("new_attempt", "run the action once more, accepting a possible duplicate"),
        ]),
        state: format!(
            "You act for the project owner under project delegation. A tool action's outcome is unknown \
             (it may or may not have happened). Tool: {tool}.\n\
             Choose new_attempt only if repeating it is harmless or clearly needed; otherwise abandon. \
             Answer with exactly one token: abandon or new_attempt."
        ),
        forced: unknown_forced(facts),
        fallback: "abandon",
    }
}

fn stall_route(p: &serde_json::Value) -> Route {
    let retry = p["retry"] == true;
    Route {
        options: opts(&[
            ("retry", "wake the role or project manager once more"),
            ("end", "end this stall"),
        ]),
        state: format!(
            "You act for the project owner under project delegation. The project stalled ({}). \
             Choose retry to try once more, or end. Answer with exactly one token: retry or end.",
            p["branch"].as_str().unwrap_or("unknown")
        ),
        forced: (!retry).then_some(("end", "retry_not_offered")),
        fallback: "end",
    }
}

/// Test3/19 (spec Q15/Q18): the host's acceptance rule. Never lowers the
/// bar — only complete current evidence is accepted; runnable gaps are run;
/// failures and other gaps go back for rework. Cost model: a false rework
/// costs one more round (bounded by the stage budget); a false accept ships
/// an unchecked delivery.
pub(crate) fn stamp_forced(
    evidence: Option<&orchestra::StageEvidence>,
) -> (&'static str, &'static str) {
    let Some(evidence) = evidence else {
        return ("incomplete", "no_active_stage");
    };
    if evidence.missing.is_empty() {
        return ("accept", "evidence_complete");
    }
    if evidence
        .checks
        .iter()
        .any(|c| c.state == orchestra::CheckState::Failed)
    {
        return ("rework", "checks_failed");
    }
    let runnable = !evidence.checks.is_empty()
        && evidence
            .checks
            .iter()
            .all(|c| c.state != orchestra::CheckState::Unavailable);
    if runnable
        && evidence.checks.iter().any(|c| {
            matches!(
                c.state,
                orchestra::CheckState::Missing | orchestra::CheckState::Stale
            )
        })
    {
        return ("run_checks", "checks_missing_or_stale");
    }
    ("rework", "requirements_missing")
}

/// Facts a delegated proposal decision may not override.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ProposalFacts {
    /// Independent quality state of a policy candidate (None = not a policy).
    pub policy_quality: Option<crate::proposals::PolicyQualityState>,
    pub role_review_missing: bool,
}

/// Test3/20 (Q15/Q18): a model's advice never completes a policy decision.
/// A policy candidate is adopted only with qualified independent quality and
/// otherwise ends incomplete; a role definition without its superior review
/// ends incomplete. Cost model: a false "incomplete" costs one regenerated
/// proposal; a false adopt changes the process without independent evidence.
pub(crate) fn proposal_forced(facts: &ProposalFacts) -> Option<(&'static str, &'static str)> {
    use crate::proposals::PolicyQualityState as Q;
    match facts.policy_quality {
        Some(Q::Qualified) => return Some(("adopt", "independent_quality_qualified")),
        Some(Q::Failed) => return Some(("reject", "independent_quality_failed")),
        Some(_) => return Some(("incomplete", "independent_quality_missing")),
        None => {}
    }
    facts
        .role_review_missing
        .then_some(("incomplete", "superior_review_missing"))
}

fn design_route(direction: &crate::design::DesignDirection) -> Route {
    let listed = direction
        .options
        .iter()
        .map(|o| {
            format!(
                "- {}: {} — {} (layout: {}; type: {}; palette: {})",
                o.id, o.title, o.description, o.layout, o.typography, o.palette
            )
        })
        .collect::<Vec<_>>()
        .join("\n");
    Route {
        options: direction
            .options
            .iter()
            .map(|o| (o.id.clone(), o.title.clone()))
            .collect(),
        state: format!(
            "You act for the project owner under project delegation. Choose the visual direction that best fits \
             the project's stated goals.\n{listed}\nAnswer with exactly one option id."
        ),
        forced: direction
            .options
            .is_empty()
            .then_some(("incomplete", "no_versioned_options")),
        fallback: "incomplete",
    }
}

fn flag_route(p: &serde_json::Value) -> Route {
    Route {
        options: opts(&[
            ("agree", "uphold the review: rework the flagged artifact"),
            ("reject", "overrule the review: continue as is"),
        ]),
        state: format!(
            "You act for the project owner under project delegation. A reviewer flagged an artifact.\n\
             Target: {}\nReason: {}\nUphold (agree) unless the flag is clearly wrong. \
             Answer with exactly one token: agree or reject.",
            p["target"].as_str().unwrap_or_default(),
            p["reason"].as_str().unwrap_or_default(),
        ),
        forced: None,
        // A failed decision keeps the reviewer's standard, never overrules it.
        fallback: "agree",
    }
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
        if !takes_over(&self.db, &self.project_id, card) {
            return Ok(None);
        }
        let Some(key) = decision_key(card) else {
            return Ok(None);
        };
        let Some(route) = self.route_for(card)? else {
            return Ok(None);
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

    /// The closed set, host-decided action and fallback for one card.
    fn route_for(&self, card: &crate::cards::QueuedCard) -> Result<Option<Route>, ApiError> {
        let p = &card.payload;
        Ok(Some(match (card.kind.as_str(), p["sub"].as_str()) {
            ("permission", _) => permission_route(p),
            ("recovery", Some("tool_outcome_unknown")) => {
                let action_id = p["action_id"].as_str().unwrap_or_default();
                let action = crate::actions::get(&self.db, &self.project_id, action_id)?;
                let facts = UnknownFacts {
                    linked_attempt: crate::actions::linked_origin(&self.db, action_id)?.is_some(),
                    resolution: crate::actions::resolution(&self.db, action_id)?.map(|r| r.0),
                    external_gap: unprovable_external(&action.tool),
                };
                unknown_route(&action.tool, &facts)
            }
            ("recovery", Some("tool_action_ready")) => {
                let action_id = p["action_id"].as_str().unwrap_or_default();
                let action = crate::actions::get(&self.db, &self.project_id, action_id)?;
                Route {
                    options: opts(&[
                        ("resume", "run the saved, provably unsent action"),
                        ("end", "leave it unexecuted"),
                    ]),
                    state: String::new(),
                    forced: Some(match unprovable_external(&action.tool) {
                        Some(gap) => ("end", gap),
                        None => ("resume", "host_proves_not_executed"),
                    }),
                    fallback: "end",
                }
            }
            ("recovery", Some("model_request")) => Route {
                options: opts(&[
                    ("retry", "send the failed request again"),
                    ("end", "stop as incomplete"),
                ]),
                state: String::new(),
                forced: Some(match p["state"].as_str() {
                    // The owner's newer instruction is their current intent.
                    Some("needs_choice") => ("follow_new", "owner_newer_instruction"),
                    _ if p["can_retry"] == false => ("end", "retry_not_available"),
                    _ => ("retry", "bounded_model_retry"),
                }),
                fallback: "end",
            },
            ("stall", _) if p["source"] == "activation_resume" => Route {
                options: opts(&[
                    ("continue", "continue the original task"),
                    ("end", "end it"),
                ]),
                state: String::new(),
                forced: Some(("continue", "continue_original_task")),
                fallback: "end",
            },
            ("stall", _) => stall_route(p),
            ("stamp", _) if !p["proposal_id"].is_null() => self.proposal_route(p)?,
            ("stamp", None) => {
                let evidence = orchestra::stage_evidence(&self.db, &self.project_id, self.pack()?)?;
                Route {
                    options: opts(&[
                        ("accept", "accept the stage with complete current evidence"),
                        (
                            "run_checks",
                            "run the approved checks for the current version",
                        ),
                        ("rework", "send the stage back for rework"),
                        ("incomplete", "stop: the stage cannot meet its requirements"),
                    ]),
                    state: String::new(),
                    forced: Some(stamp_forced(evidence.as_ref())),
                    fallback: "incomplete",
                }
            }
            ("stamp", Some("quality_revalidation")) => {
                let evidence = orchestra::stage_evidence(&self.db, &self.project_id, self.pack()?)?;
                let forced = match stamp_forced(evidence.as_ref()) {
                    ("accept", why) => ("confirm", why),
                    ("run_checks", why) => ("run_checks", why),
                    (_, why) => ("cancel", why),
                };
                Route {
                    options: opts(&[
                        ("confirm", "accept current evidence"),
                        ("cancel", "keep the new checks required"),
                    ]),
                    state: String::new(),
                    forced: Some(forced),
                    fallback: "cancel",
                }
            }
            // Acceptance never lowers the bar: exception requests are refused.
            ("stamp", Some("acceptance_exception")) => Route {
                options: opts(&[("refuse", "refuse the exception request")]),
                state: String::new(),
                forced: Some(("refuse", "acceptance_never_lowered")),
                fallback: "refuse",
            },
            ("escalation", Some("design_direction")) => {
                let direction = crate::design::read(&self.db, &self.project_id)?;
                design_route(&direction)
            }
            ("escalation", Some("context_overflow")) => Route {
                options: opts(&[
                    ("resume", "continue the original task"),
                    ("end", "end the task"),
                ]),
                state: String::new(),
                forced: Some(("resume", "continue_original_task")),
                fallback: "end",
            },
            ("escalation", _) => flag_route(p),
            _ => return Ok(None),
        }))
    }

    fn proposal_route(&self, p: &serde_json::Value) -> Result<Route, ApiError> {
        let pid = p["proposal_id"].as_str().unwrap_or_default();
        let (surface, status): (String, String) = self.db.conn().query_row(
            "SELECT surface,status FROM proposals WHERE id=?1 AND project_id=?2",
            rusqlite::params![pid, self.project_id],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )?;
        let facts = ProposalFacts {
            policy_quality: crate::proposals::quality_for(
                &self.db,
                &self.project_id,
                pid,
                &surface,
                &status,
            ),
            role_review_missing: surface == "role_def"
                && !crate::rolesurf::reviewed(&self.db, pid)?,
        };
        Ok(Route {
            options: opts(&[("adopt", "apply the proposal"), ("reject", "do not apply it")]),
            state: format!(
                "You act for the project owner under project delegation. An agent proposes an improvement.\n\
                 Surface: {surface}\nExecution judgment (advice only): {}\nWarnings: {}\nEvidence: {}\n\
                 Adopt only if the evidence shows it improves the project without widening permissions or \
                 weakening safeguards. Answer with exactly one token: adopt or reject.",
                p["judge_verdict"].as_str().unwrap_or("none"),
                p["warnings"],
                serde_json::to_string(&p["evidence"]).unwrap_or_default().chars().take(2000).collect::<String>(),
            ),
            forced: proposal_forced(&facts),
            fallback: "reject",
        })
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

    /// Act through the card owner's facade. Each branch re-validates its own
    /// target (action state, permission, activation chain) at execution.
    fn managed_act(
        &mut self,
        candidate: &ManagedCandidate,
        decision: &ManagedDecision,
    ) -> Result<String, ApiError> {
        let card = crate::cards::get(&self.db, &candidate.card_id)?;
        let p = &card.payload;
        let reason = format!("managed: {}", decision.reason);
        let action_id = || {
            p["action_id"]
                .as_str()
                .map(str::to_string)
                .ok_or_else(|| ApiError::BadInput("recovery card without an action".into()))
        };
        let outcome = match (
            card.kind.as_str(),
            p["sub"].as_str(),
            decision.action.as_str(),
        ) {
            ("permission", _, _) => return self.managed_permission(candidate, decision),
            ("recovery", Some("tool_outcome_unknown"), "new_attempt") => {
                match self.retry_tool_action(&action_id()?, &reason, true)? {
                    crate::tools::CallOutcome::Asked(_) => "new_attempt_awaits_permission",
                    _ => "new_attempt_dispatched",
                }
            }
            ("recovery", Some("tool_outcome_unknown"), _) => {
                self.abandon_tool_action(&action_id()?, &reason)?;
                "abandoned"
            }
            ("recovery", Some("tool_action_ready"), "resume") => {
                self.resume_tool_action(&action_id()?)?;
                "resumed"
            }
            ("recovery", Some("tool_action_ready"), _) => {
                let id = action_id()?;
                let action = crate::actions::get(&self.db, &self.project_id, &id)?;
                let ctx = self.ctx_for(&action.agent_id, action.stage_run_id.clone());
                crate::actions::fail_authorized(&self.db, &ctx, &id, &reason)?;
                "ended_unexecuted"
            }
            ("recovery", Some("model_request"), "retry") => {
                self.retry_model_request(&card.id)?;
                "retried"
            }
            ("recovery", Some("model_request"), "follow_new") => {
                self.resolve_model_request_choice(&card.id, true)?;
                "followed_newer_instruction"
            }
            ("recovery", Some("model_request"), _) => {
                self.retire_model_request_incomplete(&card)?;
                "incomplete"
            }
            ("stamp", _, "adopt") if !p["proposal_id"].is_null() => {
                self.managed_adopt(&card.id)?;
                "adopted"
            }
            ("stamp", _, "reject") if !p["proposal_id"].is_null() => {
                let ctx = self.ctx_for("owner", None);
                crate::proposals::reject_at_stamp_by(
                    &self.db,
                    &ctx,
                    &card.id,
                    &reason,
                    &self.decider(),
                )?;
                "rejected"
            }
            ("stamp", _, _) if !p["proposal_id"].is_null() => {
                self.end_card_incomplete(&card)?;
                "incomplete"
            }
            ("stamp", None, action) => return self.managed_stage(&card, action, &reason),
            ("stamp", Some("quality_revalidation"), action) => {
                return self.managed_revalidation(&card, action)
            }
            ("stamp", Some("acceptance_exception"), _) => {
                orchestra::cancel_acceptance_exception_by(
                    &self.db,
                    &self.project_id,
                    &card.id,
                    &self.decider(),
                )?;
                "exception_refused"
            }
            ("escalation", Some("design_direction"), "incomplete") => {
                self.end_card_incomplete(&card)?;
                "incomplete"
            }
            ("escalation", Some("design_direction"), option) => {
                let revision = p["revision"].as_i64().unwrap_or_default();
                crate::design::choose_by(
                    &self.db,
                    &self.project_id,
                    &card.id,
                    revision,
                    Some(option),
                    None,
                    &self.decider(),
                )?;
                self.continue_design_direction(revision)?;
                "direction_chosen_and_continued"
            }
            ("escalation", Some("context_overflow"), action) => {
                self.adjudicate_flag(&card.id, action == "resume")?;
                if action == "resume" {
                    "resumed"
                } else {
                    "ended"
                }
            }
            ("escalation", _, "agree" | "reject") => {
                self.adjudicate_flag(&card.id, decision.action == "agree")?;
                if decision.action == "agree" {
                    "rework_requested"
                } else {
                    "flag_overruled"
                }
            }
            ("escalation", _, _) => {
                self.end_card_incomplete(&card)?;
                "incomplete"
            }
            ("stall", _, "continue" | "retry") => {
                self.stall_retry(&card.id)?;
                "continued"
            }
            ("stall", _, _) => {
                self.stall_ack(&card.id)?;
                "ended"
            }
            _ => "unrouted",
        };
        Ok(outcome.into())
    }

    fn managed_stage(
        &self,
        card: &crate::cards::Card,
        action: &str,
        reason: &str,
    ) -> Result<String, ApiError> {
        let by = self.decider();
        let pack = self.pack()?;
        let stage_outcome = |result: orchestra::StageAction| match result {
            orchestra::StageAction::Incomplete { .. } => "still_incomplete",
            _ => "accepted",
        };
        Ok(match action {
            "accept" => stage_outcome(orchestra::stamp_by(&self.db, &self.project_id, pack, &by)?),
            "run_checks" => {
                orchestra::run_checks(&self.db, &self.project_id, &self.repo_root, pack)?;
                match stage_outcome(orchestra::stamp_by(&self.db, &self.project_id, pack, &by)?) {
                    "accepted" => "checks_ran_and_accepted",
                    _ => "checks_ran_still_incomplete",
                }
            }
            "rework" => {
                let evidence = orchestra::stage_evidence(&self.db, &self.project_id, pack)?;
                if card.payload["final_acceptance"] == true {
                    let stage = evidence
                        .as_ref()
                        .and_then(|e| {
                            e.checks
                                .iter()
                                .find(|c| c.state == orchestra::CheckState::Failed)
                                .map(|c| c.stage.clone())
                        })
                        .or_else(|| card.payload["stage"].as_str().map(str::to_string))
                        .unwrap_or_default();
                    let note = format!(
                        "{reason}; unmet: {}",
                        evidence.map(|e| e.missing.join(", ")).unwrap_or_default()
                    );
                    crate::review::reject_final_by(
                        &self.db,
                        &self.project_id,
                        pack,
                        &stage,
                        &note,
                        &by,
                    )?;
                } else {
                    crate::review::reject_stamp_by(&self.db, &self.project_id, pack, &by)?;
                }
                "rework_requested"
            }
            _ => {
                self.end_card_incomplete(card)?;
                "incomplete"
            }
        }
        .into())
    }

    fn managed_revalidation(
        &self,
        card: &crate::cards::Card,
        action: &str,
    ) -> Result<String, ApiError> {
        let by = self.decider();
        let pack = self.pack()?;
        let confirm = |db: &Db| -> Result<&'static str, ApiError> {
            let fingerprint = orchestra::stage_evidence(db, &self.project_id, pack)?
                .and_then(|e| e.fingerprint)
                .unwrap_or_default();
            Ok(
                match orchestra::confirm_quality_revalidation_by(
                    db,
                    &self.project_id,
                    pack,
                    &card.id,
                    &fingerprint,
                    &by,
                )? {
                    orchestra::StageAction::PackFinished => "revalidated",
                    _ => "still_incomplete",
                },
            )
        };
        Ok(match action {
            "confirm" => confirm(&self.db)?,
            "run_checks" => {
                orchestra::run_checks(&self.db, &self.project_id, &self.repo_root, pack)?;
                confirm(&self.db)?
            }
            _ => {
                orchestra::cancel_quality_revalidation_by(
                    &self.db,
                    &self.project_id,
                    &card.id,
                    &by,
                )?;
                "kept_checks_required"
            }
        }
        .into())
    }

    /// Same controlled adoption as the owner's, recorded under the grant.
    fn managed_adopt(&mut self, qid: &str) -> Result<(), ApiError> {
        crate::proposals::recover_policy(&self.db, &self.repo_root, &self.project_id)?;
        self.refresh_policy()?;
        let by = self.decider();
        crate::proposals::activate_by(&self.db, &self.ctx_for("owner", None), qid, &by)?;
        self.refresh_policy()?;
        Ok(())
    }

    /// The type's terminal "incomplete": the card is answered by the grant
    /// actor and marked, the target stays unaccepted. Visible in the panel.
    fn end_card_incomplete(&self, card: &crate::cards::Card) -> Result<(), ApiError> {
        crate::cards::annotate(
            &self.db,
            &card.id,
            &[("managed_outcome", json!("incomplete"))],
        )?;
        crate::cards::answer(&self.db, &card.id, &self.decider())?;
        Ok(())
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
#[path = "managed_acceptance_tests.rs"]
mod managed_acceptance_tests;

#[cfg(test)]
#[path = "managed_proposal_tests.rs"]
mod managed_proposal_tests;

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

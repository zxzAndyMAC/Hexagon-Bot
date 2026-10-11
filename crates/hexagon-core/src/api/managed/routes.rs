//! Test3/17–23: the closed-set routing table — every card type's stable
//! business key, its closed set, the host-decided action and its fixed
//! terminal fallback, plus the host judges (each with a proptest in the
//! parent's tests). Split from managed.rs (review: file over the 800-line cap).
use super::*;

/// How much tool input / evidence a delegated decision prompt carries.
pub(super) const PROMPT_EXCERPT_CHARS: usize = 2000;
/// How much of a free-text field may shape a budget key.
pub(super) const KEY_TEXT_CHARS: usize = 120;

/// Q19–Q21 (spec 托管外部边界): entries where the host cannot prove an
/// action will not delete an external file, directory, project or cloud
/// document. Under delegation they are refused and reported as a capability
/// gap; a model label is never proof. Cost model: a false refusal leaves one
/// task incomplete with a visible reason; a false allow is an unreviewed
/// external deletion. Entries with host-provable contracts (project tools,
/// host_fs_read/host_fs_write, which have no delete operation) are not here.
pub(crate) fn unprovable_external(tool: &str) -> Option<&'static str> {
    match tool {
        // Test3/23: the host shell runs under the delegated deletion fence
        // where seatbelt can enforce it; elsewhere it cannot be proven.
        "host_bash" if !crate::sandbox::fence_available() => {
            Some("host_shell_cannot_exclude_external_deletion")
        }
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
        // Interrupted stage run (crash or network suspension).
        ("recovery", None) if p["run_id"].is_string() => {
            Some(format!("run:{}", p["run_id"].as_str().unwrap_or_default()))
        }
        ("recovery", Some("model_request")) => Some(format!("model_request:{}", card.id)),
        ("stall", _) if p["source"] == "activation_resume" => Some(format!(
            "resume:{}:{}",
            p["activation_root_turn_id"], p["trigger_turn_id"]
        )),
        // A stall's business identity is the role and the stalled task, so a
        // new card for the same stall shares the budget (review: no reset).
        ("stall", _) => Some(format!(
            "stall:{}:{}:{}",
            card.agent_id.as_deref().unwrap_or_default(),
            p["branch"].as_str().unwrap_or_default(),
            p["instruction"]
                .as_str()
                .unwrap_or_default()
                .chars()
                .take(KEY_TEXT_CHARS)
                .collect::<String>()
        )),
        // Proposal confirmations share kind=stamp (cards.rs overload note).
        ("stamp", _) if !p["proposal_id"].is_null() => {
            p["proposal_id"].as_str().map(|id| format!("proposal:{id}"))
        }
        // One budget per stage across reworked runs and new cards.
        ("stamp", None) => p["stage"].as_str().map(|s| format!("stage:{s}")),
        ("stamp", Some("quality_revalidation")) => {
            p["run_id"].as_str().map(|r| format!("revalidation:{r}"))
        }
        ("stamp", Some("acceptance_exception")) => Some(format!(
            "exception:{}:{}",
            p["run_id"].as_str().unwrap_or(&card.id),
            p["fingerprint"].as_str().unwrap_or_default()
        )),
        ("escalation", Some("design_direction")) => Some(format!("design:{}", p["revision"])),
        ("escalation", Some("context_overflow")) => {
            Some(format!("context:{}", p["trigger_turn_id"]))
        }
        ("publish", _) => Some(format!(
            "publish:{}:{}",
            p["remote"].as_str().unwrap_or_default(),
            p["commit"].as_str().unwrap_or(&card.id)
        )),
        ("install", _) => Some(format!(
            "install:{}:{}",
            p["plan_kind"].as_str().unwrap_or_default(),
            p["name"].as_str().unwrap_or(&card.id)
        )),
        ("grant", _) => Some(format!(
            "grant:{}:{}:{}",
            p["agent_id"].as_str().unwrap_or_default(),
            p["grant_kind"].as_str().unwrap_or_default(),
            p["name"].as_str().unwrap_or_default()
        )),
        // Back-and-forth bounces on one artifact share one budget.
        ("escalation", _) => Some(format!("flag:{}", p["target"].as_str().unwrap_or(&card.id))),
        // Test3/23: an unregistered kind/sub is still taken over — it ends
        // incomplete and is reported, never approved by default.
        _ => Some(format!("unrouted:{}", card.id)),
    }
}

/// Every card with a business key is taken over while delegation is on —
/// never handed back as needs-human (spec: 不回到needs-human). An earlier
/// draft returned a card whose fallback also failed to the owner; now such a
/// card is ended as incomplete by `managed_commit` and reported instead.
pub(crate) fn takes_over(card: &crate::cards::QueuedCard) -> bool {
    decision_key(card).is_some()
}

/// Delegation holds cards only while on and not paused. The owner's pause
/// stops automatic decisions, so paused cards return to the owner's view
/// rather than sit under "processing" with nothing processing them.
pub(super) fn delegation_holds(db: &Db, project_id: &str) -> bool {
    crate::delegation::current(db, project_id).is_some()
        && !orchestra::is_paused(db, project_id).unwrap_or(true)
}

/// The owner's pending read: while delegation is on, cards the processor
/// owns are shown in the delegation panel as processing, not as owner cards.
pub fn owner_pending(db: &Db, project_id: &str) -> Result<Vec<crate::cards::QueuedCard>, ApiError> {
    let cards = crate::cards::queued(db, project_id)?;
    if !delegation_holds(db, project_id) {
        return Ok(cards);
    }
    Ok(cards.into_iter().filter(|card| !takes_over(card)).collect())
}

/// Cards currently held by the processor (empty while delegation is off).
pub fn managed_processing(
    db: &Db,
    project_id: &str,
) -> Result<Vec<crate::cards::QueuedCard>, ApiError> {
    if !delegation_holds(db, project_id) {
        return Ok(vec![]);
    }
    Ok(crate::cards::queued(db, project_id)?
        .into_iter()
        .filter(takes_over)
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
pub(super) struct Route {
    pub(super) options: Vec<(String, String)>,
    pub(super) state: String,
    pub(super) forced: Option<(&'static str, &'static str)>,
    pub(super) fallback: &'static str,
}

pub(super) fn opts(pairs: &[(&str, &str)]) -> Vec<(String, String)> {
    pairs
        .iter()
        .map(|(a, b)| (a.to_string(), b.to_string()))
        .collect()
}

pub(super) fn permission_route(p: &serde_json::Value) -> Route {
    let tool = p["tool"].as_str().unwrap_or_default();
    let input = serde_json::to_string(&p["raw_input"]).unwrap_or_default();
    Route {
        options: opts(&[("allow", "run the action as requested"), ("deny", "refuse the action")]),
        state: format!(
            "You act for the project owner under project delegation. Decide one pending permission.\n\
             Tool: {tool}\nInput: {}\nWhy it needs approval: {}\nSafety-net action: {}\n\
             Allow it only if it is needed for the project task and does not delete files outside the project, \
             manage accounts, pay, or publish to a new target. Answer with exactly one token: allow or deny.",
            input.chars().take(PROMPT_EXCERPT_CHARS).collect::<String>(),
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

pub(super) fn unknown_route(tool: &str, facts: &UnknownFacts) -> Route {
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

pub(super) fn stall_route(p: &serde_json::Value) -> Route {
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

/// Test3/21: an install the host cannot complete without the owner's login
/// or secrets ends incomplete (spec: 缺登录/凭据自动未完成), never queued.
pub(crate) fn install_needs_credentials(plan: &serde_json::Value) -> bool {
    plan["creds"] == true
        || plan["spec"]["env"]
            .as_object()
            .is_some_and(|env| !env.is_empty())
        || plan["spec"]["headers"]
            .as_object()
            .is_some_and(|headers| !headers.is_empty())
}

pub(super) fn install_route(p: &serde_json::Value) -> Route {
    Route {
        options: opts(&[("install", "install into this project"), ("reject", "do not install")]),
        state: format!(
            "You act for the project owner under project delegation. A role asks to install a capability into \
             this project only.\nKind: {}\nName: {}\nWhy: {}\nNetwork: {}\n\
             Install only if the current task needs it. Answer with exactly one token: install or reject.",
            p["plan_kind"].as_str().unwrap_or_default(),
            p["name"].as_str().unwrap_or_default(),
            p["desc"].as_str().unwrap_or_default(),
            p["net"] == true,
        ),
        forced: install_needs_credentials(p).then_some(("incomplete", "credentials_or_login_required")),
        fallback: "reject",
    }
}

/// Installed in this project (skills dir) or configured and spawnable (MCP).
/// A grant never stands in for availability.
pub(super) fn capability_present(root: &Path, kind: &str, name: &str) -> bool {
    match kind {
        "skill" => root.join(".hexagon/skills").join(name).is_dir(),
        "mcp" => crate::mcp::load_specs(root)
            .iter()
            .any(|spec| spec.name == name),
        _ => false,
    }
}

pub(super) fn grant_route(p: &serde_json::Value, present: bool) -> Route {
    Route {
        options: opts(&[("allow", "let this role use it in this project"), ("deny", "do not grant")]),
        state: format!(
            "You act for the project owner under project delegation. A role asks to use a capability in this \
             project.\nRole: {}\nKind: {}\nName: {}\n\
             Grant only if the role's current task needs it. Answer with exactly one token: allow or deny.",
            p["agent_id"].as_str().unwrap_or_default(),
            p["grant_kind"].as_str().unwrap_or_default(),
            p["name"].as_str().unwrap_or_default(),
        ),
        forced: (!present).then_some(("incomplete", "capability_not_installed")),
        fallback: "deny",
    }
}

/// Host-verified facts for a delegated publish (no secret values).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub(crate) struct PublishFacts {
    pub target_configured: bool,
    pub credential_present: bool,
    pub delivery_accepted: bool,
    pub baseline_unchanged: bool,
    pub commit_unchanged: bool,
}

/// Test3/22 (Q13/Q20): publish only to an existing project target with its
/// project credential, after final acceptance, for the exact version that was
/// requested. Never a new target, a personal login, or an accepted-by-model
/// shortcut. Cost model: a false refusal leaves an unpublished but accepted
/// delivery (visible, retryable by the owner); a false publish is an
/// irreversible external effect.
pub(crate) fn publish_forced(facts: &PublishFacts) -> (&'static str, &'static str) {
    let checks = [
        (facts.target_configured, "target_missing"),
        (facts.credential_present, "credential_missing"),
        (facts.delivery_accepted, "delivery_not_accepted"),
        (facts.baseline_unchanged, "target_drifted"),
        (facts.commit_unchanged, "delivery_changed"),
    ];
    match checks.iter().find(|(ok, _)| !ok) {
        Some((_, reason)) => ("incomplete", reason),
        None => ("publish", "configured_target_accepted_delivery"),
    }
}

pub(super) fn design_route(direction: &crate::design::DesignDirection) -> Route {
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

pub(super) fn flag_route(p: &serde_json::Value) -> Route {
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
        // Q15: delegation keeps the review standard. A model may not overrule a
        // reviewer (review finding: that let a model lower the bar); bounces
        // are bounded by the artifact's budget and then end incomplete.
        forced: Some(("agree", "review_standard_kept")),
        fallback: "incomplete",
    }
}

impl Workbench {
    /// The closed set, host-decided action and fallback for one card.
    pub(super) fn route_for(
        &self,
        card: &crate::cards::QueuedCard,
    ) -> Result<Option<Route>, ApiError> {
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
            ("recovery", None) if p["run_id"].is_string() => Route {
                options: opts(&[
                    ("resume_run", "resume the interrupted stage and its task"),
                    ("end", "leave it interrupted"),
                ]),
                state: String::new(),
                forced: Some(("resume_run", "interrupted_run_recoverable")),
                fallback: "end",
            },
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
            // An in-flight retry is already being worked; hold, don't decide.
            ("recovery", Some("model_request")) if p["state"] == "retrying" => return Ok(None),
            ("recovery", Some("model_request")) => Route {
                options: opts(&[
                    ("retry", "send the failed request again"),
                    ("end", "stop as incomplete"),
                ]),
                state: String::new(),
                forced: Some(match p["state"].as_str() {
                    // The owner's newer instruction is their current intent.
                    Some("needs_choice") => ("follow_new", "owner_newer_instruction"),
                    // The owner stopped this item's retries: delegation does
                    // not revive it (ticket 06: no revival by autonomy).
                    _ if p["stop_requested"] == true => ("end", "owner_stopped_retries"),
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
            ("publish", _) => {
                let facts = self.publish_facts(p)?;
                Route {
                    options: opts(&[
                        ("publish", "push to the configured target"),
                        ("incomplete", "do not publish"),
                    ]),
                    state: String::new(),
                    forced: Some(publish_forced(&facts)),
                    fallback: "incomplete",
                }
            }
            ("install", _) => install_route(p),
            ("grant", _) => {
                let present = capability_present(
                    &self.repo_root,
                    p["grant_kind"].as_str().unwrap_or_default(),
                    p["name"].as_str().unwrap_or_default(),
                );
                grant_route(p, present)
            }
            _ => Route {
                options: opts(&[("end", "end the unregistered item")]),
                state: String::new(),
                forced: Some(("end", "unrouted_card_type")),
                fallback: "end",
            },
        }))
    }

    fn publish_facts(&self, p: &serde_json::Value) -> Result<PublishFacts, ApiError> {
        let remote = p["remote"].as_str().unwrap_or_default();
        let git =
            |args: &[&str]| crate::git::run(&self.repo_root, args).map(|o| o.trim().to_string());
        let latest: Option<(i64, String)> = self
            .db
            .conn()
            .query_row(
                "SELECT seq,state FROM stage_runs WHERE project_id=?1 ORDER BY rowid DESC LIMIT 1",
                [&self.project_id],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .ok();
        let final_seq = self.pack.as_ref().map(|pack| pack.stages.len() as i64 - 1);
        Ok(PublishFacts {
            target_configured: !remote.is_empty() && git(&["remote", "get-url", remote]).is_ok(),
            credential_present: self
                .creds
                .get(&format!("publish/{remote}"))
                .is_ok_and(|value| value.is_some()),
            delivery_accepted: matches!((latest, final_seq), (Some((seq, state)), Some(last)) if state == "done" && seq == last),
            baseline_unchanged: crate::git::baseline_branch(&self.repo_root)
                .is_ok_and(|branch| p["baseline"] == branch.as_str()),
            commit_unchanged: p["commit"].is_string()
                && git(&["rev-parse", p["baseline"].as_str().unwrap_or("HEAD")])
                    .is_ok_and(|tip| p["commit"] == tip.as_str()),
        })
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
                serde_json::to_string(&p["evidence"]).unwrap_or_default().chars().take(PROMPT_EXCERPT_CHARS).collect::<String>(),
            ),
            forced: proposal_forced(&facts),
            fallback: "reject",
        })
    }
}

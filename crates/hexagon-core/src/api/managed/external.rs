//! Managed-followups ticket 05 (spec T3, ADR0087): the "external operations"
//! section of the delegation report. Full release lets delegation act through
//! apps, MCP and browser pages whose deletions the host cannot see, so the
//! owner gets a list of every such delegated action with an honest mark:
//! local items moved by host_fs_trash are host-verified recoverable; app, MCP
//! and browser actions are "host did not verify deletion".
//! Read-only, from the trace alone (managed decisions + host_trashed facts),
//! no business table. Codes and identifiers only — never tool output, typed
//! text, URL paths/queries, secrets or prompts.
use super::*;

/// One delegated external action as the owner sees it (D06 DTO).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, ts_rs::TS)]
#[ts(export, export_to = "../../../ui/src/gen/")]
pub struct ExternalOperation {
    /// The managed decision row on the timeline this entry comes from.
    #[ts(type = "number")]
    pub event_id: i64,
    pub card_id: String,
    /// local_trash | app | mcp | browser
    pub kind: String,
    /// Original path, app id, `server:tool`, or site host.
    pub target: String,
    /// The operation: move_to_trash, the browser/desktop op, or the MCP tool.
    pub action: String,
    /// The delegated decision (allow | deny).
    pub decision: String,
    /// done | incomplete | failed — the managed result classification.
    pub result: String,
    pub at: String,
    /// host_verified_recoverable | host_verified_not_moved |
    /// host_unverified_deletion
    pub verification: String,
    /// Where a trashed item now is, for the owner's restore.
    pub trash_path: Option<String>,
}

/// Which external surface a permission card's tool acts on, if any.
fn surface(tool: &str) -> Option<&'static str> {
    match tool {
        "host_fs_trash" => Some("local_trash"),
        "browser_action" => Some("browser"),
        "computer_action" | "computer_navigate" => Some("app"),
        _ if tool.starts_with("mcp:") => Some("mcp"),
        _ => None,
    }
}

/// Target and action from the scrubbed card input. Browser targets keep the
/// host only: a URL path or query can carry tokens.
fn target_and_action(kind: &str, tool: &str, input: &serde_json::Value) -> (String, String) {
    let text = |key: &str| input[key].as_str().unwrap_or_default().to_string();
    match kind {
        "local_trash" => (text("path"), "move_to_trash".into()),
        "browser" => {
            let host = input["url"]
                .as_str()
                .and_then(|url| url::Url::parse(url).ok())
                .and_then(|url| url.host_str().map(str::to_string))
                .unwrap_or_else(|| "current_page".into());
            (host, text("op"))
        }
        "app" => {
            let app = text("app_id");
            (
                if app.is_empty() {
                    "desktop".into()
                } else {
                    app
                },
                text("op"),
            )
        }
        _ => {
            let rest = tool.strip_prefix("mcp:").unwrap_or(tool);
            let tool_name = rest.split_once(':').map(|(_, t)| t).unwrap_or(rest);
            (rest.to_string(), tool_name.to_string())
        }
    }
}

/// The honest mark. Only a recorded host_trashed fact proves an item sits in
/// the Trash; a refused or failed trash moved nothing (one rename), which the
/// host also knows. Everything else is outside the host's view (ADR0087).
fn verification(kind: &str, moved_to_trash: bool) -> &'static str {
    match (kind, moved_to_trash) {
        ("local_trash", true) => "host_verified_recoverable",
        ("local_trash", false) => "host_verified_not_moved",
        _ => "host_unverified_deletion",
    }
}

/// Story 9/27 (真实结果): an allowed action's result is its execution state,
/// not the decision's landing. Unknown or never-run actions are incomplete;
/// an unreadable action falls back to the decision's own classification.
fn real_result(
    db: &Db,
    project_id: &str,
    action_id: &str,
    decision: &str,
    p: &serde_json::Value,
) -> String {
    let decided = result_of(decision, p["outcome"].as_str().unwrap_or_default());
    if decision != "allow" {
        return decided.into();
    }
    match crate::actions::get(db, project_id, action_id).map(|a| a.state) {
        Ok(state) => match state.as_str() {
            "succeeded" => "done",
            "failed" | "denied" => "failed",
            _ => "incomplete",
        }
        .into(),
        Err(_) => decided.into(),
    }
}

/// Newest first, at most `limit` entries; only delegated (managed actor)
/// decisions on permission cards for external surfaces.
pub fn managed_external_ops(
    db: &Db,
    project_id: &str,
    limit: usize,
) -> Result<Vec<ExternalOperation>, ApiError> {
    let events = db.events(project_id, Some(&[EventKind::System]))?;
    // action id -> (Trash location, when it moved)
    let trashed: HashMap<String, (String, String)> = events
        .iter()
        .filter(|event| event.payload["kind"] == "host_trashed")
        .filter_map(|event| {
            Some((
                event.payload["action_id"].as_str()?.to_string(),
                (
                    event.payload["trash_path"].as_str()?.to_string(),
                    event.created_at.clone(),
                ),
            ))
        })
        .collect();
    let mut out = vec![];
    for event in events.iter().rev() {
        let p = &event.payload;
        if p["kind"] != "managed_decision"
            || p["card_kind"] != "permission"
            || !p["actor"]
                .as_str()
                .is_some_and(|a| a.starts_with("managed:"))
        {
            continue;
        }
        let card_id = p["card_id"].as_str().unwrap_or_default();
        // Cards are never deleted, so a missing card means a corrupt row.
        // Listing it without its tool would claim an unknown surface; the
        // decision itself stays in managed_history, so the owner still sees it.
        let Ok(card) = crate::cards::get(db, card_id) else {
            continue;
        };
        let tool = card.payload["tool"].as_str().unwrap_or_default();
        let Some(kind) = surface(tool) else {
            continue;
        };
        let (target, action) = target_and_action(kind, tool, &card.payload["input"]);
        let decision = p["action"].as_str().unwrap_or_default();
        let action_id = card.payload["action_id"].as_str().unwrap_or_default();
        let moved = trashed.get(action_id).cloned();
        out.push(ExternalOperation {
            event_id: event.id,
            card_id: card_id.into(),
            kind: kind.into(),
            target,
            action,
            decision: decision.into(),
            result: real_result(db, project_id, action_id, decision, p),
            at: moved
                .as_ref()
                .map(|(_, at)| at.clone())
                .unwrap_or_else(|| event.created_at.clone()),
            verification: verification(kind, moved.is_some()).into(),
            trash_path: moved.map(|(path, _)| path),
        });
        if out.len() >= limit {
            break;
        }
    }
    Ok(out)
}

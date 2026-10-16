//! Display-only facts for the computer_action approval card (components D8,
//! control#action). Everything here is best-effort and fails toward "show
//! less": a lookup error yields None and the card still asks. Approval never
//! reads these values — the safety net asks on the tool name alone.
use super::screenshot_files;
use crate::db::Db;
use serde_json::{json, Value};
use std::path::Path;

/// ui-redesign gap D6 (2026-10-16, control#action): the computer_action card
/// shows the observation the action was planned on. Reuses the ticket 07/14
/// evidence store — a reference (local path + label) to the observation whose
/// snapshot_id the action names, never pixels, never a new copy. `activate`
/// carries no snapshot, so it falls back to the caller's latest observation.
///
/// Best-effort and display-only, so it fails toward "no thumbnail": any lookup
/// error, a mismatched snapshot or cleared evidence yields None and the card
/// still asks. It must never block the ask, and approval never reads it — a
/// stale picture would only mislead the owner, it cannot spend an action.
///
/// ui-redesign gap D8 (2026-10-17): also carries the observation's
/// `image_width`/`image_height`. click/drag x/y are screenshot pixels of that
/// observation (native `mapPoint` scales them onto the window bounds, so the
/// Retina point/pixel factor never reaches the model), which is what lets the
/// card place the target ring without knowing the display scale. Missing
/// dimensions (pre-D8 rows) → the UI draws no ring rather than guessing.
pub(crate) fn card_screenshot(
    db: &Db,
    ctx: &crate::tools::ToolContext,
    tool: &str,
    input: &Value,
) -> Option<Value> {
    if tool != "computer_action" {
        return None;
    }
    let result = planned_on(db, ctx, input["snapshot_id"].as_str())?;
    let path = result["screenshot_path"].as_str()?;
    let name = Path::new(path).file_name()?;
    // Cleared evidence (owner Clear) must not resurrect as a broken thumbnail.
    screenshot_files(&ctx.repo_root)
        .ok()?
        .iter()
        .any(|file| file.file_name() == Some(name))
        .then(|| {
            json!({"screenshot_path": path, "snapshot_id": result["snapshot_id"],
                "title": result["title"], "bundle_id": result["bundle_id"],
                "image_width": result["image_width"], "image_height": result["image_height"]})
        })
}

/// ui-redesign gap D8 (2026-10-17, control#action「系统设置 · 隐私与安全性」):
/// the card's app row shows a display name, not the raw bundle id (D6 shipped
/// `app_id`, which for click is absent and for activate is an opaque list_apps
/// receipt UUID). Resolution order, every step from evidence the agent itself
/// already produced — no new native call at ask time:
/// - activate: the latest list_apps entry whose receipt `app_id` matches.
/// - other ops: the observation the action names (window scope only; a
///   screen observation's title is a display name, not an app) → its
///   `app_name` (native, 2026-10-17+), else the latest list_apps `name` for
///   that bundle id; `title` = window title.
///
/// Returns `{name, title, bundle_id}` with nulls for what is unknown; the UI
/// falls back to the bundle id. None when nothing is known (the row is then
/// omitted, as before). A wrong name is worse than a bundle id, so names are
/// only taken from an exact receipt / bundle-id match, never fuzzily.
pub(crate) fn card_app(
    db: &Db,
    ctx: &crate::tools::ToolContext,
    tool: &str,
    input: &Value,
) -> Option<Value> {
    if tool != "computer_action" {
        return None;
    }
    if input["op"] == "activate" {
        let app_id = input["app_id"].as_str()?;
        let app = latest_listing(db, ctx)?
            .into_iter()
            .find(|app| app["app_id"] == app_id)?;
        return non_empty(json!({"name": text(&app["name"]),
            "title": Value::Null, "bundle_id": text(&app["bundle_id"])}));
    }
    // Non-activate ops are validated to carry a snapshot; never fall back to
    // "latest observation" here — that could be a different app.
    let result = planned_on(db, ctx, Some(input["snapshot_id"].as_str()?))?;
    if result["scope"] == "screen" {
        return None;
    }
    let bundle = text(&result["bundle_id"]);
    let name = text(&result["app_name"]).or_else(|| {
        let bundle = bundle.as_ref()?.as_str()?;
        latest_listing(db, ctx)?
            .into_iter()
            .find(|app| app["bundle_id"] == bundle)
            .and_then(|app| text(&app["name"]))
    });
    non_empty(json!({"name": name, "title": text(&result["title"]), "bundle_id": bundle}))
}

/// The caller's succeeded observation with this snapshot id (or its latest
/// observation when no snapshot is named — D6 activate fallback).
fn planned_on(db: &Db, ctx: &crate::tools::ToolContext, snapshot: Option<&str>) -> Option<Value> {
    let output: String = db
        .conn()
        .query_row(
            "SELECT output_json FROM tool_actions WHERE project_id=?1 AND agent_id=?2
             AND tool='computer_observe' AND state='succeeded' AND output_json IS NOT NULL
             AND (?3 IS NULL OR json_extract(output_json,'$.result.snapshot_id')=?3)
             ORDER BY rowid DESC LIMIT 1",
            rusqlite::params![ctx.project_id, ctx.agent_id, snapshot],
            |row| row.get(0),
        )
        .ok()?;
    let mut value: Value = serde_json::from_str(&output).ok()?;
    Some(value["result"].take())
}

/// The caller's most recent list_apps inventory (receipts are replaced on each
/// discovery, so only the latest one can match a live app_id).
fn latest_listing(db: &Db, ctx: &crate::tools::ToolContext) -> Option<Vec<Value>> {
    let output: String = db
        .conn()
        .query_row(
            "SELECT output_json FROM tool_actions WHERE project_id=?1 AND agent_id=?2
             AND tool='computer_observe' AND state='succeeded' AND output_json IS NOT NULL
             AND json_type(output_json,'$.applications')='array'
             ORDER BY rowid DESC LIMIT 1",
            rusqlite::params![ctx.project_id, ctx.agent_id],
            |row| row.get(0),
        )
        .ok()?;
    let mut value: Value = serde_json::from_str(&output).ok()?;
    match value["applications"].take() {
        Value::Array(apps) => Some(apps),
        _ => None,
    }
}

fn text(value: &Value) -> Option<Value> {
    value
        .as_str()
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(|s| Value::String(s.chars().take(200).collect()))
}

fn non_empty(value: Value) -> Option<Value> {
    value
        .as_object()?
        .values()
        .any(|v| !v.is_null())
        .then_some(value)
}

//! Ticket 03 / owner Q2/Q5: gate project visual source, not dependency assets.
//! This is a workflow gate, not a hostile-code detector. Unknown JS/TS project
//! semantics still need role/task review; no regex pretends to infer all UI.
use crate::{
    db::Db,
    sandbox::SandboxSpec,
    tools::{ToolContext, ToolError},
};
use serde_json::Value;

fn ui_source(path: &str) -> bool {
    let path = path.trim_start_matches("./").to_ascii_lowercase();
    let components: Vec<_> = path.split('/').collect();
    if components.iter().any(|part| {
        matches!(
            *part,
            "node_modules" | "dist" | "build" | ".next" | "coverage" | "vendor" | "target"
        )
    }) {
        return false;
    }
    let extension = path.rsplit('.').next().unwrap_or_default();
    matches!(
        extension,
        "tsx" | "jsx" | "css" | "scss" | "sass" | "less" | "html" | "vue" | "svelte"
    ) || matches!(extension, "ts" | "js")
        && components.iter().any(|part| {
            matches!(
                *part,
                "ui" | "frontend" | "client" | "components" | "pages" | "app"
            )
        })
}

pub(crate) fn guard_write(
    db: &Db,
    ctx: &ToolContext,
    name: &str,
    input: &Value,
) -> Result<Option<String>, ToolError> {
    if !matches!(name, "fs_write" | "fs_patch" | "artifact_write")
        || !input["path"].as_str().is_some_and(ui_source)
        || super::confirmed(db, &ctx.project_id)?
    {
        return Ok(None);
    }
    let started = std::time::Instant::now();
    let pending = super::require(db, &ctx.project_id, &ctx.agent_id)?;
    crate::diag::note(
        crate::diag::CLASS_REJECT,
        true,
        Some(&ctx.project_id),
        Some(&ctx.agent_id),
        ctx.stage_run_id.as_deref(),
        None,
        "design_direction",
        "owner_choice_required",
        started,
    );
    Ok(Some(format!("UI source implementation needs owner visual direction choice (revision {}). Use read_design_direction and propose_design for 2–3 static key-page mockups, then wait; the owner may confirm existing design guidance in the card.",pending.revision)))
}

pub(crate) fn guard_shell(db: &Db, ctx: &ToolContext, spec: SandboxSpec) -> SandboxSpec {
    match super::confirmed(db, &ctx.project_id) {
        Ok(true) => return spec,
        Err(_) => return SandboxSpec::Unavailable,
        _ => {}
    }
    let SandboxSpec::Seatbelt(mut profile) = spec else {
        return spec;
    };
    // Scope only root source files and known source trees. Root dependency/build
    // trees stay writable; refusing all CSS once blocked ordinary npm installs.
    // False negative delays a UI write for owner choice; false positive starts
    // implementation before the owner chose. Exact structured writes are also gated.
    for root in [
        ctx.repo_root.clone(),
        ctx.repo_root
            .canonicalize()
            .unwrap_or_else(|_| ctx.repo_root.clone()),
    ] {
        let Some(raw) = root.to_str() else {
            return SandboxSpec::Unavailable;
        };
        if raw.contains(['"', '\n', '\r']) {
            return SandboxSpec::Unavailable;
        }
        let escaped: String = raw
            .chars()
            .flat_map(|c| {
                if ".^$*+?()[]{}|\\".contains(c) {
                    vec!['\\', c]
                } else {
                    vec![c]
                }
            })
            .collect();
        let visual = "(tsx|jsx|css|scss|sass|less|html|vue|svelte)";
        for pattern in [
            format!("^{escaped}/[^/]*\\.{visual}$"),
            format!("^{escaped}/(src|components|pages|app|public|styles)/.*\\.{visual}$"),
            format!("^{escaped}/(components|pages|app)/.*\\.(js|ts)$"),
            format!("^{escaped}/src/(components|pages|app)/.*\\.(js|ts)$"),
            format!("^{escaped}/(ui|frontend|client|web|website)/[^/]*\\.(tsx|jsx|css|scss|sass|less|html|vue|svelte|js|ts)$"),
            format!("^{escaped}/(ui|frontend|client|web|website)/(src|components|pages|app|public|styles)/.*\\.(tsx|jsx|css|scss|sass|less|html|vue|svelte|js|ts)$"),
            // Spec review 2026-10-01: fs_write already blocked these common
            // monorepo sources; missing shell patterns provided a second route.
            // Package dependency/build directories deliberately remain outside.
            format!("^{escaped}/(apps|packages)/[^/]+/[^/]*\\.{visual}$"),
            format!("^{escaped}/(apps|packages)/[^/]+/(src|components|pages|app|public|styles)/.*\\.(tsx|jsx|css|scss|sass|less|html|vue|svelte|js|ts)$"),
            format!("^{escaped}/(apps|packages)/[^/]+/(ui|frontend|client)/[^/]*\\.(tsx|jsx|css|scss|sass|less|html|vue|svelte|js|ts)$"),
            format!("^{escaped}/(apps|packages)/[^/]+/(ui|frontend|client)/(src|components|pages|app|public|styles)/.*\\.(tsx|jsx|css|scss|sass|less|html|vue|svelte|js|ts)$"),
        ] {
            profile.push_str(&format!("(deny file-write* (regex #\"{pattern}\"))\n"));
        }
    }
    crate::diag::note(
        crate::diag::CLASS_JUDGE,
        false,
        Some(&ctx.project_id),
        Some(&ctx.agent_id),
        ctx.stage_run_id.as_deref(),
        None,
        "design_direction",
        "source_writes_wait_for_owner",
        std::time::Instant::now(),
    );
    SandboxSpec::Seatbelt(profile)
}

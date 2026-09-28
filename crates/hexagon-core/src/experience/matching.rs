//! Governance 06: applicability comes from host work records, never tool arguments.
use super::ExperienceConditions;
use crate::{db::Db, proposals::PropError, tools::ToolContext};
use rusqlite::{params, OptionalExtension};

pub(super) struct Context {
    role: Option<String>,
    stage: Option<String>,
    paths: Vec<String>,
}

pub(super) fn normalized_path(path: &str) -> bool {
    !path.is_empty()
        && !path.contains(['\\', ':', '*', '?', '[', ']'])
        && path
            .split('/')
            .all(|part| !part.is_empty() && part != "." && part != "..")
}

pub(super) fn context(db: &Db, ctx: &ToolContext) -> Result<Context, PropError> {
    let role = db
        .conn()
        .query_row(
            "SELECT role FROM agents WHERE project_id=?1 AND id=?2",
            params![ctx.project_id, ctx.agent_id],
            |r| r.get(0),
        )
        .optional()?;
    let active = db.active_stage_run(&ctx.project_id)?;
    let stage = active
        .map(|r| r.id)
        .filter(|id| ctx.stage_run_id.as_ref().is_none_or(|given| given == id));
    if ctx.stage_run_id.is_some() && stage.is_none() {
        return Ok(Context {
            role,
            stage: None,
            paths: Vec::new(),
        });
    }
    let activation: i64 = db.conn().query_row("SELECT COALESCE(MAX(id),0) FROM events WHERE project_id=?1 AND agent_id=?2 AND kind='agent_activated'",params![ctx.project_id,ctx.agent_id],|r|r.get(0))?;
    let mut query = db.conn().prepare("SELECT DISTINCT a.path FROM artifacts a WHERE a.project_id=?1 AND a.author_agent_id=?2 AND a.stage_run_id IS ?3 AND a.status IN ('valid','stamped') AND a.kind NOT IN ('改进提案','复审意见') AND EXISTS(SELECT 1 FROM events e WHERE e.project_id=a.project_id AND e.agent_id=a.author_agent_id AND e.kind='artifact_delivered' AND e.id>?4 AND json_extract(e.payload,'$.artifact_id')=a.id) ORDER BY a.path")?;
    let paths = query
        .query_map(
            params![ctx.project_id, ctx.agent_id, stage, activation],
            |r| r.get(0),
        )?
        .collect::<Result<Vec<String>, _>>()?;
    Ok(Context { role, stage, paths })
}

pub(super) fn reason(conditions: &ExperienceConditions, ctx: &Context) -> &'static str {
    // Missing/ambiguous host context must exclude advice. False negatives cost
    // a manual review; false positives apply guidance outside its approved scope.
    if !conditions.roles.is_empty() {
        let Some(role) = &ctx.role else {
            return "missing_role";
        };
        if !conditions.roles.contains(role) {
            return "role_mismatch";
        }
    }
    if !conditions.stages.is_empty() {
        let Some(stage) = &ctx.stage else {
            return "missing_stage";
        };
        if !conditions.stages.contains(stage) {
            return "stage_mismatch";
        }
    }
    if !conditions.paths.is_empty() {
        if ctx.paths.is_empty() {
            return "missing_paths";
        }
        if !ctx.paths.iter().all(|path| {
            normalized_path(path)
                && conditions.paths.iter().any(|prefix| {
                    normalized_path(prefix)
                        && (path == prefix
                            || path
                                .strip_prefix(prefix)
                                .is_some_and(|suffix| suffix.starts_with('/')))
                })
        }) {
            return "path_mismatch";
        }
    }
    "matched"
}

#[cfg(test)]
mod tests {
    use super::*;
    use proptest::prelude::*;
    #[test]
    fn optional_dimensions_are_anded_and_missing_context_is_not_a_match() {
        let conditions = ExperienceConditions {
            roles: vec!["author".into(), "reviewer".into()],
            stages: vec!["run-1".into()],
            paths: vec![],
        };
        let mut ctx = Context {
            role: Some("author".into()),
            stage: Some("run-1".into()),
            paths: vec![],
        };
        assert_eq!(reason(&conditions, &ctx), "matched");
        ctx.stage = None;
        assert_eq!(reason(&conditions, &ctx), "missing_stage");
        ctx.stage = Some("run-2".into());
        assert_eq!(reason(&conditions, &ctx), "stage_mismatch");
        ctx.role = Some("other".into());
        assert_eq!(reason(&conditions, &ctx), "role_mismatch");
        ctx.role = None;
        assert_eq!(reason(&conditions, &ctx), "missing_role");
    }
    proptest! {
        #[test]
        fn every_known_path_must_be_covered(part in "[a-z]{1,20}") {
            let conditions = ExperienceConditions { paths: vec!["src".into()], ..Default::default() };
            let mut ctx = Context { role: None, stage: None, paths: vec![format!("src/{part}")] };
            prop_assert_eq!(reason(&conditions,&ctx), "matched");
            ctx.paths.push(format!("src-extra/{part}"));
            prop_assert_eq!(reason(&conditions,&ctx), "path_mismatch");
            ctx.paths.clear();
            prop_assert_eq!(reason(&conditions,&ctx), "missing_paths");
        }
    }
}

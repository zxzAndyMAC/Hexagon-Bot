//! Owner decision 2026-10-01 Q2/Q5: a model can propose a visual direction,
//! only an owner control command can choose it. Bytes and revision are immutable.
use crate::{
    db::Db,
    tools::{Tool, ToolContext, ToolError},
    trace::EventKind,
};
use rusqlite::OptionalExtension;
use serde_json::json;
mod guard;
mod preview;
pub(crate) use guard::{guard_shell, guard_write};

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, ts_rs::TS)]
#[ts(export, export_to = "../../../ui/src/gen/")]
pub struct DesignMockupInput {
    pub page: String,
    pub mime: String,
    /// SVG source, or base64 PNG bytes; never an external URL.
    pub content: String,
}
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, ts_rs::TS)]
#[ts(export, export_to = "../../../ui/src/gen/")]
pub struct DesignOptionInput {
    pub id: String,
    pub title: String,
    pub description: String,
    pub layout: String,
    pub typography: String,
    pub palette: String,
    pub mockups: Vec<DesignMockupInput>,
}
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, ts_rs::TS)]
#[ts(export, export_to = "../../../ui/src/gen/")]
pub struct DesignMockup {
    pub page: String,
    pub mime: String,
    pub digest: String,
    pub data_url: String,
}
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, ts_rs::TS)]
#[ts(export, export_to = "../../../ui/src/gen/")]
pub struct DesignOption {
    pub id: String,
    pub title: String,
    pub description: String,
    pub layout: String,
    pub typography: String,
    pub palette: String,
    pub mockups: Vec<DesignMockup>,
}
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, ts_rs::TS)]
#[ts(export, export_to = "../../../ui/src/gen/")]
pub struct DesignDirection {
    pub project_id: String,
    #[ts(type = "number")]
    pub revision: i64,
    pub question_id: Option<String>,
    #[ts(type = "'unconfirmed' | 'pending' | 'selected' | 'existing'")]
    pub state: String,
    pub options: Vec<DesignOption>,
    pub selected_option: Option<String>,
    pub existing_guidance: Option<String>,
    pub selected_at: Option<String>,
}
fn invalid(message: &str) -> ToolError {
    ToolError::BadInput(message.into())
}
fn bounded(text: &str, limit: usize) -> bool {
    !text.trim().is_empty() && text.len() <= limit
}

pub fn read(db: &Db, project: &str) -> Result<DesignDirection, ToolError> {
    let value = db.conn().query_row(
        "SELECT revision,question_id,state,options_json,selected_option,existing_guidance,selected_at FROM design_directions WHERE project_id=?1 ORDER BY revision DESC LIMIT 1",
        [project], |row| Ok((row.get::<_,i64>(0)?, row.get::<_,Option<String>>(1)?, row.get::<_,String>(2)?, row.get::<_,String>(3)?, row.get::<_,Option<String>>(4)?, row.get::<_,Option<String>>(5)?, row.get::<_,Option<String>>(6)?))
    ).optional()?;
    match value {
        Some((
            revision,
            question_id,
            state,
            options,
            selected_option,
            existing_guidance,
            selected_at,
        )) => Ok(DesignDirection {
            project_id: project.into(),
            revision,
            question_id,
            state,
            options: serde_json::from_str(&options)
                .map_err(|_| invalid("stored design direction is invalid"))?,
            selected_option,
            existing_guidance,
            selected_at,
        }),
        None => Ok(DesignDirection {
            project_id: project.into(),
            revision: 0,
            question_id: None,
            state: "unconfirmed".into(),
            options: vec![],
            selected_option: None,
            existing_guidance: None,
            selected_at: None,
        }),
    }
}

pub fn pending(db: &Db, project: &str) -> rusqlite::Result<bool> {
    Ok(db.conn().query_row("SELECT state='pending' FROM design_directions WHERE project_id=?1 ORDER BY revision DESC LIMIT 1", [project], |r| r.get(0)).optional()?.unwrap_or(false))
}

pub fn confirmed(db: &Db, project: &str) -> Result<bool, ToolError> {
    let state: Option<String> = db.conn().query_row("SELECT state FROM design_directions WHERE project_id=?1 ORDER BY revision DESC LIMIT 1", [project], |row| row.get(0)).optional()?;
    Ok(matches!(state.as_deref(), Some("selected" | "existing")))
}

/// A bounded summary goes into every role's brief. Image bytes stay out of the
/// prompt; selected mockup hashes identify the same version the owner reviewed.
pub fn brief(db: &Db, project: &str) -> Result<serde_json::Value, ToolError> {
    let direction = read(db, project)?;
    let selected = direction
        .options
        .iter()
        .find(|option| Some(&option.id) == direction.selected_option.as_ref());
    Ok(
        json!({"state":direction.state,"revision":direction.revision,
        "owner_confirmed":direction.selected_at.is_some(),"existing_guidance":direction.existing_guidance,
        "selected":selected.map(|option| json!({"id":option.id,"title":option.title,"description":option.description,"layout":option.layout,"typography":option.typography,"palette":option.palette,
            "mockups":option.mockups.iter().map(|image| json!({"page":image.page,"digest":image.digest})).collect::<Vec<_>>() }))}),
    )
}

pub fn require(db: &Db, project: &str, agent: &str) -> Result<DesignDirection, ToolError> {
    let current = read(db, project)?;
    if current.state != "unconfirmed" {
        return Ok(current);
    }
    let tx = db.conn().unchecked_transaction()?;
    let question = crate::cards::enqueue(
        db,
        project,
        Some(agent),
        crate::cards::CardKind::Escalation,
        json!({"sub":"design_direction","revision":1}),
        Some("design-direction:1"),
    )?;
    db.conn().execute("INSERT INTO design_directions(project_id,revision,question_id,state,submitted_by) VALUES(?1,1,?2,'pending',?3)", rusqlite::params![project,question,agent])?;
    db.append_event(
        project,
        EventKind::System,
        json!({"kind":"design_direction_requested","revision":1,"question_id":question}),
        Some(agent),
        None,
    )?;
    tx.commit()?;
    read(db, project)
}

pub fn submit(
    db: &Db,
    ctx: &ToolContext,
    expected_revision: i64,
    options: Vec<DesignOptionInput>,
) -> Result<DesignDirection, ToolError> {
    if !(2..=3).contains(&options.len()) {
        return Err(invalid("submit two or three distinct visual directions"));
    }
    let mut ids = std::collections::HashSet::new();
    let mut digests = std::collections::HashSet::new();
    let mut checked = Vec::new();
    for option in options {
        if !bounded(&option.id, 64)
            || !ids.insert(option.id.clone())
            || !bounded(&option.title, 120)
            || !bounded(&option.description, 2000)
            || !bounded(&option.layout, 1200)
            || !bounded(&option.typography, 800)
            || !bounded(&option.palette, 800)
            || !(1..=3).contains(&option.mockups.len())
        {
            return Err(invalid("each direction needs a distinct id, explanation, layout, typography, palette and one to three key-page mockups"));
        }
        let mut mockups = Vec::new();
        for mockup in option.mockups {
            let preview = preview::validate(mockup)?;
            if !digests.insert(preview.digest.clone()) {
                return Err(invalid("directions must not reuse an identical mockup"));
            }
            mockups.push(preview);
        }
        checked.push(DesignOption {
            id: option.id,
            title: option.title,
            description: option.description,
            layout: option.layout,
            typography: option.typography,
            palette: option.palette,
            mockups,
        });
    }
    let tx = db.conn().unchecked_transaction()?;
    let current = read(db, &ctx.project_id)?;
    if current.revision != expected_revision
        || matches!(current.state.as_str(), "selected" | "existing")
    {
        return Err(invalid(
            "design revision changed or owner has already confirmed the direction",
        ));
    }
    if let Some(question) = current.question_id {
        crate::cards::answer(db, &question, "superseded")?;
    }
    let revision = current.revision + 1;
    let question = crate::cards::enqueue(
        db,
        &ctx.project_id,
        Some(&ctx.agent_id),
        crate::cards::CardKind::Escalation,
        json!({"sub":"design_direction","revision":revision}),
        Some(&format!("design-direction:{revision}")),
    )?;
    let serialized =
        serde_json::to_string(&checked).map_err(|_| invalid("cannot serialize design options"))?;
    db.conn().execute("INSERT INTO design_directions(project_id,revision,question_id,state,options_json,submitted_by) VALUES(?1,?2,?3,'pending',?4,?5)",rusqlite::params![ctx.project_id,revision,question,serialized,ctx.agent_id])?;
    db.append_event(&ctx.project_id,EventKind::System,json!({"kind":"design_options_proposed","revision":revision,"question_id":question,"option_ids":checked.iter().map(|v| &v.id).collect::<Vec<_>>()}),Some(&ctx.agent_id),ctx.stage_run_id.as_deref())?;
    tx.commit()?;
    read(db, &ctx.project_id)
}

/// Owner-only control facade: no agent tool exposes selection or reuse.
/// CAS binds the decision to the exact immutable bytes/revision shown in UI.
fn choose_inner(
    db: &Db,
    project: &str,
    question: &str,
    revision: i64,
    option_id: Option<&str>,
    existing_guidance: Option<&str>,
) -> Result<DesignDirection, ToolError> {
    let started = std::time::Instant::now();
    let tx = db.conn().unchecked_transaction()?;
    let current = read(db, project)?;
    let card = crate::cards::get_queued(db, question, crate::cards::CardKind::Escalation)?;
    if current.state != "pending"
        || current.revision != revision
        || current.question_id.as_deref() != Some(question)
        || card.project_id != project
        || card.payload["sub"] != "design_direction"
    {
        return Err(invalid(
            "design choice is stale; review the current proposal",
        ));
    }
    let state = match (option_id, existing_guidance) {
        (Some(id), None) if current.options.iter().any(|option| option.id == id) => "selected",
        (None, Some(guidance)) if bounded(guidance, 4000) => "existing",
        _ => {
            return Err(invalid(
                "choose one proposed direction, or provide existing owner-approved guidance",
            ))
        }
    };
    db.conn().execute("UPDATE design_directions SET state=?1,selected_option=?2,existing_guidance=?3,selected_at=datetime('now') WHERE project_id=?4 AND revision=?5 AND state='pending'",rusqlite::params![state,option_id,existing_guidance,project,revision])?;
    crate::cards::answer(db, question, "owner")?;
    // A frontend role can request the decision and a design role can replace
    // that empty card with mockups. Wake both originators, not only the latest
    // proposer; otherwise the implementer remains asleep after owner selection.
    let waiting = {
        let mut query=db.conn().prepare("SELECT DISTINCT submitted_by FROM design_directions WHERE project_id=?1 AND revision<=?2 AND submitted_by IS NOT NULL")?;
        let agents = query
            .query_map(rusqlite::params![project, revision], |r| {
                r.get::<_, String>(0)
            })?
            .collect::<Result<Vec<_>, _>>()?;
        agents
    };
    for agent in waiting {
        crate::orchestra::write_agent_status(db, project, &agent, false)?;
        db.append_event(
            project,
            EventKind::AgentActivated,
            json!({"by":"owner_design_choice","revision":revision,"question_id":question}),
            Some(&agent),
            None,
        )?;
    }
    db.append_event(project,EventKind::System,json!({"kind":"design_direction_selected","revision":revision,"option_id":option_id,"source":state,"question_id":question}),None,None)?;
    tx.commit()?;
    crate::diag::note(
        crate::diag::CLASS_JUDGE,
        false,
        Some(project),
        None,
        None,
        None,
        "design_direction",
        "owner_selected",
        started,
    );
    read(db, project)
}

pub struct ProposeDesign;
impl Tool for ProposeDesign {
    fn name(&self) -> &str {
        "propose_design"
    }
    fn description(&self) -> &str {
        "Use when a visual direction is unconfirmed before full UI implementation: submit 2-3 distinct key-page static visual mockups with layout, typography, colors and rationale. SVG uses only static shapes/text with an SVG namespace and bounded viewBox; no style, scripts, foreignObject or external links. PNG content is base64. Call read_design_direction first for expected_revision; an owner-confirmed direction cannot be replaced by this tool. The owner chooses in a pending card. Do not use to claim an owner choice or replace an approved direction."
    }
    fn input_schema(&self) -> serde_json::Value {
        json!({"type":"object","required":["expected_revision","options"],"properties":{"expected_revision":{"type":"integer","minimum":0,"description":"Current revision returned by read_design_direction; rejects stale proposals."},"options":{"type":"array","description":"Distinct key-page visual options with static mockups and design rationale for the owner to select.","minItems":2,"maxItems":3,"items":{"type":"object","required":["id","title","description","layout","typography","palette","mockups"],"properties":{"id":{"type":"string"},"title":{"type":"string"},"description":{"type":"string"},"layout":{"type":"string"},"typography":{"type":"string"},"palette":{"type":"string"},"mockups":{"type":"array","minItems":1,"maxItems":3,"items":{"type":"object","required":["page","mime","content"],"properties":{"page":{"type":"string"},"mime":{"enum":["image/svg+xml","image/png"]},"content":{"type":"string"}}}}}}}}})
    }
    fn risk(&self) -> crate::tools::RiskClass {
        crate::tools::RiskClass::WriteLocal
    }
    fn exec(
        &self,
        db: &Db,
        input: &serde_json::Value,
        ctx: &ToolContext,
    ) -> Result<serde_json::Value, ToolError> {
        let options = serde_json::from_value(input["options"].clone())
            .map_err(|_| invalid("invalid visual options"))?;
        let revision = input["expected_revision"]
            .as_i64()
            .ok_or_else(|| invalid("expected_revision is required"))?;
        let started = std::time::Instant::now();
        let value = submit(db, ctx, revision, options).inspect_err(|_| {
            crate::diag::note(
                crate::diag::CLASS_REJECT,
                true,
                Some(&ctx.project_id),
                Some(&ctx.agent_id),
                ctx.stage_run_id.as_deref(),
                None,
                "design_direction",
                "invalid_or_stale_proposal",
                started,
            );
        })?;
        Ok(
            json!({"revision":value.revision,"question_id":value.question_id,"state":value.state,"awaiting":"owner visual direction choice"}),
        )
    }
}
pub struct ReadDesignDirection;
impl Tool for ReadDesignDirection {
    fn name(&self) -> &str {
        "read_design_direction"
    }
    fn description(&self) -> &str {
        "Use when preparing UI work to read the host-recorded owner-approved visual direction and immutable revision. Unconfirmed/pending means propose mockups and wait before full UI implementation. Do not use as an owner approval; this only reads recorded state."
    }
    fn input_schema(&self) -> serde_json::Value {
        json!({"type":"object","properties":{}})
    }
    fn risk(&self) -> crate::tools::RiskClass {
        crate::tools::RiskClass::Read
    }
    fn exec(
        &self,
        db: &Db,
        _input: &serde_json::Value,
        ctx: &ToolContext,
    ) -> Result<serde_json::Value, ToolError> {
        brief(db, &ctx.project_id)
    }
}

/// Every owner control rejection has a diagnosable branch without logging content.
pub fn choose(
    db: &Db,
    project: &str,
    question: &str,
    revision: i64,
    option_id: Option<&str>,
    existing_guidance: Option<&str>,
) -> Result<DesignDirection, ToolError> {
    let started = std::time::Instant::now();
    let result = choose_inner(
        db,
        project,
        question,
        revision,
        option_id,
        existing_guidance,
    );
    if result.is_err() {
        crate::diag::note(
            crate::diag::CLASS_REJECT,
            true,
            Some(project),
            None,
            None,
            None,
            "design_direction",
            "invalid_or_stale_owner_choice",
            started,
        );
    }
    result
}

//! Project experience. Governance 01 keeps legacy requests inspectable but
//! refuses their whole-file application/rollback. Legacy skill sections are
//! withheld from default loading until they have governed entry receipts.
//! ADR 0069 / experience-governance Q1–Q18 supersede read-every-skill copying.

mod controls;
mod curation;
pub use curation::{propose as curate_legacy, ExperienceCuration, LegacyRange};
mod editing;
pub use controls::{revoke, ExperienceRevocation};
mod governance;
mod history;
pub use editing::{read as read_project_skill, save as save_project_skill, ProjectSkillDocument};
pub(crate) use governance::validate_review;
pub use history::{
    query as history, ExperienceHistoryItem, ExperienceHistoryKind, ExperienceHistoryPage,
    ExperienceHistoryRequest,
};
mod limits;
mod matching;
mod role_skill;
pub use limits::{read as loading_limits, set as set_loading_limits, ExperienceLimits};
mod storage;
pub use governance::{
    proposal_view, ExperienceChange, ExperienceChangeKind, ExperienceConditions,
    ExperienceProposalView, ExperienceRequest, ExperienceSource, ExperienceSubmission,
    ExperienceTarget,
};
pub use storage::{
    entries_view as entries, recover, rollback_contribution, ExperienceEntry, ExperienceEntryView,
    ExperienceRecovery,
};
#[cfg(test)]
pub use storage::{fail_next, ExperienceFault};

use crate::db::Db;
use crate::proposals::{self, PropError};
use crate::tools::ToolContext;
use serde_json::{json, Value};
use std::path::Path;

pub const SECTION: &str = "## 经验";

/// Governance 01 / Q12: legacy prose lacks a host receipt. False negatives
/// cost an owner review; false positives inject unreviewed lessons. Prefer
/// withholding the experience section, while preserving the source file.
/// Returns the default loading view and the number of withheld sections.
pub fn legacy_loading_view(markdown: &str) -> (String, u32) {
    let (view, headings, _) = legacy_view(markdown);
    (view, headings)
}
/// A governed-only section is not an unresolved legacy lesson (Governance 16).
pub fn unmanaged_experience_sections(markdown: &str) -> u32 {
    legacy_view(&storage::without_blocks(markdown)).2
}
fn legacy_view(markdown: &str) -> (String, u32, u32) {
    let mut result = String::new();
    let mut hidden = false;
    let mut blocks = 0;
    let mut unmanaged = 0;
    let mut counted = false;
    let mut fence: Option<(char, usize)> = None;
    for line in markdown.split_inclusive('\n') {
        let trimmed = line.trim();
        let at_margin =
            line.len() - line.trim_start_matches(' ').len() <= 3 && !line.starts_with('\t');
        if hidden
            && !counted
            && !trimmed.is_empty()
            && !trimmed.starts_with("## ")
            && !trimmed.starts_with("# ")
            && !trimmed.starts_with("<!-- hexagon-stopped-experience:")
        {
            unmanaged += 1;
            counted = true;
        }
        let first = trimmed.chars().next();
        if at_margin && matches!(first, Some('`' | '~')) {
            let marker = first.unwrap();
            let width = trimmed.chars().take_while(|c| *c == marker).count();
            if width >= 3 {
                match fence {
                    Some((open, length))
                        if open == marker
                            && width >= length
                            && trimmed[width..].trim().is_empty() =>
                    {
                        fence = None
                    }
                    // Governance 01 review: ```inline``` is not an opening
                    // fence. Treating it as one hid the following real heading
                    // from this filter and leaked the legacy lesson.
                    None if marker != '`' || !trimmed[width..].contains('`') => {
                        fence = Some((marker, width))
                    }
                    _ => {}
                }
                if !hidden {
                    result.push_str(line);
                }
                continue;
            }
        }
        if fence.is_none() && at_margin {
            let level = trimmed.chars().take_while(|c| *c == '#').count();
            if (1..=6).contains(&level) && trimmed[level..].starts_with(char::is_whitespace) {
                let title = trimmed[level..].trim().trim_end_matches('#').trim_end();
                if level <= 2 {
                    hidden = false;
                }
                if level == 2 && title == "经验" {
                    hidden = true;
                    counted = false;
                    blocks += 1;
                    result.push_str("## 经验\n[Legacy experience withheld: not yet governed.]\n");
                }
            }
        }
        if !hidden {
            result.push_str(line);
        }
    }
    (result, blocks, unmanaged)
}

/// 已有节则在节末追加，没有就新建。旧正文留着。
pub fn append_lesson(markdown: &str, lesson: &str) -> String {
    let lesson = lesson.trim();
    if let Some(pos) = markdown.find(SECTION) {
        let after = pos + SECTION.len();
        let rest = &markdown[after..];
        if let Some(rel) = rest.find("\n## ") {
            let at = after + rel;
            return format!("{}\n{lesson}\n{}", &markdown[..at], &markdown[at..]);
        }
        let base = markdown.trim_end();
        return format!("{base}\n{lesson}\n");
    }
    let base = markdown.trim_end();
    if base.is_empty() {
        format!("{SECTION}\n{lesson}\n")
    } else {
        format!("{base}\n\n{SECTION}\n{lesson}\n")
    }
}

fn new_skill(role: &str, lesson: &str) -> String {
    let dir = crate::skills::experience_dir_name(role);
    format!(
        "---\nname: {dir}\ndescription: 本角色积累的做法，用到时再读正文\n---\n\n{SECTION}\n{lesson}\n",
        lesson = lesson.trim()
    )
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
struct Qualification {
    review_event: i64,
    activation: i64,
    evidence: crate::artifacts::evidence::ArtifactEvidence,
}

struct Gate {
    role: String,
    skills: Vec<String>,
    qualification: Option<Qualification>,
    delivered: bool,
}

fn gate(db: &Db, ctx: &ToolContext, review_event: Option<i64>) -> Result<Gate, PropError> {
    let role: String = db
        .conn()
        .query_row(
            "SELECT role FROM agents WHERE id=?1 AND project_id=?2",
            rusqlite::params![ctx.agent_id, ctx.project_id],
            |r| r.get(0),
        )
        .map_err(|_| PropError::Rejected("agent not in project".into()))?;
    let skills_json: String = db
        .conn()
        .query_row(
            "SELECT skills FROM role_defs WHERE project_id=?1 AND name=?2",
            rusqlite::params![ctx.project_id, role],
            |r| r.get(0),
        )
        .unwrap_or_else(|_| "[]".into());
    let mut skills: Vec<String> = serde_json::from_str(&skills_json).unwrap_or_default();
    if skills.is_empty() {
        let mut st = db
            .conn()
            .prepare("SELECT name FROM grants WHERE agent_id=?1 AND kind='skill' ORDER BY name")?;
        skills = st
            .query_map([&ctx.agent_id], |r| r.get(0))?
            .collect::<Result<_, _>>()?;
    }
    // Reliability 20: the event's agent is the reviewer, not the author.
    // Unknown legacy links cost a fresh review; accepting them could award
    // another instance's experience, so eligibility deliberately fails closed.
    let run = ctx
        .stage_run_id
        .clone()
        .or(db.active_stage_run(&ctx.project_id)?.map(|r| r.id));
    let activation: i64 = db.conn().query_row(
        "SELECT COALESCE(MAX(id),0) FROM events WHERE project_id=?1 AND agent_id=?2 AND kind='agent_activated'",
        rusqlite::params![ctx.project_id,ctx.agent_id], |r| r.get(0))?;
    let mut query = db.conn().prepare(
        "SELECT e.id,e.payload FROM events e WHERE e.project_id=?1 AND e.kind='review_passed'
         AND e.stage_run_id IS ?2 AND e.agent_id != ?3 AND e.id > ?4
         AND json_extract(e.payload,'$.evidence.author')=?3
         AND NOT EXISTS(SELECT 1 FROM events later WHERE later.project_id=e.project_id
             AND later.kind IN ('review_passed','review_rejected') AND later.id>e.id
             AND json_extract(later.payload,'$.artifact_id')=json_extract(e.payload,'$.artifact_id')
             AND json_extract(later.payload,'$.reviewer')=json_extract(e.payload,'$.reviewer'))
         ORDER BY e.id DESC",
    )?;
    let rows = query
        .query_map(
            rusqlite::params![ctx.project_id, run, ctx.agent_id, activation],
            |r| Ok((r.get::<_, i64>(0)?, r.get::<_, String>(1)?)),
        )?
        .collect::<Result<Vec<_>, _>>()?;
    let mut qualification = None;
    for (id, raw) in rows {
        // Reliability 20 review: another artifact's later review must not
        // invalidate the original source of an already queued lesson.
        if review_event.is_some_and(|expected| expected != id) {
            continue;
        }
        let payload: Value = serde_json::from_str(&raw).unwrap_or_default();
        let Ok(evidence) = serde_json::from_value::<crate::artifacts::evidence::ArtifactEvidence>(
            payload["evidence"].clone(),
        ) else {
            continue;
        };
        if evidence.stage != run
            || evidence.project != ctx.project_id
            || evidence.author != ctx.agent_id
            || matches!(evidence.kind.as_str(), "改进提案" | "复审意见")
        {
            continue;
        }
        let delivery: bool = db.conn().query_row(
            "SELECT EXISTS(SELECT 1 FROM events WHERE project_id=?1 AND agent_id=?2 AND kind='artifact_delivered'
             AND stage_run_id IS ?3 AND id>?4 AND id<?5 AND json_extract(payload,'$.artifact_id')=?6)",
            rusqlite::params![ctx.project_id,ctx.agent_id,run,activation,id,evidence.id], |r|r.get(0))?;
        if delivery
            && crate::artifacts::evidence::matches(
                Some(&evidence),
                crate::artifacts::evidence::capture(
                    db,
                    &ctx.repo_root,
                    &ctx.project_id,
                    &evidence.id,
                )?
                .as_ref(),
            )
        {
            qualification = Some(Qualification {
                review_event: id,
                activation,
                evidence,
            });
            break;
        }
    }
    // A later delivery freezes this author's current work only. Reviewer
    // reports, improvement proposals and another instance's deliveries do not.
    let delivered = if let Some(source) = &qualification {
        db.conn().query_row(
            "SELECT EXISTS(SELECT 1 FROM events WHERE project_id=?1 AND agent_id=?2
            AND stage_run_id IS ?3 AND kind='artifact_delivered' AND id>?4
            AND COALESCE(json_extract(payload,'$.kind'),'') NOT IN ('改进提案','复审意见'))",
            rusqlite::params![ctx.project_id, ctx.agent_id, run, source.review_event],
            |r| r.get(0),
        )?
    } else {
        false
    };
    Ok(Gate {
        role,
        skills,
        qualification,
        delivered,
    })
}

/// 提交一份经验提案。还不写技能、不写授权。执行判定通过之后才落盘。
pub fn propose(
    db: &Db,
    ctx: &ToolContext,
    lesson: &str,
    read: &[String],
) -> Result<String, PropError> {
    let started = std::time::Instant::now();
    let lesson = lesson.trim();
    if lesson.is_empty() {
        return Err(PropError::Rejected("empty lesson".into()));
    }
    let gate = gate(db, ctx, None)?;
    if gate.qualification.is_none() {
        crate::diag::note(
            "拒绝",
            true,
            Some(&ctx.project_id),
            Some(&ctx.agent_id),
            ctx.stage_run_id.as_deref(),
            None,
            "experience",
            "unreviewed",
            started,
        );
        return Err(PropError::UnreviewedExperience);
    }
    if gate.delivered {
        crate::diag::note(
            "拒绝",
            true,
            Some(&ctx.project_id),
            Some(&ctx.agent_id),
            ctx.stage_run_id.as_deref(),
            None,
            "experience",
            "delivered",
            started,
        );
        return Err(PropError::FrozenExperience);
    }
    if gate.role.contains('/') || gate.role.contains('\\') {
        return Err(PropError::Rejected(
            "role name must not contain a path separator".into(),
        ));
    }

    let shared_name = crate::skills::experience_dir_name(&gate.role);
    let shared_path = ctx
        .repo_root
        .join(".hexagon/skills")
        .join(&shared_name)
        .join("SKILL.md");
    let shared_exists = shared_path.is_file();
    if shared_exists {
        let existing = std::fs::read_to_string(&shared_path).unwrap_or_default();
        let looks_like = existing.contains(SECTION) || existing.contains("name: 经验-");
        if !looks_like {
            return Err(PropError::Rejected(
                "target is already a different skill".into(),
            ));
        }
    }
    // 名单空且这份经验还不存在，才新建。同角色第二个人写已经存在的那一份。
    let creating = gate.skills.is_empty() && !shared_exists;
    let targets: Vec<String> = if !read.is_empty() {
        if gate.skills.is_empty() {
            return Err(PropError::Rejected(
                "cannot mint a skill name while the list is not empty".into(),
            ));
        }
        read.to_vec()
    } else if let Some(first) = gate.skills.first() {
        vec![first.clone()]
    } else {
        vec![shared_name]
    };
    if !gate.skills.is_empty() && targets.iter().any(|t| !gate.skills.contains(t)) {
        return Err(PropError::Rejected(
            "cannot mint a skill name while the list is not empty".into(),
        ));
    }

    let mut files = Vec::new();
    for name in &targets {
        if name.contains('/') || name.contains('\\') {
            return Err(PropError::Rejected(
                "skill name must not contain a path separator".into(),
            ));
        }
        let rel = format!(".hexagon/skills/{name}/SKILL.md");
        let path = ctx.repo_root.join(&rel);
        if creating {
            if path.exists() {
                let existing = std::fs::read_to_string(&path).unwrap_or_default();
                if !existing.contains("name: 经验-") && !existing.contains(SECTION) {
                    return Err(PropError::Rejected(
                        "target is already a different skill".into(),
                    ));
                }
            }
            // 另一角色的经验是另一份文件，这里只拒绝「目录已是别的技能」。
            let parent = path.parent().unwrap();
            if parent.exists() && !path.exists() {
                return Err(PropError::Rejected(
                    "target is already a different skill".into(),
                ));
            }
            files.push(json!({"path": rel, "after": new_skill(&gate.role, lesson), "grant": name}));
        } else {
            let old = std::fs::read_to_string(&path)
                .unwrap_or_else(|_| format!("---\nname: {name}\ndescription: 技能\n---\n\n"));
            let after = append_lesson(&old, lesson);
            if !after.contains(SECTION)
                || after.matches(SECTION).count() != old.matches(SECTION).count().max(1)
            {
                return Err(PropError::Rejected(
                    "experience section must be appended".into(),
                ));
            }
            if old.contains(SECTION) && !after.contains(lesson) {
                return Err(PropError::Rejected(
                    "experience section must be appended".into(),
                ));
            }
            let mut entry = json!({"path": rel, "after": after});
            if name.starts_with("经验-") {
                entry["grant"] = json!(name);
            }
            files.push(entry);
        }
    }
    crate::diag::note(
        crate::diag::CLASS_JUDGE,
        false,
        Some(&ctx.project_id),
        Some(&ctx.agent_id),
        ctx.stage_run_id.as_deref(),
        None,
        "experience",
        "author_qualified",
        started,
    );
    let payload = json!({"lesson": lesson, "files": files, "create": creating, "qualification": gate.qualification});
    let diff = format!("+ {lesson}");
    let body = format!(
        "---\nkind: 改进提案\nauthor: {}\nsurface: skill\ntarget: {}\n---\n\
         ## 动机\n复审通过后的教训\n\n## 改动面\n```diff\n{diff}\n```\n\n\
         ## 预期收益\n下次激活能复用\n\n## 验证方法\n读技能的经验节\n\n\
         ```experience\n{}\n```\n",
        ctx.agent_id,
        files[0]["path"].as_str().unwrap_or(""),
        serde_json::to_string_pretty(&payload).unwrap_or_default()
    );
    // 技能 diff 不得带 grant/permission 字样。教训正文里如果有这些词会被拒。
    // 经验提案的授权不写在 diff 里，写在 experience 围栏，执行时才插入。
    let art = format!("props/exp-{}.md", ctx.agent_id);
    let aid = crate::artifacts::deliver(db, ctx, &ctx.tiers, &art, &body, Some("改进提案"))
        .map_err(|e| PropError::Rejected(e.to_string()))?;
    proposals::submit(db, ctx, &aid, &body)
}

pub fn experience_payload(body: &str) -> Option<Value> {
    proposals::fenced(body, "experience").and_then(|raw| serde_json::from_str(&raw).ok())
}

/// Governance 01: a legacy proposal remains inspectable, but its prepared
/// whole-file payload is not an audited entry. Owner approval cannot supply
/// missing evidence. False rejection costs review; accepting it costs an
/// unreviewed write and can overwrite later lessons.
pub fn apply(
    ctx: &ToolContext,
    db: &Db,
    proposal_id: &str,
    body: &str,
    _backup: &Path,
) -> Result<bool, PropError> {
    if experience_payload(body).is_none() {
        return Ok(false);
    }
    storage::apply(db, ctx, proposal_id, body)
}

pub fn loading_view(
    db: &Db,
    ctx: &ToolContext,
    skill: &str,
    text: &str,
) -> Result<String, PropError> {
    storage::loading_view(db, ctx, skill, text)
}

/// Governance 01: restoring a legacy snapshot used to erase later experience
/// and revoke grants still in use. Preserve it for inspection, never replay it.
pub fn rollback(_db: &Db, ctx: &ToolContext, backup: &Path) -> Result<bool, PropError> {
    if !backup.join("experience.json").exists() {
        return Ok(false);
    }
    crate::diag::note(
        crate::diag::CLASS_REJECT,
        true,
        Some(&ctx.project_id),
        Some(&ctx.agent_id),
        ctx.stage_run_id.as_deref(),
        None,
        "experience_rollback",
        "ungoverned",
        std::time::Instant::now(),
    );
    Err(PropError::UngovernedExperience)
}

pub fn propose_entry(
    db: &Db,
    ctx: &ToolContext,
    request: &ExperienceRequest,
) -> Result<String, PropError> {
    governance::propose(db, ctx, request)
}

pub use history::{source_document, ExperienceSourceDocument, ExperienceSourceRequest};

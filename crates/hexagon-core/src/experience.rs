//! 经验（ADR 0069）。
//!
//! 复审通过之后，把同一段教训追加到技能「经验」节。读过的每一份都写。
//! 一份都没读、名单不空，写名单第一个。名单空才新建 `经验-<角色>`，
//! 授权和正文在同一份提案里，不走授权确认的自动放行。
//! 不进激活简报，不进用户全局。交付之后不再写。

use crate::db::Db;
use crate::proposals::{self, PropError};
use crate::tools::ToolContext;
use serde_json::{json, Value};
use std::path::Path;

pub const SECTION: &str = "## 经验";

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

struct Gate {
    role: String,
    skills: Vec<String>,
    reviewed: bool,
    delivered: bool,
}

fn gate(db: &Db, ctx: &ToolContext) -> Result<Gate, PropError> {
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
        let mut st = db.conn().prepare(
            "SELECT name FROM grants WHERE agent_id=?1 AND kind='skill' ORDER BY name",
        )?;
        skills = st
            .query_map([&ctx.agent_id], |r| r.get(0))?
            .collect::<Result<_, _>>()?;
    }
    let reviewed: i64 = db.conn().query_row(
        "SELECT COUNT(*) FROM events WHERE project_id=?1 AND agent_id=?2 AND kind='review_passed'",
        rusqlite::params![ctx.project_id, ctx.agent_id],
        |r| r.get(0),
    )?;
    // 改进提案自己的交付不是「工作交付」。否则提交经验就会把自己冻住。
    let delivered: i64 = db.conn().query_row(
        "SELECT COUNT(*) FROM events e
         WHERE e.project_id=?1 AND e.kind='artifact_delivered'
           AND COALESCE(json_extract(e.payload,'$.kind'), '') != '改进提案'
           AND e.id > COALESCE((
            SELECT MAX(id) FROM events
            WHERE project_id=?1 AND agent_id=?2 AND kind='review_passed'
         ), 0)",
        rusqlite::params![ctx.project_id, ctx.agent_id],
        |r| r.get(0),
    )?;
    Ok(Gate {
        role,
        skills,
        reviewed: reviewed > 0,
        delivered: delivered > 0,
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
    let gate = gate(db, ctx)?;
    if !gate.reviewed {
        crate::diag::note(
            "拒绝",
            true,
            Some(&ctx.project_id),
            Some(&ctx.agent_id),
            ctx.stage_run_id.as_deref(),
            "experience",
            "unreviewed",
            started,
        );
        return Err(PropError::Rejected(
            "unreviewed work cannot become experience".into(),
        ));
    }
    if gate.delivered {
        crate::diag::note(
            "拒绝",
            true,
            Some(&ctx.project_id),
            Some(&ctx.agent_id),
            ctx.stage_run_id.as_deref(),
            "experience",
            "delivered",
            started,
        );
        return Err(PropError::Rejected(
            "experience is frozen after delivery".into(),
        ));
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
            let old = std::fs::read_to_string(&path).unwrap_or_else(|_| {
                format!("---\nname: {name}\ndescription: 技能\n---\n\n")
            });
            let after = append_lesson(&old, lesson);
            if !after.contains(SECTION) || after.matches(SECTION).count() != old.matches(SECTION).count().max(1)
            {
                return Err(PropError::Rejected("experience section must be appended".into()));
            }
            if old.contains(SECTION) && !after.contains(lesson) {
                return Err(PropError::Rejected("experience section must be appended".into()));
            }
            let mut entry = json!({"path": rel, "after": after});
            if name.starts_with("经验-") {
                entry["grant"] = json!(name);
            }
            files.push(entry);
        }
    }
    let payload = json!({"lesson": lesson, "files": files, "create": creating});
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

/// 执行时写技能，并在新建时给这个 Agent 一行项目内授权。不调用授权确认。
pub fn apply(ctx: &ToolContext, db: &Db, body: &str, backup: &Path) -> Result<bool, PropError> {
    let Some(payload) = experience_payload(body) else {
        return Ok(false);
    };
    let Some(files) = payload["files"].as_array() else {
        return Err(PropError::Rejected("experience payload missing files".into()));
    };
    std::fs::create_dir_all(backup)?;
    let mut manifest = Vec::new();
    for (i, file) in files.iter().enumerate() {
        let rel = file["path"].as_str().unwrap_or("");
        if rel.contains("..") || !rel.starts_with(".hexagon/skills/") {
            return Err(PropError::Rejected("experience path escapes the project".into()));
        }
        let path = ctx.repo_root.join(rel);
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let slot = backup.join(format!("before-{i}"));
        if path.exists() {
            std::fs::copy(&path, &slot)?;
            manifest.push(json!({
                "path": rel, "had": true, "slot": i,
                "grant": file["grant"].as_str(),
            }));
        } else {
            manifest.push(json!({
                "path": rel, "had": false, "slot": i,
                "grant": file["grant"].as_str(),
            }));
        }
        std::fs::write(&path, file["after"].as_str().unwrap_or(""))?;
        if let Some(name) = file["grant"].as_str() {
            let id = format!("g{}", db.next_id("g")?);
            db.conn().execute(
                "INSERT OR IGNORE INTO grants (id, agent_id, kind, name) VALUES (?1,?2,'skill',?3)",
                rusqlite::params![id, ctx.agent_id, name],
            )?;
        }
    }
    std::fs::write(
        backup.join("experience.json"),
        serde_json::to_string(&manifest).unwrap_or_else(|_| "[]".into()),
    )?;
    Ok(true)
}

pub fn rollback(db: &Db, ctx: &ToolContext, backup: &Path) -> Result<bool, PropError> {
    let manifest_path = backup.join("experience.json");
    if !manifest_path.exists() {
        return Ok(false);
    }
    let manifest: Vec<Value> = serde_json::from_str(&std::fs::read_to_string(&manifest_path)?)
        .unwrap_or_default();
    for item in manifest {
        let rel = item["path"].as_str().unwrap_or("");
        let path = ctx.repo_root.join(rel);
        let slot = backup.join(format!("before-{}", item["slot"].as_u64().unwrap_or(0)));
        if item["had"] == true && slot.exists() {
            if let Some(parent) = path.parent() {
                std::fs::create_dir_all(parent)?;
            }
            std::fs::copy(&slot, &path)?;
        } else if path.exists() {
            std::fs::remove_file(&path).ok();
        }
        if let Some(name) = item["grant"].as_str() {
            db.conn().execute(
                "DELETE FROM grants WHERE agent_id=?1 AND kind='skill' AND name=?2",
                rusqlite::params![ctx.agent_id, name],
            )?;
        }
    }
    Ok(true)
}

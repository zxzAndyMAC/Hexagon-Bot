//! 产物管道：`.hexagon/` 下的可交接对象，随仓入库。
//!
//! 元数据头 = 文件首部的 `---` front matter（kind/stage/author/upstream/handoff）。
//! 三档校验：
//! - parse    （复审意见/测试记录/打回）：头部必须可解析且必填字段齐全；
//! - skeleton （规格/接口说明/技术裁定记录/改进提案）：parse + 必备 `## ` 节齐全；
//! - freeform （结构说明/界面稿/代码/自定义类型）：不查结构。
//!
//! 内置档位映射是代码常量不可降级；自定义类型默认 freeform，可挂更高档。
//!
//! 版本：同 path 新版本自动把旧版标 'superseded'。

use crate::db::Db;
use crate::tools::{repo_path, ToolContext, ToolError};
use crate::trace::{EventKind, TraceError};
use serde_json::json;
use std::collections::HashMap;

#[derive(Debug, thiserror::Error)]
pub enum ArtifactError {
    #[error("missing or unparseable metadata header")]
    MissingHeader,
    #[error("missing required field in header: {0}")]
    MissingField(&'static str),
    #[error("skeleton requires section: {0}")]
    MissingSection(&'static str),
    #[error(transparent)]
    Tool(#[from] ToolError),
    #[error(transparent)]
    Trace(#[from] TraceError),
    #[error(transparent)]
    Db(#[from] crate::db::DbError),
    #[error("sqlite: {0}")]
    Sqlite(#[from] rusqlite::Error),
    #[error("io: {0}")]
    Io(#[from] std::io::Error),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Tier {
    Parse,
    Skeleton,
    Freeform,
}

impl Tier {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Parse => "parse",
            Self::Skeleton => "skeleton",
            Self::Freeform => "freeform",
        }
    }
}

/// 内置 kind → 档位映射（常量，不可降级）。
fn builtin_tier(kind: &str) -> Option<Tier> {
    Some(match kind {
        "复审意见" | "测试记录" | "打回" | "改进提案" => Tier::Parse,
        "规格" | "接口说明" | "技术裁定记录" => Tier::Skeleton,
        "结构说明" | "界面稿" | "代码" => Tier::Freeform,
        _ => return None,
    })
}

/// 自定义类型可挂档：注册表覆盖默认 freeform；内置映射优先不可覆盖。
#[derive(Debug, Clone)]
pub struct TierMap {
    custom: HashMap<String, Tier>,
}
impl Default for TierMap {
    fn default() -> Self {
        Self::new()
    }
}
impl TierMap {
    pub fn new() -> Self {
        Self {
            custom: HashMap::new(),
        }
    }
    pub fn register(&mut self, kind: &str, tier: Tier) {
        if builtin_tier(kind).is_none() {
            self.custom.insert(kind.into(), tier);
        }
    }
    pub fn tier_for(&self, kind: &str) -> Tier {
        builtin_tier(kind)
            .unwrap_or_else(|| self.custom.get(kind).copied().unwrap_or(Tier::Freeform))
    }
}

/// skeleton 档各 kind 的必备 `## ` 节。
fn required_sections(kind: &str) -> &'static [&'static str] {
    match kind {
        "规格" => &["目标", "范围", "验收"],
        "接口说明" => &["资源", "端点", "错误码"],
        "技术裁定记录" => &["背景", "决定", "后果"],
        _ => &[],
    }
}

#[derive(Debug, Clone)]
pub struct ArtifactMeta {
    pub kind: String,
    pub stage: Option<String>,
    pub author: String,
    pub upstream: Option<String>,
    pub handoff: Option<String>,
    /// 头里的其余键（surface/target 等，提案等 parse 档消费）
    pub extra: std::collections::HashMap<String, String>,
}

/// 解析 front matter：`---\nkey: value\n...\n---\n<body>`。
pub fn parse_header(content: &str) -> Option<(ArtifactMeta, &str)> {
    let rest = content.strip_prefix("---\n")?;
    let end = rest.find("\n---")?;
    let (head, body) = (&rest[..end], &rest[end + 4..]);
    let mut m = std::collections::HashMap::new();
    for line in head.lines() {
        if let Some((k, v)) = line.split_once(':') {
            m.insert(k.trim().to_string(), v.trim().to_string());
        }
    }
    let known = ["kind", "stage", "author", "upstream", "handoff"];
    let extra: std::collections::HashMap<String, String> = m
        .iter()
        .filter(|(k, _)| !known.contains(&k.as_str()))
        .map(|(k, v)| (k.clone(), v.clone()))
        .collect();
    Some((
        ArtifactMeta {
            kind: m.get("kind").cloned().unwrap_or_default(),
            stage: m.get("stage").cloned(),
            author: m.get("author").cloned().unwrap_or_default(),
            upstream: m.get("upstream").cloned(),
            handoff: m.get("handoff").cloned(),
            extra,
        },
        body,
    ))
}

fn validate(
    content: &str,
    tier: Tier,
    fallback_author: &str,
) -> Result<ArtifactMeta, ArtifactError> {
    let parsed = parse_header(content);
    // freeform 不查结构：有头用头，没头给兜底 meta
    if tier == Tier::Freeform {
        return Ok(parsed.map(|(m, _)| m).unwrap_or(ArtifactMeta {
            kind: "misc".into(),
            stage: None,
            author: fallback_author.into(),
            upstream: None,
            handoff: None,
            extra: Default::default(),
        }));
    }
    let (meta, body) = parsed.ok_or(ArtifactError::MissingHeader)?;
    if meta.kind.is_empty() {
        return Err(ArtifactError::MissingField("kind"));
    }
    if meta.author.is_empty() {
        return Err(ArtifactError::MissingField("author"));
    }
    if tier == Tier::Skeleton {
        for sec in required_sections(&meta.kind) {
            let needle = format!("## {sec}");
            if !body.lines().any(|l| {
                l.trim_start() == needle || l.trim_start().starts_with(&format!("{needle} "))
            }) {
                return Err(ArtifactError::MissingSection(sec));
            }
        }
    }
    Ok(meta)
}

/// 交付产物：校验 → 写 `.hexagon/<path>` → 版本+取代 → 登记 → 交付事件。
/// `path` 是 `.hexagon/` 内相对路径。
pub fn deliver(
    db: &Db,
    ctx: &ToolContext,
    tiers: &TierMap,
    path: &str,
    content: &str,
    kind_hint: Option<&str>,
) -> Result<String, ArtifactError> {
    // 先定档位：头里的 kind 优先；没头看调用方声明的 kind_hint；都没有按 misc/freeform
    let declared_kind = parse_header(content)
        .map(|(m, _)| m.kind)
        .or_else(|| kind_hint.map(str::to_string))
        .unwrap_or_else(|| "misc".into());
    let tier = tiers.tier_for(&declared_kind);
    let meta = match validate(content, tier, &ctx.agent_id) {
        Ok(m) => m,
        Err(e) => {
            db.append_event(
                &ctx.project_id,
                EventKind::ArtifactRejected,
                json!({"path": path, "kind": declared_kind, "reason": e.to_string()}),
                Some(&ctx.agent_id),
                ctx.stage_run_id.as_deref(),
            )?;
            return Err(e);
        }
    };

    // 落盘（限仓内）
    let rel = format!(".hexagon/{}", path.trim_start_matches('/'));
    let p = repo_path(&ctx.repo_root, &rel)?;
    if let Some(parent) = p.parent() {
        std::fs::create_dir_all(parent)?;
    }
    std::fs::write(&p, content)?;

    // 版本 + 取代
    let version: i64 = db.conn().query_row(
        "SELECT COALESCE(MAX(version),0)+1 FROM artifacts WHERE project_id=?1 AND path=?2",
        rusqlite::params![ctx.project_id, path],
        |r| r.get(0),
    )?;
    db.conn().execute(
        "UPDATE artifacts SET status='superseded'
         WHERE project_id=?1 AND path=?2 AND status='valid'",
        rusqlite::params![ctx.project_id, path],
    )?;

    // 上游指针解析：path → 最新版行 id
    let upstream_id: Option<String> = meta.upstream.as_deref().and_then(|up| {
        db.conn()
            .query_row(
                "SELECT id FROM artifacts WHERE project_id=?1 AND path=?2
                 ORDER BY version DESC LIMIT 1",
                rusqlite::params![ctx.project_id, up],
                |r| r.get(0),
            )
            .ok()
    });

    let aid = format!("art{}", db.next_id("art")?);
    db.conn().execute(
        "INSERT INTO artifacts
         (id, project_id, path, kind, tier, stage_run_id, author_agent_id, version, status, upstream_id, content)
         VALUES (?1,?2,?3,?4,?5,?6,?7,?8,'valid',?9,?10)",
        rusqlite::params![
            aid,
            ctx.project_id,
            path,
            meta.kind,
            tier.as_str(),
            ctx.stage_run_id,
            ctx.agent_id,
            version,
            upstream_id,
            content
        ],
    )?;
    db.append_event(
        &ctx.project_id,
        EventKind::ArtifactDelivered,
        json!({"path": path, "kind": meta.kind, "tier": tier.as_str(), "version": version,
               "handoff": meta.handoff,
               // 票 10 taint：读过外部内容（research/mcp:*）后的产出打标——
               // 下游消费者看得出交付物来源纯度。
               "after_external": crate::provenance::tainted(db, &ctx.agent_id, ctx.stage_run_id.as_deref())}),
        Some(&ctx.agent_id),
        ctx.stage_run_id.as_deref(),
    )?;
    Ok(aid)
}

/// 产物浏览器查询：类型/阶段/状态/产出者可组合过滤。
/// 产物行（ADR 0054）：artifacts 读模型，IPC 直出。
#[derive(Debug, Clone, serde::Serialize, ts_rs::TS)]
#[ts(export, export_to = "../../../ui/src/gen/")]
pub struct ArtifactRow {
    pub id: String,
    pub path: String,
    pub kind: String,
    #[ts(type = "'parse' | 'skeleton' | 'freeform'")]
    // schema CHECK 词表钉死（migrations/*.sql / CardKind::as_str）
    pub tier: String,
    pub stage_run_id: Option<String>,
    pub author: Option<String>,
    #[ts(type = "number")] // JS number 域（wire 上是 JSON number，ts-rs 默认 bigint 不符 wire）
    pub version: i64,
    #[ts(type = "'valid' | 'superseded' | 'stamped' | 'pending'")]
    // schema CHECK 词表钉死（migrations/*.sql / CardKind::as_str）
    pub status: String,
    pub upstream_id: Option<String>,
}

pub fn query(
    db: &Db,
    project_id: &str,
    kind: Option<&str>,
    stage_run_id: Option<&str>,
    status: Option<&str>,
    author: Option<&str>,
) -> Result<Vec<ArtifactRow>, ArtifactError> {
    let mut st = db.conn().prepare(
        "SELECT id, path, kind, tier, stage_run_id, author_agent_id, version, status, upstream_id
         FROM artifacts WHERE project_id=?1
         AND (?2 IS NULL OR kind=?2) AND (?3 IS NULL OR stage_run_id=?3)
         AND (?4 IS NULL OR status=?4) AND (?5 IS NULL OR author_agent_id=?5)
         ORDER BY path, version",
    )?;
    let rows = st
        .query_map(
            rusqlite::params![project_id, kind, stage_run_id, status, author],
            |r| {
                Ok(ArtifactRow {
                    id: r.get(0)?,
                    path: r.get(1)?,
                    kind: r.get(2)?,
                    tier: r.get(3)?,
                    stage_run_id: r.get(4)?,
                    author: r.get(5)?,
                    version: r.get(6)?,
                    status: r.get(7)?,
                    upstream_id: r.get(8)?,
                })
            },
        )?
        .collect::<Result<Vec<_>, _>>()?;
    Ok(rows)
}

/// 产物正文（ADR 0052 读组）：优先读 DB 最新版（0003 起随行存），
/// 老行 content=NULL 回退读 `<root>/.hexagon/<path>` 盘上文件。
pub fn content(
    db: &Db,
    repo_root: &std::path::Path,
    project_id: &str,
    path: &str,
) -> Result<String, ArtifactError> {
    let c: Option<String> = db.conn().query_row(
        "SELECT content FROM artifacts WHERE project_id=?1 AND path=?2
         ORDER BY version DESC LIMIT 1",
        rusqlite::params![project_id, path],
        |r| r.get(0),
    )?;
    if let Some(s) = c {
        return Ok(s);
    }
    Ok(std::fs::read_to_string(
        repo_root.join(".hexagon").join(path),
    )?)
}

/// 指定版本内容（版本 diff / Agent 活动 tab 用）；版本不存在回 None。
/// 老行无 content：只有「最新版=盘上文件」这一条路。
pub fn content_at(
    db: &Db,
    repo_root: &std::path::Path,
    project_id: &str,
    path: &str,
    version: i64,
) -> Result<Option<String>, ArtifactError> {
    let c: Option<Option<String>> = db
        .conn()
        .query_row(
            "SELECT content FROM artifacts WHERE project_id=?1 AND path=?2 AND version=?3",
            rusqlite::params![project_id, path, version],
            |r| r.get(0),
        )
        .ok();
    match c {
        Some(Some(s)) => Ok(Some(s)),
        Some(None) if version == latest_version(db, project_id, path)? => Ok(Some(
            std::fs::read_to_string(repo_root.join(".hexagon").join(path))?,
        )),
        _ => Ok(None),
    }
}

fn latest_version(db: &Db, project_id: &str, path: &str) -> Result<i64, ArtifactError> {
    Ok(db.conn().query_row(
        "SELECT COALESCE(MAX(version),0) FROM artifacts WHERE project_id=?1 AND path=?2",
        rusqlite::params![project_id, path],
        |r| r.get(0),
    )?)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn setup() -> (Db, ToolContext, tempfile::TempDir) {
        let db = Db::open_in_memory().unwrap();
        db.conn()
            .execute(
                "INSERT INTO projects (id, dir, name, mode) VALUES ('p1','/tmp/x','x','pack')",
                [],
            )
            .unwrap();
        db.conn()
            .execute(
                "INSERT INTO agents (id, project_id, role, status) VALUES ('a1','p1','产品策划','active')",
                [],
            )
            .unwrap();
        let dir = tempfile::tempdir().unwrap();
        (
            db,
            ToolContext {
                project_id: "p1".into(),
                agent_id: "a1".into(),
                repo_root: dir.path().to_path_buf(),
                stage_run_id: None,
                owned_globs: vec![],
                tiers: TierMap::new(),
            },
            dir,
        )
    }

    const SPEC: &str = "---\nkind: 规格\nauthor: a1\nhandoff: 范围已收敛\n---\n## 目标\nx\n## 范围\nx\n## 验收\nx\n";

    #[test]
    fn deliver_writes_registers_and_events() {
        let (db, ctx, dir) = setup();
        let tiers = TierMap::new();
        let aid = deliver(&db, &ctx, &tiers, "specs/prd.md", SPEC, None).unwrap();
        assert!(dir.path().join(".hexagon/specs/prd.md").exists());
        let rows: Vec<serde_json::Value> = query(&db, "p1", Some("规格"), None, None, None)
            .unwrap()
            .iter()
            .map(|r| serde_json::to_value(r).unwrap())
            .collect();
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0]["id"], aid);
        assert_eq!(rows[0]["status"], "valid");
        let items = db
            .timeline("p1", None, 50, Some(&[EventKind::ArtifactDelivered]))
            .unwrap();
        assert_eq!(items[0].event.payload["handoff"], "范围已收敛");
    }

    #[test]
    fn missing_header_rejected_parse_tier() {
        let (db, ctx, _dir) = setup();
        let tiers = TierMap::new();
        let err = deliver(
            &db,
            &ctx,
            &tiers,
            "reviews/r1.md",
            "没有头的复审意见",
            Some("复审意见"),
        )
        .unwrap_err();
        assert!(matches!(err, ArtifactError::MissingHeader));
        let items = db
            .timeline("p1", None, 50, Some(&[EventKind::ArtifactRejected]))
            .unwrap();
        assert_eq!(items.len(), 1);
    }

    #[test]
    fn skeleton_missing_section_rejected() {
        let (db, ctx, _dir) = setup();
        let tiers = TierMap::new();
        let bad = "---\nkind: 规格\nauthor: a1\n---\n## 目标\nx\n"; // 缺 范围/验收
        let err = deliver(&db, &ctx, &tiers, "specs/prd.md", bad, None).unwrap_err();
        assert!(matches!(err, ArtifactError::MissingSection("范围")));
    }

    #[test]
    fn freeform_needs_no_header() {
        let (db, ctx, dir) = setup();
        let mut tiers = TierMap::new();
        tiers.register("草稿", Tier::Freeform);
        deliver(&db, &ctx, &tiers, "notes/draft.md", "随便写", None).unwrap();
        assert!(dir.path().join(".hexagon/notes/draft.md").exists());
    }

    #[test]
    fn builtin_tier_cannot_be_downgraded() {
        let mut tiers = TierMap::new();
        tiers.register("规格", Tier::Freeform); // 内置映射不可覆盖
        assert_eq!(tiers.tier_for("规格"), Tier::Skeleton);
    }

    #[test]
    fn new_version_supersedes_old() {
        let (db, ctx, _dir) = setup();
        let tiers = TierMap::new();
        deliver(&db, &ctx, &tiers, "specs/prd.md", SPEC, None).unwrap();
        let v2 = SPEC.replace("范围已收敛", "v2 修订");
        deliver(&db, &ctx, &tiers, "specs/prd.md", &v2, None).unwrap();
        let rows: Vec<serde_json::Value> = query(&db, "p1", None, None, None, None)
            .unwrap()
            .iter()
            .map(|r| serde_json::to_value(r).unwrap())
            .collect();
        assert_eq!(rows.len(), 2);
        assert_eq!(rows[0]["version"], 1);
        assert_eq!(rows[0]["status"], "superseded");
        assert_eq!(rows[1]["version"], 2);
        assert_eq!(rows[1]["status"], "valid");
    }

    #[test]
    fn upstream_pointer_resolves_to_id() {
        let (db, ctx, _dir) = setup();
        let tiers = TierMap::new();
        let up = deliver(
            &db,
            &ctx,
            &tiers,
            "ui/screens.md",
            "---\nkind: 界面稿\nauthor: a1\n---\nbody",
            None,
        )
        .unwrap();
        let api = "---\nkind: 接口说明\nauthor: a1\nupstream: ui/screens.md\n---\n## 资源\nx\n## 端点\nx\n## 错误码\nx\n";
        deliver(&db, &ctx, &tiers, "specs/api.md", api, None).unwrap();
        let rows: Vec<serde_json::Value> = query(&db, "p1", Some("接口说明"), None, None, None)
            .unwrap()
            .iter()
            .map(|r| serde_json::to_value(r).unwrap())
            .collect();
        assert_eq!(rows[0]["upstream_id"], up);
    }
}

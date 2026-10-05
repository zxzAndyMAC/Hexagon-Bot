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
use crate::tools::{ToolContext, ToolError};
pub(crate) mod delivery;
pub(crate) mod evidence;
pub(crate) mod fingerprint;
pub use evidence::ArtifactReview;
mod materialize;
use crate::trace::{EventKind, TraceError};
#[cfg(test)]
pub(crate) use materialize::with_fault;
pub(crate) use materialize::{receipt as materialization_receipt, recover};
use serde_json::json;
use std::collections::HashMap;

#[derive(Debug, thiserror::Error)]
pub enum ArtifactError {
    #[error("missing or unparseable metadata header")]
    MissingHeader,
    #[error("artifact kind parameter conflicts with the metadata header")]
    MetadataConflict,
    #[error("review target changed; read and review the current version again")]
    StaleReview,
    #[error("artifact materialization requires recovery; current content is not registered")]
    MaterializationPending,
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
    tiers: &TierMap,
    kind_hint: Option<&str>,
    fallback_author: &str,
) -> Result<(ArtifactMeta, Tier), ArtifactError> {
    let parsed = parse_header(content);
    let has_header = parsed.is_some();
    // Reliability 14 / D07: merge before choosing the validation tier. The old
    // flow chose the hinted tier but returned misc, so valid work never met due.
    let (mut meta, body) = parsed.unwrap_or((
        ArtifactMeta {
            kind: String::new(),
            stage: None,
            author: String::new(),
            upstream: None,
            handoff: None,
            extra: Default::default(),
        },
        content,
    ));
    // A false negative costs a corrected submission; a false positive writes
    // an unvalidated deliverable. Conflicting declarations therefore fail closed.
    let hint = kind_hint.map(str::trim).filter(|s| !s.is_empty());
    if let Some(hint) = hint {
        if !meta.kind.is_empty() && meta.kind != hint {
            return Err(ArtifactError::MetadataConflict);
        }
        meta.kind = hint.into();
    }
    if meta.kind.is_empty() {
        meta.kind = "misc".into();
    }
    let tier = tiers.tier_for(&meta.kind);
    if !has_header && (tier != Tier::Freeform || content.starts_with("---")) {
        return Err(ArtifactError::MissingHeader);
    }
    if tier == Tier::Freeform {
        if meta.author.is_empty() {
            meta.author = fallback_author.into();
        }
    } else if meta.author.is_empty() {
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
    Ok((meta, tier))
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
    let started = std::time::Instant::now();
    let (meta, tier) = match validate(content, tiers, kind_hint, &ctx.agent_id) {
        Ok(m) => m,
        Err(e) => {
            let trace = db.append_event(
                &ctx.project_id,
                EventKind::ArtifactRejected,
                json!({"path": path, "kind": kind_hint, "reason": e.to_string()}),
                Some(&ctx.agent_id),
                ctx.stage_run_id.as_deref(),
            )?;
            crate::diag::note(
                crate::diag::CLASS_REJECT,
                true,
                Some(&ctx.project_id),
                Some(&ctx.agent_id),
                ctx.stage_run_id.as_deref(),
                Some(&trace.to_string()),
                "artifact_metadata",
                &crate::errcode::ErrorCode::code(&e),
                started,
            );
            return Err(e);
        }
    };
    crate::diag::note(
        crate::diag::CLASS_JUDGE,
        false,
        Some(&ctx.project_id),
        Some(&ctx.agent_id),
        ctx.stage_run_id.as_deref(),
        None,
        "artifact_metadata",
        "merged_and_validated",
        started,
    );

    materialize::deliver(db, ctx, path, content, &meta, tier)
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
    /// Pending filesystem work is visible but cannot satisfy delivery gates.
    #[serde(skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub materialization: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub review: Option<ArtifactReview>,
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
        "WITH visible AS (
           SELECT a.id,a.project_id,a.path,a.kind,a.tier,a.stage_run_id,a.author_agent_id,a.version,a.status,a.upstream_id,
             CASE WHEN EXISTS(SELECT 1 FROM artifact_materializations m WHERE m.project_id=a.project_id AND m.path=a.path AND m.state='pending') THEN 'pending_recovery' END AS materialization
           FROM artifacts a
           UNION ALL
           SELECT id,project_id,path,json_extract(intent_json,'$.kind'),json_extract(intent_json,'$.tier'),
             json_extract(intent_json,'$.stage'),json_extract(intent_json,'$.author'),json_extract(intent_json,'$.version'),
             'pending',json_extract(intent_json,'$.upstream'),'pending_recovery'
           FROM artifact_materializations WHERE state='pending'
         )
         SELECT id,path,kind,tier,stage_run_id,author_agent_id,version,status,upstream_id,materialization
         FROM visible WHERE project_id=?1
         AND (?2 IS NULL OR kind=?2) AND (?3 IS NULL OR stage_run_id=?3)
         AND (?4 IS NULL OR status=?4) AND (?5 IS NULL OR author_agent_id=?5)
         ORDER BY path,version",
    )?;
    let mut rows = st
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
                    materialization: r.get(9)?,
                    review: None,
                })
            },
        )?
        .collect::<Result<Vec<_>, _>>()?;
    let root: String =
        db.conn()
            .query_row("SELECT dir FROM projects WHERE id=?1", [project_id], |r| {
                r.get(0)
            })?;
    for row in &mut rows {
        row.review = evidence::review_state(db, std::path::Path::new(&root), project_id, row)?;
    }
    Ok(rows)
}

/// Current worktree content (D07). Immutable snapshots belong exclusively to
/// content_at; a registered body must never impersonate an external edit.
pub fn content(
    db: &Db,
    repo_root: &std::path::Path,
    project_id: &str,
    path: &str,
) -> Result<String, ArtifactError> {
    let known: bool = db.conn().query_row(
        "SELECT EXISTS(SELECT 1 FROM artifacts WHERE project_id=?1 AND path=?2 UNION ALL SELECT 1 FROM artifact_materializations WHERE project_id=?1 AND path=?2 AND state='pending')",
        rusqlite::params![project_id,path],|r|r.get(0))?;
    if !known {
        return Err(rusqlite::Error::QueryReturnedNoRows.into());
    }
    let target = crate::tools::readable_repo_path(
        repo_root,
        &format!(".hexagon/{}", path.trim_start_matches('/')),
    )?;
    Ok(std::fs::read_to_string(target)?)
}

/// 指定版本内容（版本 diff / Agent 活动 tab 用）；版本不存在回 None。
/// Reliability 16: legacy NULL means no immutable evidence, including latest.
/// Never borrow the current worktree to fill a missing historical snapshot.
pub fn content_at(
    db: &Db,
    _repo_root: &std::path::Path,
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
        _ => Ok(None),
    }
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
                sessions: Default::default(),
                caps: Default::default(),
                ..Default::default()
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
    proptest::proptest! {
        #[test]
        fn metadata_conflict_cannot_select_a_weaker_validation_tier(
            header_kind in "[a-z]{1,20}", hint in "[a-z]{1,20}",
        ) {
            let content = format!("---\nkind: {header_kind}\n---\nbody");
            let result = validate(&content, &TierMap::new(), Some(&hint), "a0");
            if header_kind == hint {
                proptest::prop_assert_eq!(result.unwrap().0.kind, header_kind);
            } else {
                proptest::prop_assert!(matches!(result, Err(ArtifactError::MetadataConflict)));
            }
            for required in ["规格", "接口说明", "技术裁定记录", "测试记录", "复审意见", "改进提案"] {
                proptest::prop_assert!(matches!(validate("body", &TierMap::new(), Some(required), "a0"), Err(ArtifactError::MissingHeader)));
            }
        }
    }
}

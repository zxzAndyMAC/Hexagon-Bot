//! 核内 API 面：全项目唯一测试主接缝，也是 UI 与核的唯一边界。
//!
//! `Workbench` 把一个项目的 Db + 工具注册表 + 供应商槽 + 包副本捏在一起；
//! UI（Tauri command 层）和场景 DSL 都只过这层，没有旁路通道。

use crate::artifacts::{self, TierMap};
use crate::db::Db;
use crate::orchestra::{self, OrchError, PackDef};
use crate::provider::ModelProvider;
use crate::tools::{Registry, ToolContext};
use crate::trace::{Event, EventKind, MessageToken, TimelineItem, TraceError};
use crate::turn::{self, TurnOutcome};
use serde_json::{json, Value};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;

#[derive(Debug, thiserror::Error)]
pub enum ApiError {
    #[error(transparent)]
    Db(#[from] crate::db::DbError),
    #[error(transparent)]
    Trace(#[from] TraceError),
    #[error(transparent)]
    Orch(#[from] OrchError),
    #[error(transparent)]
    Turn(#[from] turn::TurnError),
    #[error(transparent)]
    Tool(#[from] crate::tools::ToolError),
    #[error(transparent)]
    Artifact(#[from] crate::artifacts::ArtifactError),
    #[error(transparent)]
    Publish(#[from] crate::publish::PublishError),
    #[error(transparent)]
    Autonomy(#[from] crate::autonomy::AutonomyError),
    #[error("sqlite: {0}")]
    Sqlite(#[from] rusqlite::Error),
    #[error("json: {0}")]
    Json(#[from] serde_json::Error),
    #[error("io: {0}")]
    Io(#[from] std::io::Error),
    #[error("agent role not found: {0}")]
    NoRole(String),
    #[error("no provider configured for slot: {0}")]
    NoProvider(String),
    #[error("no active stage")]
    NoStage,
}

/// 工作台实例：一个打开的项目。
pub struct Workbench {
    pub db: Db,
    pub registry: Registry,
    pub providers: HashMap<String, Arc<dyn ModelProvider>>,
    pub creds: Arc<dyn crate::credentials::CredentialStore>,
    pub project_id: String,
    pub repo_root: PathBuf,
    pub pack: Option<PackDef>,
}

impl Workbench {
    /// 打开/初始化项目：db 落 `<dir>/.hexagon/state.db`，钉住包副本。
    pub fn open(
        dir: impl AsRef<Path>,
        name: &str,
        roles: &[(String, String)], // (agent_id, role)
        pack: Option<PackDef>,
    ) -> Result<Self, ApiError> {
        let dir = dir.as_ref().to_path_buf();
        std::fs::create_dir_all(dir.join(".hexagon"))?;
        let db = Db::open(dir.join(".hexagon/state.db"))?;
        let project_id = "p1".to_string();
        db.conn().execute(
            "INSERT OR IGNORE INTO projects (id, dir, name, mode)
             VALUES ('p1', ?1, ?2, 'pack')",
            rusqlite::params![dir.to_string_lossy(), name],
        )?;
        for (aid, role) in roles {
            db.conn().execute(
                "INSERT OR IGNORE INTO agents (id, project_id, role) VALUES (?1,'p1',?2)",
                rusqlite::params![aid, role],
            )?;
        }
        if let Some(p) = &pack {
            p.pin(&dir)?;
        }
        Ok(Self {
            db,
            registry: Registry::builtin(),
            providers: HashMap::new(),
            creds: Arc::new(crate::credentials::OsKeychain),
            project_id,
            repo_root: dir,
            pack,
        })
    }

    /// 测试构造：内存库 + 临时仓。
    pub fn for_test(dir: &Path, roles: &[&str], pack: Option<PackDef>) -> Result<Self, ApiError> {
        let db = Db::open_in_memory()?;
        db.conn().execute(
            "INSERT INTO projects (id, dir, name, mode) VALUES ('p1',?1,'t','pack')",
            [dir.to_string_lossy().to_string()],
        )?;
        for (i, r) in roles.iter().enumerate() {
            db.conn().execute(
                "INSERT INTO agents (id, project_id, role) VALUES (?1,'p1',?2)",
                rusqlite::params![format!("a{i}"), r],
            )?;
        }
        if let Some(p) = &pack {
            p.pin(dir)?;
        }
        Ok(Self {
            db,
            registry: Registry::builtin(),
            providers: HashMap::new(),
            creds: Arc::new(crate::credentials::MemoryStore::default()),
            project_id: "p1".into(),
            repo_root: dir.to_path_buf(),
            pack,
        })
    }

    pub fn register_provider(&mut self, slot: &str, p: Arc<dyn ModelProvider>) {
        self.providers.insert(slot.into(), p);
    }

    /// 测试/桌面端注入凭据实现（默认内存库；生产壳换成 OsKeychain）。
    pub fn set_credential_store(&mut self, store: Arc<dyn crate::credentials::CredentialStore>) {
        self.creds = store;
    }

    /// 自治档位变更（L0/L1/L2），落 AutonomyChanged 事件。
    pub fn set_autonomy(&self, level: &str) -> Result<(), ApiError> {
        crate::autonomy::set_level(&self.db, &self.project_id, level)?;
        Ok(())
    }

    /// 当前自治档位。
    pub fn autonomy(&self) -> Result<String, ApiError> {
        Ok(crate::autonomy::level(&self.db, &self.project_id)?)
    }

    /// 负责人离开：打标记事件。
    pub fn owner_away(&self) -> Result<(), ApiError> {
        crate::autonomy::leave(&self.db, &self.project_id)?;
        Ok(())
    }

    /// 负责人归来：模板化摘要 + ReturnSummary 事件。
    pub fn owner_back(&self) -> Result<Value, ApiError> {
        Ok(crate::autonomy::back(&self.db, &self.project_id)?)
    }

    /// 远程发布：发起确认卡（kind='publish'）。
    pub fn request_publish(&self, remote: &str) -> Result<String, ApiError> {
        Ok(crate::publish::request(&self.db, &self.project_id, remote)?)
    }

    /// 确认发布：凭据闸 + push。
    pub fn confirm_publish(&self, qid: &str) -> Result<Value, ApiError> {
        Ok(crate::publish::confirm(
            &self.db,
            &self.project_id,
            qid,
            self.creds.as_ref(),
        )?)
    }

    /// 拒绝发布。
    pub fn reject_publish(&self, qid: &str) -> Result<(), ApiError> {
        Ok(crate::publish::reject(&self.db, &self.project_id, qid)?)
    }

    fn agent_by_role(&self, role: &str) -> Result<String, ApiError> {
        self.db
            .conn()
            .query_row(
                "SELECT id FROM agents WHERE project_id=?1 AND role=?2",
                rusqlite::params![self.project_id, role],
                |r| r.get(0),
            )
            .map_err(|_| ApiError::NoRole(role.into()))
    }

    fn ctx_for(&self, agent_id: &str, stage_run_id: Option<String>) -> ToolContext {
        ToolContext {
            project_id: self.project_id.clone(),
            agent_id: agent_id.into(),
            repo_root: self.repo_root.clone(),
            stage_run_id,
            owned_globs: vec![],
            tiers: TierMap::new(),
        }
    }

    // ---------- 命令 ----------

    /// 发消息：解析 @/# 成结构化 token。
    pub fn send_message(&self, body: &str) -> Result<i64, ApiError> {
        let tokens = parse_tokens(body);
        Ok(self
            .db
            .append_message(&self.project_id, "owner", body, &tokens, None, None)?)
    }

    /// 必问裁决。
    pub fn answer_permission(
        &self,
        question_id: &str,
        allow: bool,
        remember_shape: Option<&str>,
        scope: &str,
    ) -> Result<(), ApiError> {
        // payload 里有 agent_id
        let payload: String = self.db.conn().query_row(
            "SELECT agent_id FROM pending_questions WHERE id=?1",
            [question_id],
            |r| r.get(0),
        )?;
        let ctx = self.ctx_for(&payload, None);
        self.registry.resolve(
            &self.db,
            &ctx,
            question_id,
            allow,
            remember_shape,
            scope,
            self.pack.as_ref(),
        )?;
        Ok(())
    }

    /// 队列里第一个待决必问。
    pub fn first_pending_question(&self, kind: &str) -> Result<Option<String>, ApiError> {
        let mut st = self.db.conn().prepare(
            "SELECT id FROM pending_questions
             WHERE project_id=?1 AND kind=?2 AND state='queued' ORDER BY created_at LIMIT 1",
        )?;
        Ok(st
            .query_row(rusqlite::params![self.project_id, kind], |r| r.get(0))
            .ok())
    }

    /// 跑某角色一回合（若激活）。
    pub fn run_turn(&self, role: &str, input: &str) -> Result<TurnOutcome, ApiError> {
        let aid = self.agent_by_role(role)?;
        let run = self.active_run()?;
        let ctx = self.ctx_for(&aid, run.as_ref().map(|r| r.id.clone()));
        let slot: Option<String> =
            self.db
                .conn()
                .query_row("SELECT model_slot FROM agents WHERE id=?1", [&aid], |r| {
                    r.get(0)
                })?;
        let provider = self
            .providers
            .get(slot.as_deref().unwrap_or("default"))
            .or_else(|| self.providers.get("default"))
            .ok_or_else(|| ApiError::NoProvider(slot.unwrap_or_default()))?;
        Ok(turn::run_turn(
            &self.db,
            provider.as_ref(),
            &self.registry,
            &ctx,
            vec![],
            input,
        )?)
    }

    /// 当前激活阶段所有 active Agent 各跑一回合。
    pub fn run_all_active(&self, input: &str) -> Result<Vec<(String, TurnOutcome)>, ApiError> {
        let roles: Vec<String> = {
            let mut st = self
                .db
                .conn()
                .prepare("SELECT role FROM agents WHERE project_id=?1 AND status='active'")?;
            let rows = st
                .query_map([&self.project_id], |r| r.get(0))?
                .collect::<Result<_, _>>()?;
            rows
        };
        let mut out = Vec::new();
        for role in roles {
            out.push((role.clone(), self.run_turn(&role, input)?));
        }
        Ok(out)
    }

    pub fn open_stage(&self, seq: usize) -> Result<Value, ApiError> {
        let pack = self.pack()?;
        let (rid, skipped) = orchestra::open_stage(&self.db, &self.project_id, pack, seq)?;
        Ok(json!({"run_id": rid, "skipped": skipped}))
    }

    pub fn advance(&self) -> Result<Value, ApiError> {
        Ok(orchestra::advance(
            &self.db,
            &self.project_id,
            self.pack()?,
        )?)
    }
    pub fn run_checks(&self) -> Result<Value, ApiError> {
        let r = orchestra::run_checks(&self.db, &self.project_id, &self.repo_root, self.pack()?)?;
        Ok(json!({"results": r}))
    }
    pub fn stamp(&self) -> Result<Value, ApiError> {
        Ok(orchestra::stamp(&self.db, &self.project_id, self.pack()?)?)
    }
    pub fn rewind(&self, to_seq: usize) -> Result<Value, ApiError> {
        Ok(orchestra::rewind(
            &self.db,
            &self.project_id,
            self.pack()?,
            to_seq,
        )?)
    }
    pub fn skip(&self) -> Result<Value, ApiError> {
        Ok(orchestra::skip(&self.db, &self.project_id, self.pack()?)?)
    }
    pub fn pause(&self) -> Result<(), ApiError> {
        Ok(orchestra::pause(&self.db, &self.project_id)?)
    }
    pub fn resume(&self) -> Result<(), ApiError> {
        Ok(orchestra::resume(&self.db, &self.project_id)?)
    }

    /// 全员休眠。
    pub fn sleep_all(&self) -> Result<(), ApiError> {
        self.db.conn().execute(
            "UPDATE agents SET status='sleeping' WHERE project_id=?1",
            [&self.project_id],
        )?;
        self.db.append_event(
            &self.project_id,
            EventKind::TeamSlept,
            json!({"by": "owner"}),
            None,
            None,
        )?;
        Ok(())
    }

    fn pack(&self) -> Result<&PackDef, ApiError> {
        self.pack.as_ref().ok_or(ApiError::NoStage)
    }

    fn active_run(&self) -> Result<Option<orchestra::StageRun>, ApiError> {
        let mut st = self.db.conn().prepare(
            "SELECT id, seq, stage_name, state FROM stage_runs
             WHERE project_id=?1 AND state IN ('active','waiting_stamp')
             ORDER BY seq DESC LIMIT 1",
        )?;
        let mut rows = st.query_map([&self.project_id], |r| {
            Ok(orchestra::StageRun {
                id: r.get(0)?,
                seq: r.get(1)?,
                stage_name: r.get(2)?,
                state: r.get(3)?,
            })
        })?;
        Ok(rows.next().transpose()?)
    }

    // ---------- 查询 ----------

    pub fn timeline(
        &self,
        after: Option<i64>,
        limit: usize,
    ) -> Result<Vec<TimelineItem>, ApiError> {
        Ok(self.db.timeline(&self.project_id, after, limit, None)?)
    }

    pub fn artifacts(&self) -> Result<Vec<Value>, ApiError> {
        Ok(artifacts::query(
            &self.db,
            &self.project_id,
            None,
            None,
            None,
            None,
        )?)
    }

    pub fn artifact_content(&self, path: &str) -> Result<String, ApiError> {
        Ok(std::fs::read_to_string(
            self.repo_root.join(".hexagon").join(path),
        )?)
    }

    pub fn team(&self) -> Result<Vec<Value>, ApiError> {
        let mut st = self
            .db
            .conn()
            .prepare("SELECT id, role, model_slot, status FROM agents WHERE project_id=?1")?;
        let rows = st
            .query_map([&self.project_id], |r| {
                Ok(
                    json!({"id": r.get::<_,String>(0)?, "role": r.get::<_,String>(1)?,
                          "model_slot": r.get::<_,Option<String>>(2)?,
                          "status": r.get::<_,String>(3)?}),
                )
            })?
            .collect::<Result<Vec<_>, _>>()?;
        Ok(rows)
    }

    pub fn stage_status(&self) -> Result<Vec<Value>, ApiError> {
        let mut st = self.db.conn().prepare(
            "SELECT id, stage_name, seq, state FROM stage_runs WHERE project_id=?1 ORDER BY seq, id",
        )?;
        let rows = st
            .query_map([&self.project_id], |r| {
                Ok(
                    json!({"run_id": r.get::<_,String>(0)?, "stage": r.get::<_,String>(1)?,
                          "seq": r.get::<_,i64>(2)?, "state": r.get::<_,String>(3)?}),
                )
            })?
            .collect::<Result<Vec<_>, _>>()?;
        Ok(rows)
    }

    pub fn pending_questions(&self) -> Result<Vec<Value>, ApiError> {
        let mut st = self.db.conn().prepare(
            "SELECT id, kind, payload, state FROM pending_questions WHERE project_id=?1",
        )?;
        let rows = st
            .query_map([&self.project_id], |r| {
                Ok(
                    json!({"id": r.get::<_,String>(0)?, "kind": r.get::<_,String>(1)?,
                          "payload": r.get::<_,String>(2)?, "state": r.get::<_,String>(3)?}),
                )
            })?
            .collect::<Result<Vec<_>, _>>()?;
        Ok(rows)
    }

    /// 用量：账本多维汇总 + 项目总计/上限。
    pub fn usage(&self) -> Result<Vec<Value>, ApiError> {
        let mut rows = crate::usage::summarize(&self.db, &self.project_id)?;
        let limit: Option<i64> = self.db.conn().query_row(
            "SELECT usage_limit_cents FROM projects WHERE id=?1",
            [&self.project_id],
            |r| r.get(0),
        )?;
        rows.push(json!({
            "_total": true,
            "spent_mc": crate::usage::spent_mc(&self.db, &self.project_id)?,
            "limit_cents": limit,
        }));
        Ok(rows)
    }

    /// 事件断言原料：DSL/验收套件直接消费。
    pub fn events(&self, kinds: Option<&[EventKind]>) -> Result<Vec<Event>, ApiError> {
        Ok(self
            .db
            .timeline(&self.project_id, None, 10000, kinds)?
            .into_iter()
            .map(|i| i.event)
            .collect())
    }
}

/// composer 的 @/# 解析：[@名字]→mention，[#路径]→path 指针。
pub fn parse_tokens(body: &str) -> Vec<MessageToken> {
    let mut out = Vec::new();
    for word in body.split_whitespace() {
        if let Some(name) = word.strip_prefix('@') {
            if !name.is_empty() {
                out.push(MessageToken::Mention {
                    agent_role: name.to_string(),
                });
            }
        } else if let Some(p) = word.strip_prefix('#') {
            if !p.is_empty() {
                out.push(MessageToken::PathRef {
                    path: p.to_string(),
                });
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::provider::ScriptedProvider;
    use crate::turn::{text_response, tool_response};

    #[test]
    fn token_parsing() {
        let t = parse_tokens("继续 @后端 参考 #src/main.rs 谢谢");
        assert_eq!(
            t,
            vec![
                MessageToken::Mention {
                    agent_role: "后端".into()
                },
                MessageToken::PathRef {
                    path: "src/main.rs".into()
                }
            ]
        );
    }

    #[test]
    fn end_to_end_open_project_to_timeline() {
        let dir = tempfile::tempdir().unwrap();
        let pack: PackDef = serde_json::from_value(json!({
            "name":"t","version":1,
            "stages":[{"name":"规格","roles":["产品策划"],"due":["规格"]}]
        }))
        .unwrap();
        let mut wb = Workbench::for_test(dir.path(), &["产品策划"], Some(pack)).unwrap();
        wb.register_provider(
            "default",
            Arc::new(ScriptedProvider::new(vec![
                tool_response(vec![(
                    "t1",
                    "artifact_write",
                    json!({"path":"specs/prd.md","content":"---\nkind: 规格\nauthor: a0\n---\n## 目标\nx\n## 范围\nx\n## 验收\nx"}),
                )]),
                text_response("done"),
            ])),
        );
        wb.send_message("开工 @产品策划").unwrap();
        wb.open_stage(0).unwrap();
        wb.run_turn("产品策划", "写规格").unwrap();
        let r = wb.advance().unwrap();
        assert_eq!(r["action"], "pack_finished");
        // 产物 + 时间线可读
        assert!(dir.path().join(".hexagon/specs/prd.md").exists());
        let tl = wb.timeline(None, 50).unwrap();
        assert!(tl
            .iter()
            .any(|i| i.event.kind == EventKind::ArtifactDelivered));
        assert!(tl.iter().any(|i| i.message.is_some()));
        let arts = wb.artifacts().unwrap();
        assert_eq!(arts.len(), 1);
    }
}

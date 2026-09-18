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
    #[error(transparent)]
    Proposal(#[from] crate::proposals::PropError),
    #[error(transparent)]
    Review(#[from] crate::review::ReviewError),
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
    #[error("stage run interrupted; recover first: {0}")]
    Interrupted(String),
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
        // 票 37：检出上次被杀留下的中断回合（无收束的 turn_started）。
        let interrupted = orchestra::detect_interrupted(&db, &project_id)?;
        if interrupted > 0 {
            log::warn!("recovered {interrupted} interrupted run(s)");
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

    /// 提案队列。
    pub fn proposals(&self) -> Result<Vec<Value>, ApiError> {
        Ok(crate::proposals::list(&self.db, &self.project_id)?)
    }

    /// 上级复审提案：pass → 进盖章卡；reject → 带原因驳回。
    pub fn review_proposal(
        &self,
        proposal_id: &str,
        pass: bool,
        reason: &str,
        reviewer_agent: &str,
    ) -> Result<(), ApiError> {
        let ctx = self.ctx_for(reviewer_agent, None);
        crate::proposals::review(&self.db, &ctx, proposal_id, pass, reason)?;
        Ok(())
    }

    /// 提案盖章确认：快照 + 应用 + active。
    pub fn confirm_proposal(&self, qid: &str) -> Result<String, ApiError> {
        let ctx = self.ctx_for("owner", None);
        Ok(crate::proposals::activate(&self.db, &ctx, qid)?)
    }

    /// 提案回滚。
    pub fn rollback_proposal(&self, proposal_id: &str) -> Result<(), ApiError> {
        let ctx = self.ctx_for("owner", None);
        crate::proposals::rollback(&self.db, &ctx, proposal_id)?;
        Ok(())
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

    /// 提案盖章驳回：qid → proposal_id → rejected + 原因。
    pub fn reject_proposal(&self, qid: &str, reason: &str) -> Result<(), ApiError> {
        let ctx = self.ctx_for("owner", None);
        crate::proposals::reject_at_stamp(&self.db, &ctx, qid, reason)?;
        Ok(())
    }

    /// 升级卡裁决：payload 取 flag_id → review::adjudicate_flag；标记问题已答。
    pub fn adjudicate_flag(&self, qid: &str, agree: bool) -> Result<Value, ApiError> {
        let payload: String = self.db.conn().query_row(
            "SELECT payload FROM pending_questions
             WHERE id=?1 AND project_id=?2 AND kind='escalation' AND state='queued'",
            rusqlite::params![qid, self.project_id],
            |r| r.get(0),
        )?;
        let flag_id = serde_json::from_str::<Value>(&payload)?["flag_id"]
            .as_str()
            .unwrap_or_default()
            .to_string();
        let ctx = self.ctx_for("owner", None);
        let v = crate::review::adjudicate_flag(&self.db, &ctx, self.pack()?, &flag_id, agree)?;
        self.db.conn().execute(
            "UPDATE pending_questions SET state='answered' WHERE id=?1",
            [qid],
        )?;
        Ok(v)
    }

    /// 设置 agent 头像：UI 传 data URL（data:image/png;base64,…），
    /// 落盘 `<root>/.hexagon/avatars/<agent>.<ext>`，换扩展名时清旧文件。
    pub fn set_agent_avatar(&self, agent_id: &str, data_url: &str) -> Result<(), ApiError> {
        use base64::Engine;
        let (mime, b64) = data_url
            .strip_prefix("data:")
            .and_then(|s| s.split_once(";base64,"))
            .ok_or_else(|| {
                ApiError::Io(std::io::Error::new(
                    std::io::ErrorKind::InvalidInput,
                    "expected data:<mime>;base64,<payload>",
                ))
            })?;
        let ext = match mime {
            "image/jpeg" | "image/jpg" => "jpg",
            "image/webp" => "webp",
            "image/gif" => "gif",
            _ => "png",
        };
        let bytes = base64::engine::general_purpose::STANDARD
            .decode(b64)
            .map_err(|e| ApiError::Io(std::io::Error::new(std::io::ErrorKind::InvalidInput, e)))?;
        if bytes.len() > 2 * 1024 * 1024 {
            return Err(ApiError::Io(std::io::Error::new(
                std::io::ErrorKind::InvalidInput,
                "avatar >2MB",
            )));
        }
        let dir = self.repo_root.join(".hexagon/avatars");
        std::fs::create_dir_all(&dir)?;
        for e in ["png", "jpg", "webp", "gif"] {
            let p = dir.join(format!("{agent_id}.{e}"));
            if e != ext && p.exists() {
                std::fs::remove_file(p)?;
            }
        }
        std::fs::write(dir.join(format!("{agent_id}.{ext}")), bytes)?;
        Ok(())
    }

    /// 读头像 → data URL（前端直接 <img src>）；未设置回 None。
    pub fn agent_avatar(&self, agent_id: &str) -> Result<Option<String>, ApiError> {
        use base64::Engine;
        let dir = self.repo_root.join(".hexagon/avatars");
        for (ext, mime) in [
            ("png", "image/png"),
            ("jpg", "image/jpeg"),
            ("webp", "image/webp"),
            ("gif", "image/gif"),
        ] {
            let p = dir.join(format!("{agent_id}.{ext}"));
            if p.exists() {
                let b = std::fs::read(&p)?;
                return Ok(Some(format!(
                    "data:{mime};base64,{}",
                    base64::engine::general_purpose::STANDARD.encode(b)
                )));
            }
        }
        Ok(None)
    }

    /// 盖章点驳回：退上一阶段（与打回不同通道）。
    pub fn reject_stamp(&self) -> Result<Value, ApiError> {
        Ok(crate::review::reject_stamp(
            &self.db,
            &self.project_id,
            self.pack()?,
        )?)
    }

    pub(crate) fn agent_by_role(&self, role: &str) -> Result<String, ApiError> {
        self.db
            .conn()
            .query_row(
                "SELECT id FROM agents WHERE project_id=?1 AND role=?2",
                rusqlite::params![self.project_id, role],
                |r| r.get(0),
            )
            .map_err(|_| ApiError::NoRole(role.into()))
    }

    pub(crate) fn ctx_for(&self, agent_id: &str, stage_run_id: Option<String>) -> ToolContext {
        ToolContext {
            project_id: self.project_id.clone(),
            agent_id: agent_id.into(),
            repo_root: self.repo_root.clone(),
            stage_run_id,
            owned_globs: crate::permissions::agent_globs(&self.db, agent_id).unwrap_or_default(),
            tiers: TierMap::new(),
        }
    }

    // ---------- 命令 ----------

    /// 发消息：解析 @/# 成结构化 token；文本指令与按钮走同一命令通道。
    /// 「退回[N]/回退[N]」「跳过」「盖章」「暂停」「休眠/全员休眠」「恢复」
    /// 命中则分发到对应命令（事件形状与按钮完全一致），消息本体照常入档。
    pub fn send_message(&self, body: &str) -> Result<i64, ApiError> {
        let tokens = parse_tokens(body);
        let id = self
            .db
            .append_message(&self.project_id, "owner", body, &tokens, None, None)?;
        if let Some(cmd) = parse_command(body) {
            log::info!("owner text command: {cmd:?}");
            // 命令失败不吞消息——记日志，消息已入档
            if let Err(e) = self.dispatch_command(&cmd) {
                log::warn!("text command {cmd:?} failed: {e}");
            }
        }
        Ok(id)
    }

    fn dispatch_command(&self, cmd: &TextCommand) -> Result<(), ApiError> {
        match cmd {
            TextCommand::Rewind(to) => {
                let to_seq = match to {
                    Some(s) => *s,
                    None => {
                        // 默认退回上一阶段
                        let seq: i64 = self.db.conn().query_row(
                            "SELECT seq FROM stage_runs WHERE project_id=?1 AND state='active'",
                            [&self.project_id],
                            |r| r.get(0),
                        )?;
                        (seq - 1).max(0) as usize
                    }
                };
                self.rewind(to_seq)?;
            }
            TextCommand::Skip => {
                self.skip()?;
            }
            TextCommand::Stamp => {
                self.stamp()?;
            }
            TextCommand::Pause => self.pause()?,
            TextCommand::Resume => self.resume()?,
            TextCommand::SleepAll => self.sleep_all()?,
            TextCommand::Override(reason) => {
                self.override_checks(reason)?;
            }
        }
        Ok(())
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

    /// 跑某角色一回合（若激活）。有 interrupted run 未恢复时硬挡（票 37 恢复闸）。
    pub fn run_turn(&self, role: &str, input: &str) -> Result<TurnOutcome, ApiError> {
        self.run_turn_opts(role, input, false)
    }

    /// plan_first=true 时回合先发不阻塞方案消息再进工具循环（US15 快速通道）。
    fn run_turn_opts(
        &self,
        role: &str,
        input: &str,
        plan_first: bool,
    ) -> Result<TurnOutcome, ApiError> {
        if let Ok(rid) = self.db.conn().query_row(
            "SELECT id FROM stage_runs
             WHERE project_id=?1 AND state='interrupted' LIMIT 1",
            [&self.project_id],
            |r| r.get::<_, String>(0),
        ) {
            return Err(ApiError::Interrupted(rid));
        }
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
        let run = if plan_first {
            turn::run_turn_planned(
                &self.db,
                provider.as_ref(),
                &self.registry,
                &ctx,
                vec![],
                input,
            )
        } else {
            turn::run_turn(
                &self.db,
                provider.as_ref(),
                &self.registry,
                &ctx,
                vec![],
                input,
            )
        };
        Ok(run?)
    }

    /// 快速通道派发（票 26）：负责人直接把任务派给一个已勾选角色，
    /// 不走阶段——产物/事件落同一 .hexagon/，stage_runs 不产生行。
    /// 休眠角色被点名即唤醒（会诊唤醒同款语义）；未勾选角色 → NoRole。
    pub fn dispatch(&self, role: &str, input: &str) -> Result<TurnOutcome, ApiError> {
        let aid = self.agent_by_role(role)?;
        let status: String =
            self.db
                .conn()
                .query_row("SELECT status FROM agents WHERE id=?1", [&aid], |r| {
                    r.get(0)
                })?;
        if status == "sleeping" {
            self.db
                .conn()
                .execute("UPDATE agents SET status='active' WHERE id=?1", [&aid])?;
            self.db.append_event(
                &self.project_id,
                EventKind::AgentActivated,
                json!({"by": "dispatch"}),
                Some(&aid),
                None,
            )?;
        }
        self.db.append_event(
            &self.project_id,
            EventKind::FastpathDispatched,
            json!({"role": role}),
            Some(&aid),
            None,
        )?;
        // US15：动手前先发不阻塞方案消息，负责人有打断窗口
        self.run_turn_opts(role, input, true)
    }

    /// 升级为流程包（票 26）：不换目录——钉包副本 + mode='pack' + PackUpgraded 事件。
    /// 已在 pack 模式视为换包重钉，同样允许。
    pub fn upgrade_to_pack(&mut self, pack: PackDef) -> Result<(), ApiError> {
        pack.pin(&self.repo_root)?;
        self.db.conn().execute(
            "UPDATE projects SET mode='pack', pack_name=?1, pack_copy_version=?2 WHERE id=?3",
            rusqlite::params![pack.name, pack.version as i64, self.project_id],
        )?;
        self.db.append_event(
            &self.project_id,
            EventKind::PackUpgraded,
            json!({"pack": pack.name, "version": pack.version}),
            None,
            None,
        )?;
        self.pack = Some(pack);
        Ok(())
    }

    /// 项目标识：mode / pack_name / 快速通道角色（UI 与测试用）。
    pub fn project_info(&self) -> Result<Value, ApiError> {
        let (name, mode, pack_name, fast_aid): (String, String, Option<String>, Option<String>) =
            self.db.conn().query_row(
                "SELECT name, mode, pack_name, fastpath_agent_id FROM projects WHERE id=?1",
                [&self.project_id],
                |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)),
            )?;
        let fast_role: Option<String> = fast_aid.as_ref().and_then(|a| {
            self.db
                .conn()
                .query_row("SELECT role FROM agents WHERE id=?1", [a], |r| r.get(0))
                .ok()
        });
        Ok(json!({
            "name": name,
            "mode": mode,
            "pack_name": pack_name,
            "fastpath_agent_id": fast_aid,
            "fastpath_role": fast_role,
        }))
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

    /// 跳过某次声明复审（票 33 / US26）：留 review_skipped 事件，
    /// 阶段评估视为该复审满足。已通过的复审不可跳——章后无后门。
    pub fn skip_review(&self, artifact_kind: &str) -> Result<(), ApiError> {
        let run = self.active_run()?.ok_or(ApiError::NoStage)?;
        let pack = self.pack()?;
        let stage = pack.stages.get(run.seq as usize).ok_or(ApiError::NoStage)?;
        let decl = stage
            .reviews
            .iter()
            .find(|r| r.artifact_kind == artifact_kind)
            .ok_or_else(|| ApiError::NoRole(format!("no declared review for {artifact_kind}")))?;
        let latest: Option<String> = self
            .db
            .conn()
            .query_row(
                "SELECT kind FROM events
                 WHERE project_id=?1 AND stage_run_id=?2
                 AND kind IN ('review_passed','review_rejected','review_skipped')
                 AND json_extract(payload,'$.artifact_kind')=?3
                 ORDER BY id DESC LIMIT 1",
                rusqlite::params![self.project_id, run.id, artifact_kind],
                |r| r.get(0),
            )
            .ok();
        if latest.as_deref() == Some("review_passed") {
            return Err(ApiError::NoRole(format!(
                "review for {artifact_kind} already passed"
            )));
        }
        self.db.append_event(
            &self.project_id,
            EventKind::ReviewSkipped,
            json!({"artifact_kind": artifact_kind, "reviewer": decl.reviewer,
                   "stage_run_id": run.id, "by": "owner"}),
            None,
            Some(&run.id),
        )?;
        Ok(())
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
    /// 恢复中断的阶段 run（票 37）：负责人按「继续」才重激活，不重放模型调用。
    pub fn recover_run(&self, run_id: &str) -> Result<(), ApiError> {
        orchestra::recover_run(&self.db, &self.project_id, run_id)?;
        Ok(())
    }

    /// 显式覆盖检验失败（票 40）：留痕 check_overridden（谁/哪些命令/理由）。
    pub fn override_checks(&self, reason: &str) -> Result<Value, ApiError> {
        Ok(orchestra::override_checks(
            &self.db,
            &self.project_id,
            self.pack()?,
            reason,
        )?)
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

    /// 单个 Agent 休眠/唤醒（Agent tab 头卡用）。
    pub fn set_agent_sleeping(&self, agent_id: &str, sleeping: bool) -> Result<(), ApiError> {
        self.db.conn().execute(
            "UPDATE agents SET status=?1 WHERE id=?2 AND project_id=?3",
            rusqlite::params![
                if sleeping { "sleeping" } else { "active" },
                agent_id,
                self.project_id
            ],
        )?;
        self.db.append_event(
            &self.project_id,
            if sleeping {
                EventKind::AgentSlept
            } else {
                EventKind::AgentActivated
            },
            json!({"by": "owner"}),
            Some(agent_id),
            None,
        )?;
        Ok(())
    }

    fn pack(&self) -> Result<&PackDef, ApiError> {
        self.pack.as_ref().ok_or(ApiError::NoStage)
    }

    pub(crate) fn active_run(&self) -> Result<Option<orchestra::StageRun>, ApiError> {
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

    /// 轨迹导出（US54）：过滤后事件集写 JSON 文件供回放核对，返回条数。
    /// 提示词全文从不落库，导出天然不含提示词/凭据正文。
    pub fn export_events(
        &self,
        dest: &std::path::Path,
        filter: &crate::trace::ExportFilter,
    ) -> Result<usize, ApiError> {
        Ok(self.db.export_events(&self.project_id, dest, filter)?)
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
        // 优先读 DB 最新版内容（0003 起随行存）；老行 content=NULL 回退读盘
        let c: Option<String> = self.db.conn().query_row(
            "SELECT content FROM artifacts WHERE project_id=?1 AND path=?2
             ORDER BY version DESC LIMIT 1",
            rusqlite::params![self.project_id, path],
            |r| r.get(0),
        )?;
        if let Some(s) = c {
            return Ok(s);
        }
        Ok(std::fs::read_to_string(
            self.repo_root.join(".hexagon").join(path),
        )?)
    }

    /// 指定版本内容（版本 diff / Agent 活动 tab 用）；版本不存在回 None。
    pub fn artifact_content_at(
        &self,
        path: &str,
        version: i64,
    ) -> Result<Option<String>, ApiError> {
        let c: Option<Option<String>> = self
            .db
            .conn()
            .query_row(
                "SELECT content FROM artifacts WHERE project_id=?1 AND path=?2 AND version=?3",
                rusqlite::params![self.project_id, path, version],
                |r| r.get(0),
            )
            .ok();
        match c {
            Some(Some(s)) => Ok(Some(s)),
            // 老行无 content：只有「最新版=盘上文件」这一条路
            Some(None) if version == self.latest_artifact_version(path)? => Ok(Some(
                std::fs::read_to_string(self.repo_root.join(".hexagon").join(path))?,
            )),
            _ => Ok(None),
        }
    }

    fn latest_artifact_version(&self, path: &str) -> Result<i64, ApiError> {
        Ok(self.db.conn().query_row(
            "SELECT COALESCE(MAX(version),0) FROM artifacts WHERE project_id=?1 AND path=?2",
            rusqlite::params![self.project_id, path],
            |r| r.get(0),
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
            "SELECT id, kind, payload, state FROM pending_questions
             WHERE project_id=?1 AND state='queued' ORDER BY created_at",
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
        let tokens: i64 = self.db.conn().query_row(
            "SELECT COALESCE(SUM(prompt_tokens+completion_tokens+tool_output_tokens),0)
             FROM usage WHERE project_id=?1",
            [&self.project_id],
            |r| r.get(0),
        )?;
        rows.push(json!({
            "_total": true,
            "spent_mc": crate::usage::spent_mc(&self.db, &self.project_id)?,
            "limit_cents": limit,
            "tokens": tokens,
        }));
        Ok(rows)
    }

    /// 设/清项目用量上限（分）；None = 不限。
    pub fn set_usage_limit(&self, limit_cents: Option<i64>) -> Result<(), ApiError> {
        self.db.conn().execute(
            "UPDATE projects SET usage_limit_cents=?1 WHERE id=?2",
            rusqlite::params![limit_cents, self.project_id],
        )?;
        Ok(())
    }

    /// 用量时间序列：按 bucket × Agent 分组，带三类 token 与成本。
    /// `granularity`: "day"（YYYY-MM-DD）| "hour"（YYYY-MM-DD HH:00）。
    /// `from`/`to`：日期串 YYYY-MM-DD（含当天，`to` 含整日）；None = 不限。
    pub fn usage_series(
        &self,
        granularity: &str,
        from: Option<&str>,
        to: Option<&str>,
    ) -> Result<Vec<Value>, ApiError> {
        let bucket = if granularity == "hour" {
            "strftime('%Y-%m-%d %H:00', u.created_at)"
        } else {
            "date(u.created_at)"
        };
        let sql = format!(
            "SELECT {bucket}, u.agent_id,
                    SUM(u.prompt_tokens), SUM(u.completion_tokens),
                    SUM(u.tool_output_tokens), SUM(u.cost_millicents)
             FROM usage u
             WHERE u.project_id=?1
               AND (?2 IS NULL OR u.created_at >= ?2)
               AND (?3 IS NULL OR u.created_at < date(?3, '+1 day'))
             GROUP BY 1, u.agent_id ORDER BY 1"
        );
        let mut st = self.db.conn().prepare(&sql)?;
        let rows = st
            .query_map(rusqlite::params![self.project_id, from, to], |r| {
                Ok(json!({
                    "bucket": r.get::<_, String>(0)?,
                    "agent_id": r.get::<_, Option<String>>(1)?,
                    "prompt_tokens": r.get::<_, i64>(2)?,
                    "completion_tokens": r.get::<_, i64>(3)?,
                    "tool_output_tokens": r.get::<_, i64>(4)?,
                    "cost_mc": r.get::<_, i64>(5)?,
                }))
            })?
            .collect::<Result<Vec<_>, _>>()?;
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

/// 文本指令（与按钮同权同痕）：只认整句命令，防普通语句被劫持。
#[derive(Debug, PartialEq)]
enum TextCommand {
    Rewind(Option<usize>),
    Skip,
    Stamp,
    Pause,
    Resume,
    SleepAll,
    Override(String),
}

/// 整句匹配：命令词 + 可选数字参数，多余文本一律不算命令。
/// 双轨（ADR 0051）：`/verb` 规范式全语言通用；本地化斜杠别名（/退回 /盖章）也收；
/// 裸词（退回/盖章…）保留为过渡形态。
fn parse_command(body: &str) -> Option<TextCommand> {
    let b = body.trim();
    // /verb 规范式 + 本地化斜杠别名（退回参数走同一数字尾巴规则）
    if let Some(rest) = b.strip_prefix('/') {
        let (verb, arg) = match rest.split_once(' ') {
            Some((v, a)) => (v, a.trim()),
            None => (rest, ""),
        };
        return match verb {
            "rewind" | "退回" | "回退" => {
                if arg.is_empty() {
                    Some(TextCommand::Rewind(None))
                } else {
                    arg.trim_start_matches('到')
                        .parse::<usize>()
                        .ok()
                        .map(|n| TextCommand::Rewind(Some(n)))
                }
            }
            "skip" | "跳过" if arg.is_empty() => Some(TextCommand::Skip),
            "stamp" | "盖章" | "通过" if arg.is_empty() => Some(TextCommand::Stamp),
            "pause" | "暂停" if arg.is_empty() => Some(TextCommand::Pause),
            "resume" | "恢复" if arg.is_empty() => Some(TextCommand::Resume),
            "sleep" | "sleep_all" | "休眠" | "全员休眠" if arg.is_empty() => {
                Some(TextCommand::SleepAll)
            }
            // /override <理由>：理由必填，空理由不算指令（落普通消息）
            "override" | "覆盖" if !arg.is_empty() => {
                Some(TextCommand::Override(arg.to_string()))
            }
            _ => None, // 未知 /verb 或多余参数 → 普通消息，不吞
        };
    }
    match b {
        "跳过" => return Some(TextCommand::Skip),
        "盖章" | "通过" => return Some(TextCommand::Stamp),
        "暂停" => return Some(TextCommand::Pause),
        "恢复" => return Some(TextCommand::Resume),
        "休眠" | "全员休眠" => return Some(TextCommand::SleepAll),
        "退回" | "回退" | "退回上一阶段" | "回退上一阶段" => {
            return Some(TextCommand::Rewind(None))
        }
        _ => {}
    }
    // 「退回 2」「回退到 1」：前缀 + 纯数字尾巴才算命令，其他尾巴不劫持
    for prefix in ["退回", "回退"] {
        if let Some(rest) = b.strip_prefix(prefix) {
            let rest = rest.trim().trim_start_matches('到').trim();
            if let Ok(n) = rest.parse::<usize>() {
                return Some(TextCommand::Rewind(Some(n)));
            }
            return None; // 「退回」打头但尾巴不是数字 → 普通消息
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::provider::ScriptedProvider;
    use crate::turn::{text_response, tool_response};

    #[test]
    fn slash_commands_parse_both_rails() {
        // /verb 规范式 + 本地化斜杠别名，与裸词同权
        assert_eq!(parse_command("/skip"), Some(TextCommand::Skip));
        assert_eq!(parse_command("/stamp"), Some(TextCommand::Stamp));
        assert_eq!(parse_command("/pause"), Some(TextCommand::Pause));
        assert_eq!(parse_command("/resume"), Some(TextCommand::Resume));
        assert_eq!(parse_command("/sleep"), Some(TextCommand::SleepAll));
        assert_eq!(parse_command("/rewind"), Some(TextCommand::Rewind(None)));
        assert_eq!(
            parse_command("/rewind 2"),
            Some(TextCommand::Rewind(Some(2)))
        );
        assert_eq!(parse_command("/盖章"), Some(TextCommand::Stamp));
        assert_eq!(parse_command("/跳过"), Some(TextCommand::Skip));
        assert_eq!(parse_command("/退回 1"), Some(TextCommand::Rewind(Some(1))));
        // 未知 verb 与非法参数不劫持——算普通消息
        assert_eq!(parse_command("/dance"), None);
        assert_eq!(parse_command("/rewind abc"), None);
        assert_eq!(parse_command("/skip 多余尾巴"), None);
    }

    #[test]
    fn text_command_same_shape_as_button() {
        let dir = tempfile::tempdir().unwrap();
        let pack: PackDef = serde_json::from_value(json!({
            "name":"t","version":1,
            "stages":[{"name":"s0","roles":[],"due":[]},{"name":"s1","roles":["后端"],"due":[]}]
        }))
        .unwrap();
        let wb = Workbench::for_test(dir.path(), &["后端"], Some(pack)).unwrap();
        wb.open_stage(0).unwrap(); // 无角色 → skipped
        wb.open_stage(1).unwrap(); // s1 active
                                   // 文本指令「退回」应与按钮 rewind 产生同样事件
        wb.send_message("退回").unwrap();
        let kinds: Vec<String> = wb
            .events(Some(&[EventKind::StageRewound]))
            .unwrap()
            .iter()
            .map(|e| format!("{:?}", e.kind))
            .collect();
        assert_eq!(kinds, vec!["StageRewound"]);
        // 普通消息不触发命令
        let before = wb.events(None).unwrap().len();
        wb.send_message("退回这个事情我们再想想").unwrap();
        assert_eq!(wb.events(None).unwrap().len(), before + 1); // 只多一条消息
    }

    #[test]
    fn mention_and_path_reach_sleeping_agent_brief() {
        let dir = tempfile::tempdir().unwrap();
        let wb = Workbench::for_test(dir.path(), &["后端"], None).unwrap();
        // a0(后端) 默认 sleeping；负责人点名 + 路径指针
        wb.send_message("@后端 参考 #src/api.rs 重写鉴权").unwrap();
        let brief = crate::turn::build_brief_context(&wb.db, "a0", None).unwrap();
        assert_eq!(brief.mentions, vec!["@后端 参考 #src/api.rs 重写鉴权"]);
        assert_eq!(brief.paths, vec!["src/api.rs"]);
    }

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
    #[test]
    fn pending_questions_filters_answered_and_reject_at_stamp() {
        let dir = tempfile::tempdir().unwrap();
        let wb = Workbench::for_test(dir.path(), &["后端"], None).unwrap();
        // 手工造一条已答问题 + 一条排队问题：列表只回排队的
        wb.db.conn().execute(
            "INSERT INTO pending_questions (id, project_id, kind, payload, state)
             VALUES ('qa','p1','permission','{}','answered'), ('qb','p1','permission','{}','queued')",
            [],
        ).unwrap();
        let qs = wb.pending_questions().unwrap();
        assert_eq!(qs.len(), 1);
        assert_eq!(qs[0]["id"], "qb");
    }
    #[test]
    fn avatar_roundtrip_and_ext_switch() {
        let dir = tempfile::tempdir().unwrap();
        let wb = Workbench::for_test(dir.path(), &["后端"], None).unwrap();
        let aid = "a0";
        assert!(wb.agent_avatar(aid).unwrap().is_none());
        // 伪 PNG：若干字节即可，读写只认 data URL 包装
        use base64::Engine;
        let url = format!(
            "data:image/png;base64,{}",
            base64::engine::general_purpose::STANDARD.encode(b"fakepng")
        );
        wb.set_agent_avatar(aid, &url).unwrap();
        assert_eq!(wb.agent_avatar(aid).unwrap().unwrap(), url);
        assert!(dir.path().join(".hexagon/avatars/a0.png").exists());
        // 换 jpg：旧 png 应被清掉
        let url2 = format!(
            "data:image/jpeg;base64,{}",
            base64::engine::general_purpose::STANDARD.encode(b"fakejpg")
        );
        wb.set_agent_avatar(aid, &url2).unwrap();
        assert_eq!(wb.agent_avatar(aid).unwrap().unwrap(), url2);
        assert!(!dir.path().join(".hexagon/avatars/a0.png").exists());
    }
    #[test]
    fn artifact_content_at_reads_each_version() {
        let dir = tempfile::tempdir().unwrap();
        let wb = Workbench::for_test(dir.path(), &["后端"], None).unwrap();
        let ctx = crate::tools::ToolContext {
            project_id: "p1".into(),
            agent_id: "a0".into(),
            repo_root: dir.path().to_path_buf(),
            stage_run_id: None,
            owned_globs: vec![],
            tiers: crate::artifacts::TierMap::new(),
        };
        let d = |c: &str| {
            crate::artifacts::deliver(
                &wb.db,
                &ctx,
                &crate::artifacts::TierMap::new(),
                "docs/x.md",
                c,
                None,
            )
            .unwrap()
        };
        d("v1 body");
        d("v2 body");
        assert_eq!(
            wb.artifact_content_at("docs/x.md", 1).unwrap().unwrap(),
            "v1 body"
        );
        assert_eq!(
            wb.artifact_content_at("docs/x.md", 2).unwrap().unwrap(),
            "v2 body"
        );
        assert!(wb.artifact_content_at("docs/x.md", 9).unwrap().is_none());
        assert_eq!(wb.artifact_content("docs/x.md").unwrap(), "v2 body");
    }

    // ---------- 票 26 快速通道 ----------

    fn fastpath_wb(dir: &Path) -> Workbench {
        let wb = Workbench::for_test(dir, &["后端", "产品策划"], None).unwrap();
        wb.db
            .conn()
            .execute(
                "UPDATE projects SET mode='fastpath', fastpath_agent_id='a0' WHERE id='p1'",
                [],
            )
            .unwrap();
        wb
    }

    #[test]
    fn fastpath_dispatch_runs_without_stages() {
        let dir = tempfile::tempdir().unwrap();
        let mut wb = fastpath_wb(dir.path());
        wb.register_provider(
            "default",
            Arc::new(ScriptedProvider::new(vec![
                text_response("方案：定位登录态校验后修过期判断"),
                tool_response(vec![(
                    "t1",
                    "artifact_write",
                    json!({"path":"notes/fix.md","content":"---\nkind: 笔记\nauthor: a0\n---\n## 记\n修好了"}),
                )]),
                text_response("done"),
            ])),
        );
        wb.dispatch("后端", "直接修登录 bug").unwrap();
        // 无 stage_runs 行——快速通道不占阶段
        let n: i64 = wb
            .db
            .conn()
            .query_row("SELECT COUNT(*) FROM stage_runs", [], |r| r.get(0))
            .unwrap();
        assert_eq!(n, 0);
        // 产物落同一 .hexagon/，事件打标记
        assert!(dir.path().join(".hexagon/notes/fix.md").exists());
        let tl = wb.timeline(None, 50).unwrap();
        assert!(tl
            .iter()
            .any(|i| i.event.kind == EventKind::FastpathDispatched));
        assert!(tl
            .iter()
            .any(|i| i.event.kind == EventKind::ArtifactDelivered));
        assert_eq!(wb.project_info().unwrap()["mode"], "fastpath");
    }

    #[test]
    fn dispatch_coexists_with_pack() {
        let dir = tempfile::tempdir().unwrap();
        let pack: PackDef = serde_json::from_value(json!({
            "name":"t","version":1,
            "stages":[{"name":"规格","roles":["产品策划"],"due":["规格"]}]
        }))
        .unwrap();
        let mut wb = Workbench::for_test(dir.path(), &["产品策划", "后端"], Some(pack)).unwrap();
        wb.register_provider(
            "default",
            Arc::new(ScriptedProvider::new(vec![
                text_response("方案：改错别字"),
                text_response("fast fix"),
                text_response("spec done"),
            ])),
        );
        wb.open_stage(0).unwrap();
        // 阶段跑着的同时直接派后端干活——互不干扰
        wb.dispatch("后端", "顺手修个错别字").unwrap();
        let runs = wb.stage_status().unwrap();
        assert_eq!(runs.len(), 1);
        assert_eq!(runs[0]["state"], "active");
        // 阶段照常推进
        wb.run_turn("产品策划", "写规格").unwrap();
    }

    #[test]
    fn dispatch_wakes_sleeping_and_rejects_unchecked() {
        let dir = tempfile::tempdir().unwrap();
        let mut wb = fastpath_wb(dir.path());
        wb.register_provider(
            "default",
            Arc::new(ScriptedProvider::new(vec![
                text_response("方案：先看一遍"),
                text_response("ok"),
            ])),
        );
        wb.set_agent_sleeping("a0", true).unwrap();
        wb.dispatch("后端", "活来了").unwrap();
        let st: String = wb
            .db
            .conn()
            .query_row("SELECT status FROM agents WHERE id='a0'", [], |r| r.get(0))
            .unwrap();
        assert_eq!(st, "active");
        // 未勾选角色 → NoRole
        assert!(matches!(wb.dispatch("运维", "x"), Err(ApiError::NoRole(_))));
    }

    /// US15：快速通道动手前先发方案——方案消息先于首个工具调用落时间线，不阻塞等确认。
    #[test]
    fn us15_dispatch_posts_plan_before_tools() {
        let dir = tempfile::tempdir().unwrap();
        let mut wb = fastpath_wb(dir.path());
        let prov = Arc::new(ScriptedProvider::new(vec![
            text_response("方案：先读 auth.rs 定位登录态校验，再修过期判断"),
            tool_response(vec![(
                "t1",
                "artifact_write",
                json!({"path":"notes/fix.md","content":"---\nkind: 笔记\nauthor: a0\n---\n## 记\nx"}),
            )]),
            text_response("done"),
        ]));
        wb.register_provider("default", prov.clone());
        wb.dispatch("后端", "修登录 bug").unwrap();
        let tl = wb.timeline(None, 50).unwrap();
        let plan_idx = tl
            .iter()
            .position(|i| {
                i.message
                    .as_ref()
                    .map(|m| m.body.contains("方案"))
                    .unwrap_or(false)
            })
            .expect("plan message missing");
        let tool_idx = tl
            .iter()
            .position(|i| i.event.kind == EventKind::ToolCalled)
            .expect("tool call missing");
        assert!(plan_idx < tool_idx, "plan must land before first tool call");
        assert_eq!(prov.recorded().len(), 3); // 方案 1 + 执行 2，不阻塞
        assert!(dir.path().join(".hexagon/notes/fix.md").exists());
    }

    /// US15：负责人在工具循环期间可暂停——循环每轮顶检 paused 状态。
    #[test]
    fn us15_owner_pauses_mid_tool_loop() {
        // 第 2 次模型调用时经第二库句柄落 paused 事件，模拟工具循环中被叫停
        struct PauseOnNth {
            script: std::sync::Mutex<std::collections::VecDeque<crate::provider::ChatResponse>>,
            db2: std::sync::Mutex<Db>,
            pid: String,
            nth: usize,
            calls: std::sync::Mutex<usize>,
        }
        impl crate::provider::ModelProvider for PauseOnNth {
            fn complete(
                &self,
                _req: &crate::provider::ChatRequest,
            ) -> Result<crate::provider::ChatResponse, crate::provider::ProviderError> {
                let n = {
                    let mut c = self.calls.lock().unwrap();
                    *c += 1;
                    *c
                };
                if n == self.nth {
                    self.db2
                        .lock()
                        .unwrap()
                        .append_event(&self.pid, EventKind::Paused, json!({}), None, None)
                        .unwrap();
                }
                self.script
                    .lock()
                    .unwrap()
                    .pop_front()
                    .ok_or(crate::provider::ProviderError::ScriptExhausted)
            }
        }
        let dir = tempfile::tempdir().unwrap();
        let mut wb =
            Workbench::open(dir.path(), "t", &[("a0".into(), "后端".into())], None).unwrap();
        wb.db
            .conn()
            .execute(
                "UPDATE projects SET mode='fastpath', fastpath_agent_id='a0' WHERE id='p1'",
                [],
            )
            .unwrap();
        let db2 = Db::open(dir.path().join(".hexagon/state.db")).unwrap();
        wb.register_provider(
            "default",
            Arc::new(PauseOnNth {
                script: std::sync::Mutex::new(
                    vec![
                        text_response("方案：两步走"),
                        tool_response(vec![(
                            "t1",
                            "fs_write",
                            json!({"path":"src/a.rs","content":"x"}),
                        )]),
                        tool_response(vec![(
                            "t2",
                            "fs_write",
                            json!({"path":"src/b.rs","content":"x"}),
                        )]),
                        text_response("done"),
                    ]
                    .into(),
                ),
                db2: std::sync::Mutex::new(db2),
                pid: "p1".into(),
                nth: 2,
                calls: std::sync::Mutex::new(0),
            }),
        );
        let out = wb.dispatch("后端", "干活").unwrap();
        assert!(
            matches!(out, TurnOutcome::Failed(ref e) if e.contains("paused")),
            "got {out:?}"
        );
        // 只跑了方案 + 一轮工具：第 3 次模型调用没发生
        assert!(!dir.path().join("src/b.rs").exists());
    }

    #[test]
    fn upgrade_to_pack_pins_and_opens_stages() {
        let dir = tempfile::tempdir().unwrap();
        let mut wb = fastpath_wb(dir.path());
        wb.register_provider(
            "default",
            Arc::new(ScriptedProvider::new(vec![text_response("ok")])),
        );
        let pack: PackDef = serde_json::from_value(json!({
            "name":"规格驱动","version":1,
            "stages":[{"name":"规格","roles":["产品策划"],"due":["规格"],"stamp_point":true}]
        }))
        .unwrap();
        wb.upgrade_to_pack(pack).unwrap();
        assert!(dir.path().join(".hexagon/pack.active.json").exists());
        assert_eq!(wb.project_info().unwrap()["mode"], "pack");
        assert_eq!(wb.project_info().unwrap()["pack_name"], "规格驱动");
        // 钉完能开阶段
        wb.open_stage(0).unwrap();
        let runs = wb.stage_status().unwrap();
        assert_eq!(runs[0]["state"], "active");
        let tl = wb.timeline(None, 50).unwrap();
        assert!(tl.iter().any(|i| i.event.kind == EventKind::PackUpgraded));
    }

    /// US4：Agent 的模型槽决定消费哪个供应商脚本（BYOK 槽位路由）。
    #[test]
    fn us04_model_slot_binds_provider() {
        let dir = tempfile::tempdir().unwrap();
        let pack: PackDef = serde_json::from_value(json!({
            "name":"t","version":1,
            "stages":[{"name":"实现","roles":["后端"],"due":[]}]
        }))
        .unwrap();
        let mut wb = Workbench::for_test(dir.path(), &["后端"], Some(pack)).unwrap();
        // 该 Agent 绑 chat 槽：register 两个供应商，脚本只被 chat 消费
        wb.db
            .conn()
            .execute("UPDATE agents SET model_slot='chat' WHERE id='a0'", [])
            .unwrap();
        let chat = Arc::new(ScriptedProvider::new(vec![text_response("chat said")]));
        let other = Arc::new(ScriptedProvider::new(vec![text_response("wrong")]));
        wb.register_provider("chat", chat.clone());
        wb.register_provider("vision", other.clone());
        wb.open_stage(0).unwrap();
        wb.run_turn("后端", "go").unwrap();
        assert_eq!(chat.recorded().len(), 1);
        assert!(other.recorded().is_empty());
    }

    /// US71：关闭再打开只恢复文件真相——阶段/产物/团队/用量原样在，无重放。
    #[test]
    fn us71_reopen_restores_state() {
        let dir = tempfile::tempdir().unwrap();
        let pack: PackDef = serde_json::from_value(json!({
            "name":"t","version":1,
            "stages":[{"name":"规格","roles":["产品策划"],"due":["规格"]}]
        }))
        .unwrap();
        // 第一轮：开项目、开阶段、交一份产物
        {
            let mut wb = Workbench::open(
                dir.path(),
                "t",
                &[("a0".into(), "产品策划".into())],
                Some(pack.clone()),
            )
            .unwrap();
            wb.set_credential_store(Arc::new(crate::credentials::MemoryStore::default()));
            wb.register_provider(
                "default",
                Arc::new(ScriptedProvider::new(vec![
                    tool_response(vec![("t0", "artifact_write", json!({
                        "path":"specs/prd.md",
                        "content":"---\nkind: 规格\nauthor: a0\n---\n## 目标\nx\n## 范围\nx\n## 验收\nx"
                    }))]),
                    text_response("done"),
                ])),
            );
            wb.open_stage(0).unwrap();
            wb.run_turn("产品策划", "go").unwrap();
            assert_eq!(wb.artifacts().unwrap().len(), 1);
        }
        // 第二轮：重开——状态原样，零模型调用
        let mut wb = Workbench::open(
            dir.path(),
            "t",
            &[("a0".into(), "产品策划".into())],
            Some(pack),
        )
        .unwrap();
        let prov = Arc::new(ScriptedProvider::new(vec![text_response("replay?")]));
        wb.register_provider("default", prov.clone());
        assert_eq!(wb.stage_status().unwrap()[0]["state"], "active");
        assert_eq!(wb.artifacts().unwrap().len(), 1);
        assert_eq!(wb.team().unwrap().len(), 1);
        assert!(!wb.events(None).unwrap().is_empty());
        assert!(
            prov.recorded().is_empty(),
            "reopen must not replay model calls"
        );
    }

    /// US59：崩溃后重开检出中断回合——run 标 interrupted、出恢复卡、
    /// 不按恢复不发模型调用；按继续后恢复 active 且可正常跑回合。
    #[test]
    fn us59_interrupted_run_recovers_by_owner() {
        let dir = tempfile::tempdir().unwrap();
        let pack: PackDef = serde_json::from_value(json!({
            "name":"t","version":1,
            "stages":[{"name":"规格","roles":["产品策划"],"due":[]}]
        }))
        .unwrap();
        // 第一轮：开阶段，然后落一条无收束的 turn_started——模拟进程被杀时的盘上痕迹
        let run_id;
        {
            let wb = Workbench::open(
                dir.path(),
                "t",
                &[("a0".into(), "产品策划".into())],
                Some(pack.clone()),
            )
            .unwrap();
            wb.open_stage(0).unwrap();
            let run = wb.active_run().unwrap().unwrap();
            run_id = run.id.clone();
            wb.db
                .append_event(
                    "p1",
                    EventKind::TurnStarted,
                    json!({"agent": "a0"}),
                    Some("a0"),
                    Some(&run.id),
                )
                .unwrap();
        }
        // 第二轮：重开——检出中断痕迹
        let mut wb = Workbench::open(
            dir.path(),
            "t",
            &[("a0".into(), "产品策划".into())],
            Some(pack.clone()),
        )
        .unwrap();
        let prov = Arc::new(ScriptedProvider::new(vec![text_response("go")]));
        wb.register_provider("default", prov.clone());
        assert_eq!(wb.stage_status().unwrap()[0]["state"], "interrupted");
        let qs = wb.pending_questions().unwrap();
        let rec = qs
            .iter()
            .find(|q| q["kind"] == "recovery")
            .expect("recovery pending card");
        let rp: Value = serde_json::from_str(rec["payload"].as_str().unwrap()).unwrap();
        assert_eq!(rp["run_id"], run_id);
        assert!(prov.recorded().is_empty(), "reopen must not replay");
        // 恢复闸：不按继续，回合不发
        assert!(wb.run_turn("产品策划", "go").is_err());
        assert!(prov.recorded().is_empty());
        // 中断痕迹已闭环：轨迹里补了 turn_failed(interrupted_shutdown)
        let evs = wb.events(Some(&[EventKind::TurnFailed])).unwrap();
        assert_eq!(evs.len(), 1);
        // 负责人按继续——恢复 active、卡销、零模型调用
        wb.recover_run(&run_id).unwrap();
        assert_eq!(wb.stage_status().unwrap()[0]["state"], "active");
        assert!(wb
            .pending_questions()
            .unwrap()
            .iter()
            .all(|q| q["kind"] != "recovery"));
        assert_eq!(wb.events(Some(&[EventKind::Resumed])).unwrap().len(), 1);
        assert!(prov.recorded().is_empty());
        // 恢复后回合正常
        wb.run_turn("产品策划", "go").unwrap();
        assert_eq!(prov.recorded().len(), 1);
        // 第三轮：再重开——幂等，不重复出卡
        let wb = Workbench::open(
            dir.path(),
            "t",
            &[("a0".into(), "产品策划".into())],
            Some(pack),
        )
        .unwrap();
        assert_eq!(wb.stage_status().unwrap()[0]["state"], "active");
        assert!(wb
            .pending_questions()
            .unwrap()
            .iter()
            .all(|q| q["kind"] != "recovery"));
    }

    /// US33 余量（票 40）：检验红默认挡推进；负责人显式覆盖留痕放行。
    #[test]
    fn us33_check_override_lets_stage_pass() {
        let dir = tempfile::tempdir().unwrap();
        let pack: PackDef = serde_json::from_value(json!({
            "name":"t","version":1,
            "stages":[{"name":"实现","roles":["后端"],"due":[],"checks":["false"]}]
        }))
        .unwrap();
        let wb = Workbench::for_test(dir.path(), &["后端"], Some(pack)).unwrap();
        wb.open_stage(0).unwrap();
        wb.run_checks().unwrap(); // `false` → exit 1，检验红
                                  // 无覆盖：检验红挡推进
        let r = wb.advance().unwrap();
        assert_eq!(r["action"], "incomplete");
        assert!(r["missing"]
            .as_array()
            .unwrap()
            .iter()
            .any(|m| m.as_str() == Some("check:false")));
        // 显式覆盖：留痕 cmds + reason + by
        wb.override_checks("CI 环境缺依赖，本地已过").unwrap();
        let evs = wb.events(Some(&[EventKind::CheckOverridden])).unwrap();
        assert_eq!(evs.len(), 1);
        assert_eq!(evs[0].payload["cmds"], json!(["false"]));
        assert_eq!(evs[0].payload["reason"], "CI 环境缺依赖，本地已过");
        assert_eq!(evs[0].payload["by"], "owner");
        // 覆盖后推进放行（阶段收尾）
        let r = wb.advance().unwrap();
        assert_ne!(r["action"], "incomplete");
        // 无红可覆 → 报错（防无痕迹空覆盖）
        assert!(wb.override_checks("again").is_err());
    }
}

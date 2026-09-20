//! 核内 API 面：全项目唯一测试主接缝，也是 UI 与核的唯一边界。
//!
//! `Workbench` 把一个项目的 Db + 工具注册表 + 供应商槽 + 包副本捏在一起；
//! UI（Tauri command 层）和场景 DSL 都只过这层，没有旁路通道。

use crate::artifacts::TierMap;
use crate::commands::TextCommand;
use crate::db::Db;
use crate::orchestra::{self, OrchError, PackDef};
use crate::provider::ModelProvider;
use crate::tools::{Registry, ToolContext};
use crate::trace::{EventKind, TraceError};
use crate::turn::{self, TurnOutcome};
use serde_json::json;
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::Mutex;

/// 回合 delta 外发钩子类型（票 03）：壳层注入，emit 到 webview。
pub type TurnDeltaHook = Box<dyn FnMut(&turn::TurnDelta) + Send>;

/// 升级卡裁决回执（ADR 0054）：两个子型共享一条命令，serde(untagged)
/// 平铺——wire 形状与旧 json! 一致（{resumed:...} 或 {adjudicated:...}）。
#[derive(Debug, Clone, serde::Serialize)]
#[serde(untagged)]
#[derive(ts_rs::TS)]
#[ts(export, export_to = "../../../ui/src/gen/")]
pub enum AdjudicateOutcome {
    /// context_overflow 子型（US37）：放行=续跑一回合。
    Resumed {
        resumed: bool,
        #[serde(skip_serializing_if = "Option::is_none")]
        outcome: Option<String>,
    },
    /// 普通打回卡：agree→退回重跑 / reject→留痕。
    Flag(crate::review::FlagOutcome),
}

/// open_stage 回执（ADR 0054）。
#[derive(Debug, Clone, serde::Serialize, ts_rs::TS)]
#[ts(export, export_to = "../../../ui/src/gen/")]
pub struct OpenStageOutcome {
    pub run_id: String,
    pub skipped: bool,
}

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
    #[error(transparent)]
    PolicyDev(#[from] crate::policydev::PolicyDevError),
    #[error("sqlite: {0}")]
    Sqlite(#[from] rusqlite::Error),
    #[error(transparent)]
    Cards(#[from] crate::cards::CardsError),
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
    #[error(transparent)]
    Install(#[from] crate::install::InstallError),
    #[error(transparent)]
    Roles(#[from] crate::roles::RoleError),
    #[error(transparent)]
    PackEdit(#[from] crate::packedit::PackEditError),
    #[error(transparent)]
    Judge(#[from] crate::judge::JudgeError),
    #[error("bad input: {0}")]
    BadInput(String),
}

/// 工作台实例：一个打开的项目。
pub struct Workbench {
    pub db: Db,
    pub registry: Registry,
    /// 票 05：槽位表不外泄——壳层热刷新走 reload_providers()，
    /// 测试注入走 register_provider()，直改本字段的路已断（ADR 0053）。
    pub(crate) providers: HashMap<String, Arc<dyn ModelProvider>>,
    pub creds: Arc<dyn crate::credentials::CredentialStore>,
    pub project_id: String,
    pub repo_root: PathBuf,
    pub pack: Option<PackDef>,
    /// 回合 delta hook（turn-streaming 票 03）：壳层挂 Tauri emit；
    /// None=静默（测试/CLI 路径不变）。Mutex 包住——turn 入口全是 &self；
    /// Send 约束在这里（hook 要跨线程 emit），turn 层的 DeltaSink 本身无此要求。
    /// 注意：锁跨越整个回合，hook 内回调 Workbench 会死锁。
    delta_hook: Mutex<Option<TurnDeltaHook>>,
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
        let project_id = crate::PROJECT_ID.to_string();
        db.conn().execute(
            "INSERT OR IGNORE INTO projects (id, dir, name, mode)
             VALUES (?1, ?2, ?3, 'pack')",
            rusqlite::params![crate::PROJECT_ID, dir.to_string_lossy(), name],
        )?;
        for (aid, role) in roles {
            db.conn().execute(
                "INSERT OR IGNORE INTO agents (id, project_id, role) VALUES (?1,?2,?3)",
                rusqlite::params![aid, crate::PROJECT_ID, role],
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
            delta_hook: Mutex::new(None),
        })
    }

    /// 打开 + 接线（壳层 open_project/open_recent 共用序列，票 05）：
    /// open → 凭据库 → 按 providers 文档注册运行槽位。
    pub fn open_with(
        dir: impl AsRef<Path>,
        name: &str,
        roles: &[(String, String)],
        pack: Option<PackDef>,
        store: Arc<dyn crate::credentials::CredentialStore>,
    ) -> Result<Self, ApiError> {
        let mut wb = Self::open(dir, name, roles, pack)?;
        wb.attach_providers(store);
        Ok(wb)
    }

    /// 接线尾步（create_project 等自建 Workbench 的路径共用）：
    /// 设凭据库 + 按当前 providers 文档挂槽位。
    pub fn attach_providers(&mut self, store: Arc<dyn crate::credentials::CredentialStore>) {
        self.creds = store;
        self.reload_providers();
    }

    /// 热刷新运行中的供应商注册（保存/删除/绑定变更后调）：
    /// 清掉文档覆盖的槽再按现状重挂——文档外的槽位（测试注入的
    /// ScriptedProvider）原样保留。
    pub fn reload_providers(&mut self) {
        let doc = crate::provider_config::load().unwrap_or_default();
        for slot in doc.slots.keys() {
            self.providers.remove(slot);
        }
        crate::provider_config::register_all(&mut self.providers, self.creds.clone());
    }

    /// 测试构造：内存库 + 临时仓。
    pub fn for_test(dir: &Path, roles: &[&str], pack: Option<PackDef>) -> Result<Self, ApiError> {
        let db = Db::open_in_memory()?;
        db.conn().execute(
            "INSERT INTO projects (id, dir, name, mode) VALUES (?1,?2,'t','pack')",
            rusqlite::params![crate::PROJECT_ID, dir.to_string_lossy().to_string()],
        )?;
        for (i, r) in roles.iter().enumerate() {
            db.conn().execute(
                "INSERT INTO agents (id, project_id, role) VALUES (?1,?2,?3)",
                rusqlite::params![format!("a{i}"), crate::PROJECT_ID, r],
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
            project_id: crate::PROJECT_ID.into(),
            repo_root: dir.to_path_buf(),
            pack,
            delta_hook: Mutex::new(None),
        })
    }

    /// 注册回合 delta hook（票 03）：壳层在 open 后调一次挂上 emit。
    /// hook 不得回调 Workbench——锁跨越整个回合，回调即死锁。
    pub fn set_turn_delta_hook(&self, hook: Option<TurnDeltaHook>) {
        *self.delta_hook.lock().unwrap() = hook;
    }

    pub fn register_provider(&mut self, slot: &str, p: Arc<dyn ModelProvider>) {
        self.providers.insert(slot.into(), p);
    }

    /// 测试/桌面端注入凭据实现（默认内存库；生产壳换成 OsKeychain）。
    pub fn set_credential_store(&mut self, store: Arc<dyn crate::credentials::CredentialStore>) {
        self.creds = store;
    }

    /// 确认发布：凭据闸 + push。
    pub fn confirm_publish(&self, qid: &str) -> Result<crate::publish::PublishOutcome, ApiError> {
        Ok(crate::publish::confirm(
            &self.db,
            &self.project_id,
            qid,
            self.creds.as_ref(),
        )?)
    }

    /// 升级卡裁决：payload 取 flag_id → review::adjudicate_flag；标记问题已答。
    /// sub=context_overflow（US37）走撞限语义：放行=续跑回合，驳回=回合收场。
    pub fn adjudicate_flag(&self, qid: &str, agree: bool) -> Result<AdjudicateOutcome, ApiError> {
        // 卡表读写归 cards.rs（arch-review 票 04）；sub 分发也归它（票 05）
        let card = crate::cards::get_queued(&self.db, qid, crate::cards::CardKind::Escalation)?;
        let pv = &card.payload;
        if crate::cards::escalation_sub(&card) == crate::cards::EscalationSub::ContextOverflow {
            crate::cards::answer(&self.db, qid, "owner")?;
            self.db.append_event(
                &self.project_id,
                EventKind::System,
                json!({
                    "kind": if agree { "context_resumed" } else { "context_denied" },
                    "question_id": qid,
                    "est_tokens": pv["est_tokens"],
                    "by": "owner",
                }),
                None,
                None,
            )?;
            if !agree {
                return Ok(AdjudicateOutcome::Resumed {
                    resumed: false,
                    outcome: None,
                });
            }
            // 放行：以「继续」指令续跑一回合（上下文重建自带轻量裁剪）
            let role = pv["role"].as_str().unwrap_or_default().to_string();
            let out = self.run_turn_opts(
                &role,
                "上下文撞限已由负责人放行。请接着完成未竟任务（上下文已重建+裁剪）。",
                false,
            )?;
            return Ok(AdjudicateOutcome::Resumed {
                resumed: true,
                outcome: Some(format!("{out:?}")),
            });
        }
        let flag_id = pv["flag_id"].as_str().unwrap_or_default().to_string();
        let ctx = self.ctx_for("owner", None);
        let v = crate::review::adjudicate_flag(&self.db, &ctx, self.pack()?, &flag_id, agree)?;
        crate::cards::answer(&self.db, qid, "owner")?;
        Ok(AdjudicateOutcome::Flag(v))
    }

    /// 设置 agent 头像：UI 传 data URL（data:image/png;base64,…），
    /// 盖章点驳回：退上一阶段（与打回不同通道）。
    pub fn reject_stamp(&self) -> Result<orchestra::StageAction, ApiError> {
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

    /// 策略研发提案（票 10）：副本改旋钮 → 双保险 → 回放 → 携证据
    /// +机械判定进提案队列。产出物无任何特权——submit 全校验照常。
    /// 旋钮编辑走 JSON（壳层 Tauri 命令与测试同一入口）;sandbox 由
    /// 内部建在 `.hexagon/replay/` 下,回放不碰项目仓工作树。
    pub fn policydev_propose(
        &self,
        edits: &[serde_json::Value],
        scenario: &crate::scenario::Scenario,
        motive: &str,
    ) -> Result<String, ApiError> {
        let edits: Vec<crate::policydev::KnobEdit> = edits
            .iter()
            .map(crate::policydev::KnobEdit::from_value)
            .collect::<Result<_, _>>()?;
        let base = self
            .pack
            .clone()
            .ok_or_else(|| ApiError::NoRole("no pack pinned".into()))?;
        let aid = crate::policydev::policy_dev_agent(&self.db, &self.project_id)
            .map_err(|_| ApiError::NoRole("政策研发 agent 不在团队".into()))?;
        let ctx = self.ctx_for(&aid, None);
        // nanos 而非 secs：同秒两次 propose 会撞目录互相覆盖回放产物
        //（arch-review 附录 B5 核验证实）。nanos 冲突实际不可能。
        let sandbox = self.repo_root.join(format!(
            ".hexagon/replay/{}",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_nanos())
                .unwrap_or(0)
        ));
        Ok(crate::policydev::propose(
            &self.db, &ctx, &base, &edits, scenario, motive, &sandbox,
        )?)
    }

    // ---------- 命令 ----------

    /// 文本指令分发（wb 侧：壳层经 commands::send_via_control 落库 +
    /// 就地处理 Pause/Resume 后，余下指令到这里；票 05 前壳层自己路由）。
    pub fn dispatch_command(&self, cmd: &TextCommand) -> Result<(), ApiError> {
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
            TextCommand::Pause => orchestra::pause(&self.db, &self.project_id)?,
            TextCommand::Resume => orchestra::resume(&self.db, &self.project_id)?,
            TextCommand::SleepAll => orchestra::sleep_all(&self.db, &self.project_id)?,
            TextCommand::Install(desc) => {
                crate::install::request_install(&self.db, &self.project_id, &self.repo_root, desc)?;
            }
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
        // 列里存的是 agent_id（变量曾误名 payload——arch-review 票 01 订正）
        let agent_id = crate::cards::get(&self.db, question_id)?
            .agent_id
            .ok_or_else(|| ApiError::BadInput(format!("question {question_id} has no agent")))?;
        let ctx = self.ctx_for(&agent_id, None);
        self.registry.resolve(
            &self.db,
            &ctx,
            question_id,
            allow,
            remember_shape,
            scope,
            self.pack.as_ref(),
            "owner",
        )?;
        Ok(())
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
        let provider = crate::provider_config::resolve_slot(
            &self.providers,
            slot.as_deref().unwrap_or("default"),
        )
        .ok_or_else(|| ApiError::NoProvider(slot.clone().unwrap_or_default()))?;
        // 票 03：delta hook 锁跨整个回合——hook 一旦挂上，所有走
        // run_turn_opts 的入口（run_turn/dispatch/撞限放行）自动流式。
        let mut guard = self.delta_hook.lock().unwrap();
        let sink = guard.as_mut().map(|h| &mut **h as &mut turn::DeltaSink<'_>);
        let run = turn::run_turn_streaming(
            &self.db,
            provider.as_ref(),
            &self.registry,
            &ctx,
            vec![],
            input,
            plan_first,
            sink,
        );
        // 票 08：回合扫尾——本轮若落了带回放证据的待盖章提案,
        // 按包旋钮跑判定层。判定失败不挡回合（judge 是建议不是闸）。
        drop(guard);
        if let Some(backend) = crate::judge::backend_for(
            crate::judge::JudgeFacet::ProposalStamp,
            self.pack.as_ref(),
            &self.providers,
            slot.as_deref().unwrap_or("default"),
            Some(crate::judge::JudgeObs {
                db: &self.db,
                project_id: &self.project_id,
                repo_root: &self.repo_root,
            }),
        ) {
            if let Err(e) = crate::judge::sweep(
                &self.db,
                &self.project_id,
                &self.repo_root,
                backend.as_ref(),
            ) {
                log::warn!("judge sweep failed: {e}");
            }
        }
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
            orchestra::write_agent_status(&self.db, &self.project_id, &aid, false)?;
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

    pub fn open_stage(&self, seq: usize) -> Result<OpenStageOutcome, ApiError> {
        let pack = self.pack()?;
        let (rid, skipped) = orchestra::open_stage(&self.db, &self.project_id, pack, seq)?;
        Ok(OpenStageOutcome {
            run_id: rid,
            skipped,
        })
    }

    pub fn advance(&self) -> Result<orchestra::StageAction, ApiError> {
        Ok(orchestra::advance(
            &self.db,
            &self.project_id,
            self.pack()?,
        )?)
    }
    pub fn run_checks(&self) -> Result<orchestra::CheckOutcome, ApiError> {
        let r = orchestra::run_checks(&self.db, &self.project_id, &self.repo_root, self.pack()?)?;
        Ok(orchestra::CheckOutcome { results: r })
    }
    pub fn stamp(&self) -> Result<orchestra::StageAction, ApiError> {
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

    pub fn rewind(&self, to_seq: usize) -> Result<orchestra::StageAction, ApiError> {
        Ok(orchestra::rewind(
            &self.db,
            &self.project_id,
            self.pack()?,
            to_seq,
        )?)
    }
    pub fn skip(&self) -> Result<orchestra::StageAction, ApiError> {
        Ok(orchestra::skip(&self.db, &self.project_id, self.pack()?)?)
    }

    /// 恢复中断的阶段 run（票 37）：负责人按「继续」才重激活，不重放模型调用。
    pub fn recover_run(&self, run_id: &str) -> Result<(), ApiError> {
        orchestra::recover_run(&self.db, &self.project_id, run_id)?;
        Ok(())
    }

    /// 角色设定起草（票 30）：模型起草 duty/定位，人确认才写回（update_agent）。
    /// 单发无工具调用；走该 Agent 的模型槽。
    pub fn draft_role_def(&self, agent_id: &str, hint: &str) -> Result<String, ApiError> {
        let slot: Option<String> = self.db.conn().query_row(
            "SELECT model_slot FROM agents WHERE id=?1",
            [agent_id],
            |r| r.get(0),
        )?;
        let provider = crate::provider_config::resolve_slot(
            &self.providers,
            slot.as_deref().unwrap_or("default"),
        )
        .ok_or_else(|| ApiError::NoProvider(slot.clone().unwrap_or_default()))?;
        let role: String =
            self.db
                .conn()
                .query_row("SELECT role FROM agents WHERE id=?1", [agent_id], |r| {
                    r.get(0)
                })?;
        let req = crate::provider::ChatRequest {
            model_slot: slot.unwrap_or_else(|| "default".into()),
            messages: vec![crate::provider::Message {
                role: crate::provider::Role::User,
                content: vec![crate::provider::ContentBlock::Text {
                    text: format!(
                        "为虚拟团队角色「{role}」起草一段中文职责描述（≤80字，只输出职责正文）。补充要求：{hint}"
                    ),
                }],
            }],
            tools: vec![],
        };
        let resp = provider.complete(&req).map_err(turn::TurnError::from)?;
        let text = resp
            .content
            .iter()
            .filter_map(|b| match b {
                crate::provider::ContentBlock::Text { text } => Some(text.as_str()),
                _ => None,
            })
            .collect::<Vec<_>>()
            .join("\n");
        Ok(text)
    }

    /// 显式覆盖检验失败（票 40）：留痕 check_overridden（谁/哪些命令/理由）。
    pub fn override_checks(&self, reason: &str) -> Result<orchestra::OverrideOutcome, ApiError> {
        Ok(orchestra::override_checks(
            &self.db,
            &self.project_id,
            self.pack()?,
            reason,
        )?)
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
}

// 文本指令与 token 解析已迁往 `commands.rs`（中立模块，arch-review 票 01）：
// turn.rs 曾为此反向依赖本门面（诊断卡 D08）。

#[cfg(test)]
mod tests;

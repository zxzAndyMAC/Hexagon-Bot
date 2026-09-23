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
use std::cell::Cell;
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::Mutex;

/// 一条派活链上最多再派这么多次。
///
/// 代价：再放行 = 模型互相点名可以无人值守烧完额度（false continue）；
/// 停下来 = 这一轮之后要负责人再说一句（false stop）。偏向停。
/// 出处：票 09。被否决：不设上限，只靠「先不派活」——模型不选那一项就停不下来。
const DISPATCH_CHAIN_CAP: u32 = 8;

thread_local! {
    static DISPATCH_CHAIN: Cell<u32> = const { Cell::new(0) };
}

struct ChainGuard(u32);

impl ChainGuard {
    fn enter() -> Option<Self> {
        let cur = DISPATCH_CHAIN.get();
        if cur >= DISPATCH_CHAIN_CAP {
            return None;
        }
        DISPATCH_CHAIN.set(cur + 1);
        Some(Self(cur))
    }
}

impl Drop for ChainGuard {
    fn drop(&mut self) {
        DISPATCH_CHAIN.set(self.0);
    }
}

/// 花名册里的点名，按出现顺序去重。说话人自己不算「下一位」。
fn roster_mentions(body: &str, roster: &[String], speaker: &str) -> Vec<String> {
    let mut out = Vec::new();
    for token in crate::commands::parse_tokens(body) {
        let crate::trace::MessageToken::Mention { agent_role } = token else {
            continue;
        };
        if agent_role == speaker || out.iter().any(|r| r == &agent_role) {
            continue;
        }
        if roster.iter().any(|r| r == &agent_role) {
            out.push(agent_role);
        }
    }
    out
}

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
    Grant(#[from] crate::grants::GrantError),
    #[error(transparent)]
    Roles(#[from] crate::roles::RoleError),
    #[error(transparent)]
    PackEdit(#[from] crate::packedit::PackEditError),
    #[error(transparent)]
    Judge(#[from] crate::judge::JudgeError),
    #[error("bad input: {0}")]
    BadInput(String),
    /// 封闭选择的模型调用失败（不是聊天回合的 Turn 错）。
    #[error("decision model: {0}")]
    Decision(String),
    /// 开场草案还没产生，或已经写过。点头没有对象。
    #[error("no opening draft to confirm")]
    NoIntakeDraft,
    /// 仓库里已经有项目说明。开场分析只引用，确认也不覆盖。
    #[error("project instructions already exist")]
    IntakeBriefExists,
}

/// 下一手派完之后的结果。
///
/// `Skipped` = 不派：整句是指令、接话人就是刚说完的那位，或链到了上限。
/// `KeptInChat` = 角色点名在 L0/L1，只留在时间线上，不唤醒。
/// `Noted` = 没有接话人，工作台自己写了说明，作者不是角色。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum UnnamedRoute {
    Skipped,
    Dispatched {
        role: String,
        via: String,
    },
    Held {
        via: String,
    },
    Rejected {
        raw: String,
        via: String,
    },
    /// 点名直接派了，没有做「先不派活」的封闭选择。
    Mentioned {
        roles: Vec<String>,
    },
    KeptInChat {
        roles: Vec<String>,
    },
    Noted,
}

/// 开场分析跑完（或决定不跑）之后，门面外能看见的结果。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum IntakeRun {
    /// 空目录、已经分析过、或另一次调用正在跑。没叫模型。
    Skipped,
    /// 没有接话人。时间线上是工作台的说明，作者不是角色。
    Noted,
    /// 接话人把分析写进了时间线。`draft` = 磁盘上还没有项目说明。
    Posted { role: String, draft: bool },
}

/// `prepare` 的结果。`Call` 的模型调用必须在工作台锁外面做，
/// 否则负责人打字要等模型返回。
pub enum IntakePrepared {
    Finished(IntakeRun),
    Call {
        request: crate::provider::ChatRequest,
        provider: Arc<dyn ModelProvider>,
    },
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
    /// MCP 宿主（ui-audit-2 票 06）：持全部服务子进程，Drop 时全停。
    /// `.hexagon/mcp.json` 缺席/为空 → None（MCP 是可选项）。
    mcp_host: Option<crate::mcp::McpHost>,
    /// 终端会话表（agent-senses 票 05）：ctx_for 注入共享表；
    /// Drop 时 kill_all——项目关闭/切换不留 sh 孤儿。
    sessions: crate::sessions::SessionTable,
    /// 开场分析已经领走、还没落时间线的接话人。模型调用在壳层的锁外面，
    /// 所以接话人不能只放在栈上。
    intake_speaker: Mutex<Option<(String, String)>>,
}

impl Drop for Workbench {
    fn drop(&mut self) {
        self.sessions.kill_all();
    }
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
        // 票 01 / ADR 0064：省略 autonomy → 列默认 L4。再次打开走 OR IGNORE，不改已选档。
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
        // ui-audit-2 票 06：MCP 端到端——`.hexagon/mcp.json` 配置的服务
        // 在 open 时 spawn+握手，工具注册进统一管线（权限照常求值，
        // grants 表 mcp 授权闸门在 evaluate L0）。
        let registry = Registry::builtin();
        let specs = crate::mcp::load_specs(&dir);
        // 2026-09-22：握手不挡进入工作台。工具在后台就绪后由 poll / 回合入口装上。
        let mcp_host = if specs.is_empty() {
            None
        } else {
            Some(crate::mcp::McpHost::begin(specs))
        };
        Ok(Self {
            db,
            registry,
            providers: HashMap::new(),
            creds: crate::credentials::active(),
            project_id,
            repo_root: dir,
            pack,
            delta_hook: Mutex::new(None),
            mcp_host,
            sessions: Default::default(),
            intake_speaker: Mutex::new(None),
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
        // 决策槽绑上了就交给项目经理。槽名固定 decision，不和主对话槽合成一个。
        // 没这个角色（测试夹具）就略过。卸掉绑定且当前正指着 decision 时清空。
        let points_at_decision = self
            .db
            .conn()
            .query_row(
                "SELECT decision_slot FROM agents WHERE project_id=?1 AND role=?2",
                rusqlite::params![self.project_id, crate::pm_route::PM_ROLE],
                |r| r.get::<_, Option<String>>(0),
            )
            .ok();
        if self.providers.contains_key("decision") {
            let _ = self.set_decision_slot(crate::pm_route::PM_ROLE, Some("decision"));
        } else if points_at_decision.flatten().as_deref() == Some("decision") {
            let _ = self.set_decision_slot(crate::pm_route::PM_ROLE, None);
        }
    }

    /// 测试构造：内存库 + 临时仓。
    pub fn for_test(dir: &Path, roles: &[&str], pack: Option<PackDef>) -> Result<Self, ApiError> {
        let db = Db::open_in_memory()?;
        // 票 01：for_test 不是新项目。钉 L0，避免把「默认 L4、执行如 L2」灌进
        // 既有「未声明档位 = 全部排队」的执行语义用例。用户路径是 open / create_project。
        db.conn().execute(
            "INSERT INTO projects (id, dir, name, mode, autonomy) VALUES (?1,?2,'t','pack','L0')",
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
        // for_test 同样接 MCP（票 06）——测试仓写 .hexagon/mcp.json
        // 即得端到端覆盖；无配置则 None。
        let registry = Registry::builtin();
        let specs = crate::mcp::load_specs(dir);
        let mcp_host = if specs.is_empty() {
            None
        } else {
            Some(crate::mcp::McpHost::start(specs, &registry))
        };
        Ok(Self {
            db,
            registry,
            providers: HashMap::new(),
            creds: Arc::new(crate::credentials::MemoryStore::default()),
            project_id: crate::PROJECT_ID.into(),
            repo_root: dir.to_path_buf(),
            pack,
            delta_hook: Mutex::new(None),
            mcp_host,
            sessions: Default::default(),
            intake_speaker: Mutex::new(None),
        })
    }

    /// 注册回合 delta hook（票 03）：壳层在 open 后调一次挂上 emit。
    /// hook 不得回调 Workbench——锁跨越整个回合，回调即死锁。
    pub fn set_turn_delta_hook(&self, hook: Option<TurnDeltaHook>) {
        *self.delta_hook.lock().unwrap() = hook;
    }

    /// 注册 bash 输出 delta hook（exec-cards 票 04）：与 turn-delta
    /// 同纪律的瞬时展示通道——不落库、只增不重放，持久层照旧是
    /// tool_result 事件。直接挂进共享 SessionTable（经 ToolContext
    /// 注入每个调用），spawn/run_in 时绑归属。
    pub fn set_tool_output_hook(&self, hook: Option<crate::sessions::OutputTapCb>) {
        self.sessions.set_output_tap(hook);
    }

    pub fn register_provider(&mut self, slot: &str, p: Arc<dyn ModelProvider>) {
        self.providers.insert(slot.into(), p);
    }

    /// 项目经理的决策槽（票 08）。`None` 或空白 = 没配，封闭选择改走主对话槽。
    /// 只写这一列，不碰 `model_slot`——两个槽不许合成一个。
    pub fn set_decision_slot(&self, role: &str, slot: Option<&str>) -> Result<(), ApiError> {
        let slot = slot.map(str::trim).filter(|s| !s.is_empty());
        let n = self.db.conn().execute(
            "UPDATE agents SET decision_slot=?1 WHERE project_id=?2 AND role=?3",
            rusqlite::params![slot, self.project_id, role],
        )?;
        if n == 0 {
            return Err(ApiError::NoRole(role.into()));
        }
        Ok(())
    }

    /// MCP 服务实况（ui-audit-2 票 06）：每配置服务一行（含失败原因）。
    /// 无 `.hexagon/mcp.json` → 空表。
    pub fn mcp_services(&self) -> Vec<crate::mcp::McpServiceRow> {
        self.install_ready_mcp_tools();
        self.mcp_host
            .as_ref()
            .map(|h| h.status())
            .unwrap_or_default()
    }

    /// 把已经握手成功的 MCP 工具装进注册表。不等人。
    fn install_ready_mcp_tools(&self) {
        let Some(host) = self.mcp_host.as_ref() else {
            return;
        };
        for tool in host.take_ready() {
            self.registry.register(tool);
        }
    }

    /// 回合要工具清单之前：握手还没完就等到结束（或 8 秒）。
    /// 失败的服务保持 down，不在这里抛——调用时由工具缺失或状态行提示。
    pub fn ensure_mcp_for_turn(&self) {
        if let Some(host) = self.mcp_host.as_ref() {
            host.wait_settled(std::time::Duration::from_secs(8));
        }
        self.install_ready_mcp_tools();
    }

    /// 测试/桌面端注入凭据实现（默认内存库；生产壳换成 OsKeychain）。
    pub fn set_credential_store(&mut self, store: Arc<dyn crate::credentials::CredentialStore>) {
        self.creds = store;
    }

    /// 自治读口。ADR 0069：不再返回可调档。离开时的放行是固定范围，
    /// 不是 L0–L4 里的一项。存储列仍给既有判定读，不从这扇门暴露。
    pub fn autonomy(&self) -> Result<&'static str, ApiError> {
        let _ = self;
        Ok("fixed")
    }

    /// 改自治档。一律拒绝，已存列不动。
    pub fn set_autonomy(&self, level: &str) -> Result<(), ApiError> {
        crate::autonomy::set_level(&self.db, &self.project_id, level)?;
        Ok(())
    }

    /// 发起远程发布：只入队确认卡，不执行。任何自治档都不自动放行（票 03 / ADR 0034）。
    pub fn request_publish(&self, remote: &str) -> Result<String, ApiError> {
        Ok(crate::publish::request(&self.db, &self.project_id, remote)?)
    }

    /// 自然语言安装。L4 自动写入当前项目；L0–L3 只入队。
    pub fn request_install(&self, desc: &str) -> Result<String, ApiError> {
        Ok(crate::install::request_install(
            &self.db,
            &self.project_id,
            &self.repo_root,
            desc,
        )?)
    }

    /// 上级复审结论。通过且存储档为 L4 时，负责人盖章自动生效（可回滚）。
    /// 驳回不生效。无上级的提案不走这里，仍等负责人。
    pub fn review_proposal(
        &self,
        proposal_id: &str,
        pass: bool,
        reason: &str,
    ) -> Result<(), ApiError> {
        let author: String = self.db.conn().query_row(
            "SELECT author_agent_id FROM proposals WHERE id=?1 AND project_id=?2",
            rusqlite::params![proposal_id, self.project_id],
            |r| r.get(0),
        )?;
        let ctx = self.ctx_for(&author, None);
        // 只取 Jev 槽，而且必须是决策接口。没绑、或绑的是聊天模型，都不调用。
        // 被否决：没配 Jev 就 resolve_slot 到 default。那是用聊天模型顶替判定。
        let jev = self
            .providers
            .get(crate::provider_config::JEV_SLOT)
            .filter(|p| p.uses_decision_api())
            .map(|p| p.as_ref());
        crate::proposals::review(&self.db, &ctx, proposal_id, pass, reason, jev)?;
        Ok(())
    }

    /// 复审通过之后提交经验。还不写技能。执行判定通过才落盘。
    pub fn propose_experience(
        &self,
        agent_id: &str,
        lesson: &str,
        read: &[String],
    ) -> Result<String, ApiError> {
        let ctx = self.ctx_for(agent_id, None);
        Ok(crate::experience::propose(
            &self.db,
            &ctx,
            lesson,
            read,
        )?)
    }

    /// 激活简报里的技能目录。经验正文不在这里。
    pub fn skill_catalog(&self) -> Result<String, ApiError> {
        let loader = crate::skills::SkillLoader::new(crate::skills::skill_dirs(&self.repo_root));
        Ok(loader.catalog_text(&Default::default()).unwrap_or_default())
    }

    /// 回滚一张已生效的改进提案。
    pub fn rollback_proposal(&self, proposal_id: &str) -> Result<(), ApiError> {
        let author: String = self.db.conn().query_row(
            "SELECT author_agent_id FROM proposals WHERE id=?1 AND project_id=?2",
            rusqlite::params![proposal_id, self.project_id],
            |r| r.get(0),
        )?;
        let ctx = self.ctx_for(&author, None);
        crate::proposals::rollback(&self.db, &ctx, proposal_id)?;
        Ok(())
    }

    /// 技能或 MCP 授权确认。L4 只写入当前项目 grants；L0–L3 入队等人。
    pub fn request_grant(
        &self,
        agent_id: &str,
        kind: &str,
        name: &str,
    ) -> Result<crate::grants::GrantOutcome, ApiError> {
        Ok(crate::grants::request(
            &self.db,
            &self.project_id,
            agent_id,
            kind,
            name,
        )?)
    }

    /// 负责人裁决授权卡。允许才写入项目 grants，不写用户全局。
    pub fn confirm_grant(
        &self,
        qid: &str,
        allow: bool,
    ) -> Result<crate::grants::GrantOutcome, ApiError> {
        Ok(crate::grants::confirm(
            &self.db,
            &self.project_id,
            qid,
            allow,
        )?)
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
                &[],
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

    /// 非最终盖章点驳回：退上一阶段。停在最终验收上时拒绝——要阶段名和修改意见。
    pub fn reject_stamp(&self) -> Result<orchestra::StageAction, ApiError> {
        Ok(crate::review::reject_stamp(
            &self.db,
            &self.project_id,
            self.pack()?,
        )?)
    }

    /// 最终验收退回。缺阶段名或修改意见（空白不算）则拒绝。只重开被点名的阶段。
    pub fn reject_final(
        &self,
        stage_name: &str,
        note: &str,
    ) -> Result<orchestra::StageAction, ApiError> {
        let stage_name = stage_name.trim();
        let note = note.trim();
        if stage_name.is_empty() || note.is_empty() {
            return Err(ApiError::BadInput(
                "final acceptance reject requires a stage name and a revision note".into(),
            ));
        }
        Ok(crate::review::reject_final(
            &self.db,
            &self.project_id,
            self.pack()?,
            stage_name,
            note,
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
            sessions: self.sessions.clone(),
            caps: {
                // 票 02：槽位 caps 注入（读 agents.model_slot → 绑定的模型
                // → caps 词表）。槽没绑定/无条目 → 空集（无 vision 等能力，
                // fail-closed：宁可降读图，不把字节塞进看不见图的模型）。
                let slot: Option<String> = self
                    .db
                    .conn()
                    .query_row(
                        "SELECT model_slot FROM agents WHERE id=?1",
                        [agent_id],
                        |r| r.get(0),
                    )
                    .ok();
                crate::provider_config::caps_for_slot(slot.as_deref().unwrap_or("default"))
            },
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
            .map_err(|_| ApiError::NoRole(format!("{} agent 不在团队", crate::policydev::ROLE)))?;
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
                self.request_install(desc)?;
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
        self.run_turn_opts(role, input, &[], false)
    }

    /// plan_first=true 时回合先发不阻塞方案消息再进工具循环（US15 快速通道）。
    /// 票 03：`attachments` 是负责人随消息贴的图片引用（.hexagon/inbox/ 内），
    /// vision 槽注入 Image 块，否则降级为路径文本。
    fn run_turn_opts(
        &self,
        role: &str,
        input: &str,
        attachments: &[crate::trace::AttachRef],
        plan_first: bool,
    ) -> Result<TurnOutcome, ApiError> {
        // 握手还在进行时，这一回合等它结束再拿工具清单。打开项目本身不等。
        self.ensure_mcp_for_turn();
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
        // e2e_live 活测实证：主回合曾恒传 vec![]——agent 不知道自己的
        // 职责/归属 globs/已授技能/本阶段 due kind，模型只能猜，产物
        // kind 不声明（落成 misc 不计交付）、写盘出归属线触发权限打转。
        // RoleDef 层补齐身份面；格式细节仍走技能（load_skill 自助取）。
        let mut turn_layers = Vec::new();
        if let Ok(def) = crate::roles::role_def(&self.db, &self.project_id, role) {
            let mut text = format!("你是「{role}」。{}", def.duty);
            if !def.globs.is_empty() {
                text += &format!(
                    " 你的归属路径：{}——写盘只写范围内；范围外写入会排队等负责人批准。",
                    def.globs.join("、")
                );
            }
            if !def.skills.is_empty() {
                text += &format!(
                    " 已授技能：{}（与任务相关时先 load_skill 取全文再动手）。",
                    def.skills.join("、")
                );
            }
            if let Some(stage) = run.as_ref().and_then(|r| {
                self.pack
                    .as_ref()
                    .and_then(|p| p.stages.get(r.seq as usize))
            }) {
                if !stage.due.is_empty() {
                    text += &format!(
                        " 本阶段「{}」应交产物 kind：{}——用 artifact_write 交付并传 kind 参数（或产物开头三行 `---` / `kind: <kind>` / `---` 声明）；kind 不符不计入交付。",
                        stage.name,
                        stage.due.join("、")
                    );
                }
            }
            turn_layers.push(turn::PromptLayer::new(turn::LayerLevel::RoleDef, text));
        }
        // 票 09：本回合新落的消息才算「说完的内容」。水位取在模型调用之前。
        let watermark = self.message_high_water()?;
        // 票 03：delta hook 锁跨整个回合——hook 一旦挂上，所有走
        // run_turn_opts 的入口（run_turn/dispatch/撞限放行）自动流式。
        let mut guard = self.delta_hook.lock().unwrap();
        let sink = guard.as_mut().map(|h| &mut **h as &mut turn::DeltaSink<'_>);
        let run = turn::run_turn_streaming(
            &self.db,
            provider.as_ref(),
            &self.registry,
            &ctx,
            turn_layers,
            input,
            attachments,
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
        let outcome = run?;
        // 票 09：说完且没有点名下一位，再走与负责人没点名时相同的下一手。
        // 续派失败不推翻已经说完的这一回合——否则脚本耗尽会让成功的回复变成错误。
        if matches!(outcome, TurnOutcome::Finished) {
            if let Err(e) = self.continue_after_turn(role, watermark) {
                log::warn!("continue after {role}: {e}");
            }
        }
        Ok(outcome)
    }

    /// 快速通道派发（票 26）：负责人直接把任务派给一个已勾选角色，
    /// 不走阶段——产物/事件落同一 .hexagon/，stage_runs 不产生行。
    /// 休眠角色被点名即唤醒（会诊唤醒同款语义）；未勾选角色 → NoRole。
    pub fn dispatch(
        &self,
        role: &str,
        input: &str,
        attachments: &[crate::trace::AttachRef],
    ) -> Result<TurnOutcome, ApiError> {
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
        self.run_turn_opts(role, input, attachments, true)
    }

    /// 负责人发言的下一手（票 08 / 票 09 / ADR 0065）。
    ///
    /// 点名在任何档位都直接派给被点名者，不经过「先不派活」。没点名时，
    /// 项目经理做一次封闭选择；卸掉项目经理后，流程包交给当前阶段激活名单
    /// 的第一位，快速通道交给通道角色。没有这样的角色时，工作台自己写说明。
    ///
    /// 不拨阶段指针、不改流程包激活名单、不改写激活简报——那些写者
    /// 仍是 `open_stage` / 回合内核。选中的角色可以不在当前阶段名单里，
    /// 唤醒走既有 [`Self::dispatch`]（改的是 `agents.status`，不是名单）。
    ///
    /// 角色说完且没有点名下一位时，[`Self::continue_after_turn`] 再进这同一条路。
    /// 角色点名和负责人点名都派活（ADR 0069，不再看自治秩）。
    pub fn route_unnamed_owner(
        &self,
        body: &str,
        attachments: &[crate::trace::AttachRef],
    ) -> Result<UnnamedRoute, ApiError> {
        self.route_body("owner", body, attachments, true)
    }

    fn route_body(
        &self,
        speaker: &str,
        body: &str,
        attachments: &[crate::trace::AttachRef],
        from_owner: bool,
    ) -> Result<UnnamedRoute, ApiError> {
        if from_owner && crate::commands::parse_command(body).is_some() {
            return Ok(UnnamedRoute::Skipped);
        }
        let roster = self.roster()?;
        let mentions = roster_mentions(body, &roster, speaker);
        if !mentions.is_empty() {
            let rank = crate::autonomy::rank(&self.db, &self.project_id)?;
            // 负责人的点名不进封闭选择，所以也没有「先不派活」这一项。
            if !crate::pm_route::mention_wakes(from_owner, rank) {
                return Ok(UnnamedRoute::KeptInChat { roles: mentions });
            }
            for role in &mentions {
                self.dispatch(role, body, attachments)?;
            }
            return Ok(UnnamedRoute::Mentioned { roles: mentions });
        }
        if roster.iter().any(|r| r == crate::pm_route::PM_ROLE) {
            return self.closed_choice(speaker, body, attachments, from_owner, &roster);
        }
        match self.handoff_role(&roster)? {
            Some(role) if role != speaker => {
                self.dispatch(&role, body, attachments)?;
                Ok(UnnamedRoute::Dispatched {
                    role,
                    via: "fallback".into(),
                })
            }
            // 接话人就是刚说完的这位。再派一次不是下一位，是把同一回合再跑一遍。
            // 代价：再派 = 无人值守对着同一个人循环（false continue）；
            // 停 = 这一位已经接过话（false stop）。偏向停。出处：票 09。
            Some(_) => Ok(UnnamedRoute::Skipped),
            None => {
                self.write_no_receiver_note()?;
                Ok(UnnamedRoute::Noted)
            }
        }
    }

    fn closed_choice(
        &self,
        speaker: &str,
        body: &str,
        attachments: &[crate::trace::AttachRef],
        from_owner: bool,
        roster: &[String],
    ) -> Result<UnnamedRoute, ApiError> {
        let pm_id = self.agent_by_role(crate::pm_route::PM_ROLE)?;
        let decision_slot: Option<String> = self.db.conn().query_row(
            "SELECT decision_slot FROM agents WHERE id=?1",
            [&pm_id],
            |r| r.get(0),
        )?;
        let model_slot: Option<String> = self.db.conn().query_row(
            "SELECT model_slot FROM agents WHERE id=?1",
            [&pm_id],
            |r| r.get(0),
        )?;
        let decision_slot = decision_slot.filter(|s| !s.trim().is_empty());
        // 配了决策槽就精确取这个槽，不走 resolve_slot 的 default 回退——
        // 回退会让决策调用和主对话合成同一个供应商（CONTEXT「模型槽」Avoid）。
        // 槽名写了但没注册 → NoProvider，不假装没配。
        let (provider, slot_name, via) = if let Some(slot) = decision_slot {
            let p = self
                .providers
                .get(&slot)
                .cloned()
                .ok_or_else(|| ApiError::NoProvider(slot.clone()))?;
            (p, slot, "decision")
        } else {
            let slot = model_slot.unwrap_or_else(|| "default".into());
            let p = crate::provider_config::resolve_slot(&self.providers, &slot)
                .cloned()
                .ok_or_else(|| ApiError::NoProvider(slot.clone()))?;
            (p, slot, "chat")
        };
        let run = self.active_run()?;
        let stage_name = run.as_ref().map(|r| r.stage_name.clone());
        let activation = run
            .as_ref()
            .and_then(|r| {
                self.pack
                    .as_ref()
                    .and_then(|p| p.stages.get(r.seq as usize).map(|s| s.roles.clone()))
            })
            .unwrap_or_default();
        let speaker_label = if from_owner { "负责人" } else { speaker };
        let prompt = crate::pm_route::choice_prompt(
            stage_name.as_deref(),
            &activation,
            roster,
            speaker_label,
            body,
        );
        let req = crate::pm_route::choice_request(&slot_name, &prompt);
        let use_jev = provider.uses_decision_api();
        let run_id = run.as_ref().map(|r| r.id.as_str());
        // 选择也是一次模型派发：信封在调用前落，用量在成功后记到项目经理头上。
        // 不走 run_turn——那会把选择写成聊天回复。
        self.db.append_event(
            &self.project_id,
            EventKind::System,
            turn::request_envelope(0, &req, &req.messages, &[]),
            Some(&pm_id),
            run_id,
        )?;
        let resp = if use_jev {
            let state = crate::pm_route::choice_state(
                stage_name.as_deref(),
                &activation,
                speaker_label,
                body,
            );
            let mut options: Vec<(&str, &str)> =
                roster.iter().map(|r| (r.as_str(), "花名册角色")).collect();
            options.push((crate::pm_route::HOLD, "先不唤醒任何人"));
            provider
                .decide(&state, &options)
                .map_err(|e| ApiError::Decision(e.to_string()))?
        } else {
            provider
                .complete(&req)
                .map_err(|e| ApiError::Decision(e.to_string()))?
        };
        let usage_ctx = crate::tools::ToolContext {
            project_id: self.project_id.clone(),
            agent_id: pm_id.clone(),
            repo_root: self.repo_root.clone(),
            stage_run_id: run.as_ref().map(|r| r.id.clone()),
            owned_globs: vec![],
            tiers: crate::artifacts::TierMap::new(),
            sessions: Default::default(),
            caps: Default::default(),
        };
        crate::usage::record(&self.db, &usage_ctx, &slot_name, &resp.usage, 0)?;
        let raw = crate::pm_route::choice_text(&resp);
        let choice = crate::pm_route::parse_route_choice(&raw, roster);
        let mut eligible = roster.to_vec();
        eligible.push(crate::pm_route::HOLD.to_string());
        let (held, rejected, role) = match &choice {
            Some(crate::pm_route::RouteChoice::Dispatch(role)) => {
                (false, false, Some(role.clone()))
            }
            Some(crate::pm_route::RouteChoice::Hold) => (true, false, None),
            None => (false, true, None),
        };
        let mut payload = json!({
            "held": held,
            "rejected": rejected,
            "via": via,
            "role": role,
            "raw": raw,
        });
        // 拒绝的输出不是一次决策：不写 decision，免得花名册外的字符串
        // 被回放当成 chosen。接受/先不派活才落闭集决策。
        if let Some(chosen) = choice.as_ref().map(|c| match c {
            crate::pm_route::RouteChoice::Dispatch(role) => role.clone(),
            crate::pm_route::RouteChoice::Hold => crate::pm_route::HOLD.to_string(),
        }) {
            payload["choice"] = json!(chosen);
            payload["decision"] = json!({
                "kind": "pm_route",
                "eligible": eligible,
                "chosen": chosen,
                "via": via,
            });
        }
        self.db.append_event(
            &self.project_id,
            EventKind::PmRouted,
            payload,
            Some(&pm_id),
            run_id,
        )?;
        match choice {
            Some(crate::pm_route::RouteChoice::Dispatch(role)) => {
                self.dispatch(&role, body, attachments)?;
                Ok(UnnamedRoute::Dispatched {
                    role,
                    via: via.to_string(),
                })
            }
            Some(crate::pm_route::RouteChoice::Hold) => Ok(UnnamedRoute::Held {
                via: via.to_string(),
            }),
            None => Ok(UnnamedRoute::Rejected {
                raw,
                via: via.to_string(),
            }),
        }
    }

    /// 角色回合正常说完之后的下一手。链上限见 [`DISPATCH_CHAIN_CAP`]。
    fn continue_after_turn(&self, role: &str, watermark: i64) -> Result<UnnamedRoute, ApiError> {
        let Some(_guard) = ChainGuard::enter() else {
            log::warn!("dispatch chain stopped at {DISPATCH_CHAIN_CAP} after {role}");
            return Ok(UnnamedRoute::Skipped);
        };
        let aid = self.agent_by_role(role)?;
        let text = self.text_since(&aid, watermark)?;
        let body = if text.trim().is_empty() {
            "（没有可见回复）".to_string()
        } else {
            text
        };
        self.route_body(role, &body, &[], false)
    }

    fn message_high_water(&self) -> Result<i64, ApiError> {
        Ok(self.db.conn().query_row(
            "SELECT COALESCE(MAX(id), 0) FROM messages WHERE project_id=?1",
            [&self.project_id],
            |r| r.get(0),
        )?)
    }

    fn text_since(&self, author: &str, after_id: i64) -> Result<String, ApiError> {
        let mut st = self.db.conn().prepare(
            "SELECT body FROM messages
             WHERE project_id=?1 AND author=?2 AND id>?3
             ORDER BY id",
        )?;
        let rows = st.query_map(rusqlite::params![self.project_id, author, after_id], |r| {
            r.get::<_, String>(0)
        })?;
        let mut parts = Vec::new();
        for row in rows {
            let body = row?;
            if !body.is_empty() {
                parts.push(body);
            }
        }
        Ok(parts.join("\n"))
    }

    fn roster(&self) -> Result<Vec<String>, ApiError> {
        let mut st = self
            .db
            .conn()
            .prepare("SELECT role FROM agents WHERE project_id=?1 ORDER BY id")?;
        let rows = st.query_map([&self.project_id], |r| r.get(0))?;
        Ok(rows.collect::<Result<Vec<_>, _>>()?)
    }

    /// 卸掉项目经理之后的接话人。快速通道是通道角色；流程包是当前阶段
    /// 激活名单的第一位，而且这人必须还在花名册里。没有则 `None`。
    fn handoff_role(&self, roster: &[String]) -> Result<Option<String>, ApiError> {
        let (mode, fast_id): (String, Option<String>) = self.db.conn().query_row(
            "SELECT mode, fastpath_agent_id FROM projects WHERE id=?1",
            [&self.project_id],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )?;
        if mode == "fastpath" {
            let Some(id) = fast_id.filter(|s| !s.trim().is_empty()) else {
                return Ok(None);
            };
            let role: Option<String> = self
                .db
                .conn()
                .query_row(
                    "SELECT role FROM agents WHERE project_id=?1 AND id=?2",
                    rusqlite::params![self.project_id, id],
                    |r| r.get(0),
                )
                .ok();
            return Ok(role.filter(|r| roster.iter().any(|x| x == r)));
        }
        let Some(run) = self.active_run()? else {
            return Ok(None);
        };
        let Some(pack) = self.pack.as_ref() else {
            return Ok(None);
        };
        let Some(stage) = pack.stages.get(run.seq as usize) else {
            return Ok(None);
        };
        let Some(first) = stage.roles.first() else {
            return Ok(None);
        };
        if roster.iter().any(|r| r == first) {
            Ok(Some(first.clone()))
        } else {
            Ok(None)
        }
    }

    fn write_no_receiver_note(&self) -> Result<(), ApiError> {
        let run = self.active_run()?;
        self.db.append_message(
            &self.project_id,
            crate::pm_route::WORKBENCH_AUTHOR,
            crate::pm_route::NO_RECEIVER_NOTE,
            &[],
            &[],
            None,
            run.as_ref().map(|r| r.id.as_str()),
        )?;
        Ok(())
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

    /// 角色设定起草。ADR 0069：用角色起草槽；没绑则落到默认槽。
    /// 不再用这个 Agent 自己的模型槽。人确认才写回。
    pub fn draft_role_def(&self, agent_id: &str, hint: &str) -> Result<String, ApiError> {
        let slot = crate::provider_config::ROLE_DRAFT_SLOT.to_string();
        let provider = crate::provider_config::resolve_slot(&self.providers, &slot)
            .ok_or_else(|| ApiError::NoProvider(slot.clone()))?;
        let role: String =
            self.db
                .conn()
                .query_row("SELECT role FROM agents WHERE id=?1", [agent_id], |r| {
                    r.get(0)
                })?;
        let resolved = if self.providers.contains_key(&slot) {
            slot.clone()
        } else {
            "default".to_string()
        };
        let req = crate::provider::ChatRequest {
            model_slot: resolved,
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

    /// 现成仓库第一次进入工作台后的只读开场分析（票 17 / ADR 0067）。
    ///
    /// 空目录、已经跑过、上次崩溃留在 running，都直接跳过，不叫模型。
    /// 有项目经理就由项目经理写；卸掉时用票 09 的接话人。没有接话人时
    /// 工作台自己说明，不伪造角色发言，也不叫模型。
    ///
    /// 不拨阶段指针，不改 `agents.status`，不给工具，所以不能改业务文件，
    /// 也不能远程发布。草案只进 `intake_draft`，不写 `AGENTS.md`。
    /// L4 不在这条路上自动确认——确认是 [`Self::confirm_intake_brief`]。
    pub fn run_opening_intake(&self) -> Result<IntakeRun, ApiError> {
        match self.prepare_opening_intake()? {
            IntakePrepared::Finished(run) => Ok(run),
            IntakePrepared::Call { request, provider } => {
                let resp = match provider.complete(&request) {
                    Ok(resp) => resp,
                    Err(e) => {
                        let _ = self.abort_opening_intake();
                        return Err(ApiError::Turn(e.into()));
                    }
                };
                match self.commit_opening_intake(&crate::intake::response_text(&resp)) {
                    Ok(run) => Ok(run),
                    Err(e) => {
                        let _ = self.abort_opening_intake();
                        Err(e)
                    }
                }
            }
        }
    }

    /// 领走一次 pending 分析。模型调用由调用方在锁外完成，再 [`Self::commit_opening_intake`]。
    pub fn prepare_opening_intake(&self) -> Result<IntakePrepared, ApiError> {
        let status = self.intake_status()?;
        if status != crate::intake::STATUS_PENDING {
            return Ok(IntakePrepared::Finished(IntakeRun::Skipped));
        }
        let roster = self.roster()?;
        let speaker = if roster.iter().any(|r| r == crate::pm_route::PM_ROLE) {
            Some(crate::pm_route::PM_ROLE.to_string())
        } else {
            // 票 09 的接话人。没有进行中的阶段就没有「当前阶段第一位」，
            // 这里不改用流程包第 0 阶段顶上——那会跟票 09 分叉，说出一个
            // 当时并不会接话的角色。
            self.handoff_role(&roster)?
        };
        let Some(role) = speaker else {
            let claimed = self.claim_intake(crate::intake::STATUS_DONE)?;
            if !claimed {
                return Ok(IntakePrepared::Finished(IntakeRun::Skipped));
            }
            if let Err(e) = self.write_intake_note() {
                // 说明没写上就不要记成做完，否则这次开场分析再也补不回来。
                let _ = self.db.conn().execute(
                    "UPDATE projects SET opening_intake=?1 WHERE id=?2 AND opening_intake=?3",
                    rusqlite::params![
                        crate::intake::STATUS_PENDING,
                        self.project_id,
                        crate::intake::STATUS_DONE,
                    ],
                );
                return Err(e);
            }
            return Ok(IntakePrepared::Finished(IntakeRun::Noted));
        };
        let agent_id = self.agent_by_role(&role)?;
        let slot = self.speaker_chat_slot(&agent_id)?;
        let Some(provider) = crate::provider_config::resolve_slot(&self.providers, &slot).cloned()
        else {
            return Err(ApiError::NoProvider(slot));
        };
        // 决策槽只做封闭选择（票 08）。开场分析是读仓库后的说明，走主对话槽。
        // 被否决：没配主对话槽就改用 decision_slot——那会把「派给谁」的模型
        // 拿来写项目说明。
        let resolved = if self.providers.contains_key(&slot) {
            slot.clone()
        } else {
            "default".to_string()
        };
        let name = self.project_name()?;
        let cmds = crate::intake::read_commands(&self.repo_root);
        let req = crate::provider::ChatRequest {
            model_slot: resolved,
            messages: vec![
                crate::provider::Message {
                    role: crate::provider::Role::System,
                    content: vec![crate::provider::ContentBlock::Text {
                        text: crate::intake::INTAKE_PROMPT.to_string(),
                    }],
                },
                crate::provider::Message {
                    role: crate::provider::Role::User,
                    content: vec![crate::provider::ContentBlock::Text {
                        text: crate::intake::user_prompt(&self.repo_root, &name, &cmds),
                    }],
                },
            ],
            tools: vec![],
        };
        if !self.claim_intake(crate::intake::STATUS_RUNNING)? {
            return Ok(IntakePrepared::Finished(IntakeRun::Skipped));
        }
        *self.intake_speaker.lock().unwrap() = Some((role, agent_id));
        Ok(IntakePrepared::Call {
            request: req,
            provider,
        })
    }

    /// 把模型回复写成时间线上的分析。命令段以文件为准，不采用模型编的命令。
    /// 没有项目说明时把草案存进库，不写磁盘。
    pub fn commit_opening_intake(&self, model_text: &str) -> Result<IntakeRun, ApiError> {
        if self.intake_status()? != crate::intake::STATUS_RUNNING {
            *self.intake_speaker.lock().unwrap() = None;
            return Ok(IntakeRun::Skipped);
        }
        // 先放下锁再 abort。let-else 会把锁的临时守卫留到整句结束，
        // abort 再锁同一把就是自死锁。
        let speaker = self.intake_speaker.lock().unwrap().clone();
        let Some((role, agent_id)) = speaker else {
            let _ = self.abort_opening_intake();
            return Err(ApiError::BadInput("opening intake has no speaker".into()));
        };
        let cmds = crate::intake::read_commands(&self.repo_root);
        let scrubbed = crate::intake::scrub_model_text(model_text, &cmds.allowed());
        let cited = crate::intake::instruction_file(&self.repo_root).map(str::to_string);
        let draft = if cited.is_none() {
            Some(crate::intake::draft_body(
                &self.project_name()?,
                &crate::intake::about_line(&scrubbed),
                &cmds,
                &crate::intake::layout_entries(&self.repo_root),
            ))
        } else {
            None
        };
        let body =
            crate::intake::compose_timeline(&scrubbed, &cmds, cited.as_deref(), draft.as_deref());
        let run = self.active_run()?;
        self.db.append_message(
            &self.project_id,
            &agent_id,
            &body,
            &[],
            &[],
            Some(&agent_id),
            run.as_ref().map(|r| r.id.as_str()),
        )?;
        self.db.conn().execute(
            "UPDATE projects SET opening_intake=?1, intake_draft=?2 WHERE id=?3",
            rusqlite::params![
                crate::intake::STATUS_DONE,
                draft.as_deref(),
                self.project_id,
            ],
        )?;
        *self.intake_speaker.lock().unwrap() = None;
        Ok(IntakeRun::Posted {
            role,
            draft: draft.is_some(),
        })
    }

    /// 模型调用失败时把 running 收回 pending，同一次打开还可以再试。
    /// 进程崩溃来不及收回的，留在 running，再次打开不重跑。
    pub fn abort_opening_intake(&self) -> Result<(), ApiError> {
        self.db.conn().execute(
            "UPDATE projects SET opening_intake=?1 WHERE id=?2 AND opening_intake=?3",
            rusqlite::params![
                crate::intake::STATUS_PENDING,
                self.project_id,
                crate::intake::STATUS_RUNNING,
            ],
        )?;
        *self.intake_speaker.lock().unwrap() = None;
        Ok(())
    }

    /// 负责人点头：把草案写成 `AGENTS.md`。
    ///
    /// 不读自治档来决定写不写。L4 自动通过的是提案盖章和项目内安装（票 04），
    /// 不是这份草案。`run_opening_intake` 不调用这里。
    pub fn confirm_intake_brief(&self) -> Result<(), ApiError> {
        let draft: Option<String> = self.db.conn().query_row(
            "SELECT intake_draft FROM projects WHERE id=?1",
            [&self.project_id],
            |r| r.get(0),
        )?;
        let Some(draft) = draft.filter(|s| !s.trim().is_empty()) else {
            return Err(ApiError::NoIntakeDraft);
        };
        if crate::intake::instruction_file(&self.repo_root).is_some() {
            return Err(ApiError::IntakeBriefExists);
        }
        match crate::setup::write_agents_md(&self.repo_root, &draft) {
            Ok(()) => {}
            Err(crate::setup::SetupError::AgentsMdExists(_)) => {
                return Err(ApiError::IntakeBriefExists);
            }
            Err(e) => return Err(ApiError::BadInput(e.to_string())),
        }
        self.db.conn().execute(
            "UPDATE projects SET intake_draft=NULL WHERE id=?1",
            [&self.project_id],
        )?;
        Ok(())
    }

    /// 还有没有等人点头的草案。读侧用，不跑分析。
    pub fn intake_draft_pending(&self) -> Result<bool, ApiError> {
        let draft: Option<String> = self.db.conn().query_row(
            "SELECT intake_draft FROM projects WHERE id=?1",
            [&self.project_id],
            |r| r.get(0),
        )?;
        Ok(draft.is_some_and(|s| !s.trim().is_empty()))
    }

    fn intake_status(&self) -> Result<String, ApiError> {
        Ok(self.db.conn().query_row(
            "SELECT opening_intake FROM projects WHERE id=?1",
            [&self.project_id],
            |r| r.get(0),
        )?)
    }

    /// pending → `to`。返回是否领到。并发的第二次领不到。
    fn claim_intake(&self, to: &str) -> Result<bool, ApiError> {
        let n = self.db.conn().execute(
            "UPDATE projects SET opening_intake=?1 WHERE id=?2 AND opening_intake=?3",
            rusqlite::params![to, self.project_id, crate::intake::STATUS_PENDING],
        )?;
        Ok(n == 1)
    }

    fn write_intake_note(&self) -> Result<(), ApiError> {
        let run = self.active_run()?;
        self.db.append_message(
            &self.project_id,
            crate::pm_route::WORKBENCH_AUTHOR,
            crate::intake::NO_INTAKE_SPEAKER_NOTE,
            &[],
            &[],
            None,
            run.as_ref().map(|r| r.id.as_str()),
        )?;
        Ok(())
    }

    fn project_name(&self) -> Result<String, ApiError> {
        Ok(self.db.conn().query_row(
            "SELECT name FROM projects WHERE id=?1",
            [&self.project_id],
            |r| r.get(0),
        )?)
    }

    fn speaker_chat_slot(&self, agent_id: &str) -> Result<String, ApiError> {
        let slot: Option<String> = self.db.conn().query_row(
            "SELECT model_slot FROM agents WHERE id=?1",
            [agent_id],
            |r| r.get(0),
        )?;
        Ok(slot
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty())
            .unwrap_or_else(|| "default".into()))
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

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
use std::sync::atomic::{AtomicUsize, Ordering};
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

/// 回合或封闭选择在飞的计数守卫。失速监视的「回合在飞不计时」看它（ADR 0074）。
/// 计数不是布尔：派活链上回合里套着封闭选择、封闭选择里又套着回合。
struct InFlight<'a>(&'a AtomicUsize);

impl<'a> InFlight<'a> {
    fn enter(c: &'a AtomicUsize) -> Self {
        c.fetch_add(1, Ordering::SeqCst);
        Self(c)
    }
}

impl Drop for InFlight<'_> {
    fn drop(&mut self) {
        self.0.fetch_sub(1, Ordering::SeqCst);
    }
}

/// 失速监视一拍（或失速卡上一次按钮）之后发生了什么。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum StallTick {
    /// 没出手。原因码同诊断记录（in_flight / frozen / clock / owner_waits …）。
    Wait(&'static str),
    /// 无回复：对同一个 Agent 重触发了一轮。
    Retriggered { agent_id: String },
    /// 空转：项目经理的封闭选择派给了这个角色。
    Investigated { role: String },
    /// 调查请求了但没开回合（例如项目经理的槽没配上）。等满预算入卡。
    InvestigationPending,
    /// 入了失速卡。
    Carded {
        question_id: String,
        branch: crate::stallwatch::Branch,
        retry: bool,
    },
    /// 失速收场（调查里先不派活，或负责人点了「知道了」）。
    Closed,
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

/// 指令变体名——诊断记录的原因码用（diagnostic-records 票 02）。
/// 只给变体名不带参数：Override/Install 的参数是自由文本，不进记录。
fn command_name(cmd: &TextCommand) -> &'static str {
    match cmd {
        TextCommand::Rewind(_) => "rewind",
        TextCommand::Skip => "skip",
        TextCommand::Stamp => "stamp",
        TextCommand::Pause => "pause",
        TextCommand::Resume => "resume",
        TextCommand::SleepAll => "sleep",
        TextCommand::Install(_) => "install",
        TextCommand::Override(_) => "override",
    }
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
    #[error("data boundary configuration unavailable")]
    DataBoundaryUnavailable,
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
    #[error("agent role is ambiguous; select an instance: {0}")]
    AmbiguousRole(String),
    #[error("agent instance not found: {0}")]
    NoAgent(String),
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
        context: Box<crate::tools::ToolContext>,
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
    /// 激活任务清单（code-search 票 07）：Workbench 级共享——同一激活的
    /// 各回合 ctx 看到同一块板；激活边界在回合起跑线清场。
    pub tasks: crate::subagent::TaskBoard,
    /// 断网等待策略（network-resilience 票 01）：生产默认常量；
    /// 测试直接改写本字段注入毫秒级预算。
    pub wait_policy: crate::turn::WaitPolicy,
    pub call_deadline: Option<std::time::Instant>,
    pub mcp_timeout: std::time::Duration,
    /// web 搜索摘要槽（票 03）：attach/reload 时按 providers 文档重建；
    /// 测试注入替身直接写本字段。
    pub websearch: Option<Arc<dyn crate::websearch::SearchBackend>>,
    /// 语义索引嵌入器（票 02）：None = 默认本地哈希嵌入器；测试注替身。
    pub embedder: Option<Arc<dyn crate::semsearch::Embedder>>,
    /// 失速监视预算（stall-watch 票 01）：生产 60 秒；测试直接改写。
    pub stall_policy: crate::stallwatch::StallPolicy,
    /// 失速监视时钟：生产单调时钟；测试注入可拨的假钟。
    pub stall_clock: Arc<dyn crate::stallwatch::StallClock>,
    /// 监视状态只在进程内：重开项目从零计时。被否决：落库——
    /// 重开时上一次的回合早已结束，拿旧锚点一开就判失速是误报。
    stall: Mutex<crate::stallwatch::Watch>,
    turn_depth: AtomicUsize,
}

impl Drop for Workbench {
    fn drop(&mut self) {
        self.tasks.halt_all();
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
        Self::open_scoped(dir.as_ref(), name, roles, pack, true)
    }

    // Evaluation 01: an offline run must not start globally configured MCP
    // services or select the user's credential store as a side effect of open.
    fn open_scoped(
        dir: &Path,
        name: &str,
        roles: &[(String, String)],
        pack: Option<PackDef>,
        external_services: bool,
    ) -> Result<Self, ApiError> {
        let dir = dir.to_path_buf();
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
        // Reliability 21: reopening must not rewrite the evidence of an
        // interrupted policy replacement before recovery inspects it.
        let recovering_policy: bool = db.conn().query_row(
            "SELECT EXISTS(SELECT 1 FROM policy_changes WHERE project_id=?1 AND state IN ('pending','conflict'))",
            [&project_id], |r| r.get(0))?;
        if !recovering_policy && !dir.join(".hexagon/pack.active.json").exists() {
            if let Some(p) = &pack {
                p.pin(&dir)?;
            }
        }
        crate::proposals::recover_policy(&db, &dir, &project_id)?;
        let pack = if dir.join(".hexagon/pack.active.json").exists() {
            Some(PackDef::pinned(&dir)?)
        } else if recovering_policy {
            None
        } else {
            pack
        };
        crate::actions::recover(&db, &project_id)?;
        crate::usage::recover_requests(&db, &dir, &project_id)?;
        crate::artifacts::recover(&db, &dir, &project_id)?;
        // 票 37：检出上次被杀留下的中断回合（无收束的 turn_started）。
        let interrupted = orchestra::detect_interrupted(&db, &project_id)?;
        if interrupted > 0 {
            log::warn!("recovered {interrupted} interrupted run(s)");
        }
        // ui-audit-2 票 06：MCP 端到端——`.hexagon/mcp.json` 配置的服务
        // 在 open 时 spawn+握手，工具注册进统一管线（权限照常求值，
        // grants 表 mcp 授权闸门在 evaluate L0）。
        let registry = Registry::builtin();
        let specs = if external_services {
            crate::mcp::load_specs(&dir)
        } else {
            Vec::new()
        };
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
            creds: if external_services {
                crate::credentials::active()
            } else {
                Arc::new(crate::credentials::MemoryStore::default())
            },
            project_id,
            repo_root: dir,
            pack,
            delta_hook: Mutex::new(None),
            mcp_host,
            sessions: Default::default(),
            intake_speaker: Mutex::new(None),
            tasks: Default::default(),
            wait_policy: Default::default(),
            call_deadline: None,
            mcp_timeout: std::time::Duration::from_secs(120),
            websearch: None,
            embedder: None,
            stall_policy: Default::default(),
            stall_clock: Arc::new(crate::stallwatch::SystemClock),
            stall: Default::default(),
            turn_depth: AtomicUsize::new(0),
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
        // 票 03：web 搜索槽随 providers 文档热刷——配置变了即重建，
        // 未配置即 None（工具回报槽未配置而不是隐式失败）。
        self.websearch = crate::websearch::configured(self.creds.clone());
        // 决策槽绑上了就交给项目经理。槽名固定 decision，不和主对话槽合成一个。
        // 没这个角色（测试夹具）就略过。卸掉绑定且当前正指着 decision 时清空。
        let points_at_decision = self
            .agent_by_role(crate::pm_route::PM_ROLE)
            .ok()
            .and_then(|id| {
                self.db
                    .conn()
                    .query_row(
                        "SELECT decision_slot FROM agents WHERE project_id=?1 AND id=?2",
                        rusqlite::params![self.project_id, id],
                        |r| r.get::<_, Option<String>>(0),
                    )
                    .ok()
            });
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
            tasks: Default::default(),
            wait_policy: Default::default(),
            call_deadline: None,
            mcp_timeout: std::time::Duration::from_secs(120),
            websearch: None,
            embedder: None,
            stall_policy: Default::default(),
            stall_clock: Arc::new(crate::stallwatch::SystemClock),
            stall: Default::default(),
            turn_depth: AtomicUsize::new(0),
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
        let aid = self.agent_by_role(role)?;
        let slot = slot.map(str::trim).filter(|s| !s.is_empty());
        let n = self.db.conn().execute(
            "UPDATE agents SET decision_slot=?1 WHERE project_id=?2 AND id=?3",
            rusqlite::params![slot, self.project_id, aid],
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
        let jev = crate::provider_config::resolve_exact(
            &self.providers,
            crate::provider_config::JEV_SLOT,
        )
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
        let ctx = self.ctx_for(agent_id, self.active_run()?.map(|r| r.id));
        Ok(crate::experience::propose(&self.db, &ctx, lesson, read)?)
    }

    /// 激活简报里的技能目录。经验正文不在这里。
    pub fn skill_catalog(&self) -> Result<String, ApiError> {
        let loader = crate::skills::SkillLoader::new(crate::skills::skill_dirs(&self.repo_root));
        Ok(loader.catalog_text(&Default::default()).unwrap_or_default())
    }

    /// Controlled owner adoption synchronizes runtime knobs before returning.
    pub fn confirm_proposal(&mut self, question_id: &str) -> Result<String, ApiError> {
        crate::proposals::recover_policy(&self.db, &self.repo_root, &self.project_id)?;
        self.refresh_policy()?;
        let pid = crate::proposals::activate(&self.db, &self.ctx_for("owner", None), question_id)?;
        self.refresh_policy()?;
        Ok(pid)
    }

    fn refresh_policy(&mut self) -> Result<(), ApiError> {
        if self.repo_root.join(".hexagon/pack.active.json").exists() {
            self.pack = Some(PackDef::pinned(&self.repo_root)?);
        }
        Ok(())
    }

    pub fn rollback_proposal(&mut self, proposal_id: &str) -> Result<(), ApiError> {
        let author: String = self.db.conn().query_row(
            "SELECT author_agent_id FROM proposals WHERE id=?1 AND project_id=?2",
            rusqlite::params![proposal_id, self.project_id],
            |r| r.get(0),
        )?;
        let ctx = self.ctx_for(&author, None);
        crate::proposals::recover_policy(&self.db, &self.repo_root, &self.project_id)?;
        self.refresh_policy()?;
        crate::proposals::rollback(&self.db, &ctx, proposal_id)?;
        self.refresh_policy()?;
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
            let role = pv["role"].as_str().unwrap_or_default().to_string();
            let resume_id = if agree {
                let aid = match card.agent_id.as_deref() {
                    Some(id) => id.to_string(),
                    None => self.agent_by_role(&role)?,
                };
                self.instance_role(&aid)?;
                Some(aid)
            } else {
                None
            };

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
            // reliability 06: use the card owner, never another same-role peer.
            let aid = resume_id.expect("agree resolves the instance before answering");
            let out = self.run_turn_agent(
                &aid,
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
        let started = std::time::Instant::now();
        let mut st = self.db.conn().prepare(
            "SELECT id FROM agents WHERE project_id=?1 AND role=?2 ORDER BY created_at, id LIMIT 2",
        )?;
        let ids = st
            .query_map(rusqlite::params![self.project_id, role], |r| {
                r.get::<_, String>(0)
            })?
            .collect::<Result<Vec<_>, _>>()?;
        // reliability 06 / D10: rejecting ambiguity costs a selection; silently
        // picking the first instance spends another agent's authority and budget.
        let (result, reason) = match ids.as_slice() {
            [id] => (Ok(id.clone()), "unique_role"),
            [] => (Err(ApiError::NoRole(role.into())), "role_missing"),
            _ => (Err(ApiError::AmbiguousRole(role.into())), "role_ambiguous"),
        };
        crate::diag::note(
            if result.is_ok() {
                crate::diag::CLASS_JUDGE
            } else {
                crate::diag::CLASS_REJECT
            },
            result.is_err(),
            Some(&self.project_id),
            result.as_ref().ok().map(String::as_str),
            None,
            None,
            "instance_resolution",
            reason,
            started,
        );
        result
    }

    fn instance_role(&self, agent_id: &str) -> Result<String, ApiError> {
        let started = std::time::Instant::now();
        let role = self.db.conn().query_row(
            "SELECT role FROM agents WHERE project_id=?1 AND id=?2",
            rusqlite::params![self.project_id, agent_id],
            |r| r.get::<_, String>(0),
        );
        match role {
            Ok(role) => Ok(role),
            Err(rusqlite::Error::QueryReturnedNoRows) => {
                crate::diag::note(
                    crate::diag::CLASS_REJECT,
                    true,
                    Some(&self.project_id),
                    Some(agent_id),
                    None,
                    None,
                    "instance_resolution",
                    "instance_missing",
                    started,
                );
                Err(ApiError::NoAgent(agent_id.into()))
            }
            Err(e) => Err(e.into()),
        }
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
            wait: self.wait_policy,
            deadline: self.call_deadline,
            mcp_timeout: self.mcp_timeout,
            action_key: None,
            write_lease: None,
            tasks: self.tasks.clone(),
            subagent: None,
            websearch: self.websearch.clone(),
            embedder: self.embedder.clone(),
            subagent_provider: None,
            reads: Default::default(),
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
        // diagnostic-records 票 02：整句是指令 → 路由没派活，记是哪个指令
        // 接管。只记变体名——Override/Install 的参数是自由文本，不进记录。
        let started = std::time::Instant::now();
        crate::diag::note(
            crate::diag::CLASS_JUDGE,
            false,
            Some(&self.project_id),
            None,
            None,
            None,
            "route",
            &format!("command:{}", command_name(cmd)),
            started,
        );
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

    /// Explicit owner acceptance creates one linked new business attempt. It
    /// passes current permissions and is durable across duplicate clicks.
    pub fn retry_tool_action(
        &self,
        action_id: &str,
        reason: &str,
        accepts_duplicate: bool,
    ) -> Result<crate::tools::CallOutcome, ApiError> {
        let original = crate::actions::get(&self.db, &self.project_id, action_id)?;
        self.instance_role(&original.agent_id)?;
        let ctx = self.ctx_for(&original.agent_id, original.stage_run_id.clone());
        let next = crate::actions::new_owner_attempt(
            &self.db,
            &ctx,
            action_id,
            reason,
            accepts_duplicate,
        )?;
        let result = self.registry.call_with_seq(
            &self.db,
            &ctx,
            &next.tool,
            next.input,
            Some(&format!("owner-new-attempt:{action_id}")),
        );
        if let Err(error) = &result {
            crate::actions::fail_unstarted_attempt(&self.db, &ctx, &next.id, error)?;
        }
        let current = crate::actions::get(&self.db, &self.project_id, &next.id)?;
        if current.state != "pending" || current.question_id.is_some() {
            crate::actions::close_resolved_card(&self.db, &ctx, action_id)?;
        }
        Ok(result?)
    }

    /// Resolve the blocking decision, without rewriting uncertain history.
    pub fn abandon_tool_action(&self, action_id: &str, reason: &str) -> Result<(), ApiError> {
        Ok(crate::actions::abandon(
            &self.db,
            &self.project_id,
            action_id,
            reason,
        )?)
    }

    /// Owner-triggered read-only reconciliation of an uncertain action.
    pub fn reconcile_tool_action(
        &self,
        action_id: &str,
    ) -> Result<crate::tools::CallOutcome, ApiError> {
        let action = crate::actions::get(&self.db, &self.project_id, action_id)?;
        self.instance_role(&action.agent_id)?;
        let ctx = self.ctx_for(&action.agent_id, action.stage_run_id.clone());
        Ok(self.registry.reconcile_action(&self.db, &ctx, action_id)?)
    }

    /// Resume a durably authorized but provably not yet executed action.
    /// Unknown outcomes remain blocked for reconciliation (reliability 08).
    pub fn resume_tool_action(
        &self,
        action_id: &str,
    ) -> Result<crate::tools::CallOutcome, ApiError> {
        let action = crate::actions::get(&self.db, &self.project_id, action_id)?;
        self.instance_role(&action.agent_id)?;
        let ctx = self.ctx_for(&action.agent_id, action.stage_run_id.clone());
        Ok(self.registry.resume_action(&self.db, &ctx, action_id)?)
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

    /// Run exactly one instance. Sleeping/missing instances never fall back to peers.
    pub fn run_instance(&self, agent_id: &str, input: &str) -> Result<TurnOutcome, ApiError> {
        self.run_turn_agent(agent_id, input, &[], false)
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
        let aid = self.agent_by_role(role)?;
        self.run_turn_agent(&aid, input, attachments, plan_first)
    }

    /// 按确切 agent id 跑回合（票 NR-03 重触发用——恢复卡记的是 agent_id，
    /// 同名角色的其他 Agent 不该替它复工）。角色从实例读取，仅用于提示模板。
    fn run_turn_agent(
        &self,
        aid: &str,
        input: &str,
        attachments: &[crate::trace::AttachRef],
        plan_first: bool,
    ) -> Result<TurnOutcome, ApiError> {
        // reliability 06: resumed work retains its stored instance ID. Re-read
        // its current role only for templates; stale/deleted IDs cannot retarget.
        let current_role = self.instance_role(aid)?;
        let role = current_role.as_str();
        let _in_flight = InFlight::enter(&self.turn_depth);
        // 失速监视（票 01）：重触发回合记的是原指令，不是包了提示的入参。
        let instruction = self
            .watch()
            .retrigger_original
            .take()
            .unwrap_or_else(|| input.to_string());
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
        let run = self.active_run()?;
        let ctx = self.ctx_for(aid, run.as_ref().map(|r| r.id.clone()));
        let slot: Option<String> =
            self.db
                .conn()
                .query_row("SELECT model_slot FROM agents WHERE id=?1", [aid], |r| {
                    r.get(0)
                })?;
        // diagnostic-records 票 03：槽未绑 → resolve_slot 落 default 是正常回退，
        // 记 Debug 并说清哪个槽回退了。只在解析成功后记（没绑到任何东西是
        // NoProvider，不是回退）。
        let slot_name = slot.as_deref().unwrap_or("default").to_string();
        let started = std::time::Instant::now();
        let provider = crate::provider_config::resolve_slot(&self.providers, &slot_name)
            .ok_or_else(|| ApiError::NoProvider(slot.clone().unwrap_or_default()))?;
        // 子代理线程派遣要移动 provider——父回合解析出的就是子代理的模型
        // （票 04：同主对话模型，不开新槽）。Arc 克隆后注入 ctx。
        let mut ctx = ctx;
        ctx.subagent_provider = Some(provider.clone());
        if crate::provider_config::fell_back_to_default(&self.providers, &slot_name) {
            crate::diag::slot_fallback(
                Some(&self.project_id),
                Some(aid),
                run.as_ref().map(|r| r.id.as_str()),
                "turn_dispatch",
                &slot_name,
                started,
            );
        }
        // e2e_live 活测实证：主回合曾恒传 vec![]——agent 不知道自己的
        // 职责/归属 globs/已授技能/本阶段 due kind，模型只能猜，产物
        // kind 不声明（落成 misc 不计交付）、写盘出归属线触发权限打转。
        // RoleDef 层补齐身份面；格式细节仍走技能（load_skill 自助取）。
        let mut turn_layers = Vec::new();
        if let Ok(def) = crate::roles::role_def(&self.db, &self.project_id, role) {
            // 模板英文（prompt-engineering 票 07 / ADR 0071）；职责是负责人写的
            // 内容，保持原语言。角色名、阶段名、kind 逐字引用——kind 按字面
            // 比对计交付，模型把它译成英文就不计数。
            let stage = run.as_ref().and_then(|r| {
                self.pack
                    .as_ref()
                    .and_then(|p| p.stages.get(r.seq as usize))
            });
            let text = turn::prompt::role_layer_text(
                role,
                &def.duty,
                &def.globs,
                &def.skills,
                stage.map(|s| (s.name.as_str(), s.due.as_slice())),
            );
            turn_layers.push(turn::PromptLayer::new(turn::LayerLevel::RoleDef, text));
        }
        // 票 09：本回合新落的消息才算「说完的内容」。水位取在模型调用之前。
        let watermark = self.message_high_water()?;
        let fp_before = crate::stallwatch::fingerprint(&self.db, &self.project_id).ok();
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
        // 失败的回合也落地：没有可见回复就是无回复的起点。记在续派之前，
        // 续派出去的下一位会覆盖成它自己的动静。
        self.stall_note_turn(aid, role, instruction, watermark, fp_before);
        // Reliability 13/23 regression: admission refused by the budget is a
        // known stop, not an unexplained silent model. Re-triggering cannot
        // create funds and used to produce a false stall card. A new owner
        // instruction/activation or explicit turn reopens the existing watch.
        if matches!(
            &run,
            Err(turn::TurnError::Provider(
                crate::provider::ProviderError::BudgetUnavailable
            ))
        ) {
            self.watch().status = crate::stallwatch::Status::Closed;
            crate::diag::note(
                crate::diag::CLASS_REJECT,
                true,
                Some(&self.project_id),
                Some(aid),
                ctx.stage_run_id.as_deref(),
                None,
                "stall",
                "budget_blocked",
                std::time::Instant::now(),
            );
        }

        let outcome = run?;
        // 票 09：说完且没有点名下一位，再走与负责人没点名时相同的下一手。
        // 续派失败不推翻已经说完的这一回合——否则脚本耗尽会让成功的回复变成错误。
        if matches!(outcome, TurnOutcome::Finished) {
            if let Err(e) = self.continue_after_turn(aid, role, watermark) {
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
        self.dispatch_instance(&aid, input, attachments)
    }

    /// Explicit dispatch wakes only the selected instance, preserving the stage pointer.
    pub fn dispatch_instance(
        &self,
        aid: &str,
        input: &str,
        attachments: &[crate::trace::AttachRef],
    ) -> Result<TurnOutcome, ApiError> {
        let role = self.instance_role(aid)?;
        // reliability 07: unavailable model used to wake a sleeping instance and
        // record dispatch before failing. Prove dispatchability before mutation.
        let slot: Option<String> = self.db.conn().query_row(
            "SELECT model_slot FROM agents WHERE project_id=?1 AND id=?2",
            rusqlite::params![self.project_id, aid],
            |r| r.get(0),
        )?;
        let started = std::time::Instant::now();
        if crate::provider_config::resolve_slot(
            &self.providers,
            slot.as_deref().unwrap_or("default"),
        )
        .is_none()
        {
            crate::diag::note(
                crate::diag::CLASS_REJECT,
                true,
                Some(&self.project_id),
                Some(aid),
                None,
                None,
                "instance_dispatch",
                "provider_unavailable",
                started,
            );
            return Err(ApiError::NoProvider(slot.unwrap_or_default()));
        }
        let status: String =
            self.db
                .conn()
                .query_row("SELECT status FROM agents WHERE id=?1", [aid], |r| r.get(0))?;
        if status == "sleeping" {
            orchestra::write_agent_status(&self.db, &self.project_id, aid, false)?;
            self.db.append_event(
                &self.project_id,
                EventKind::AgentActivated,
                json!({"by": "dispatch"}),
                Some(aid),
                None,
            )?;
        }
        self.db.append_event(
            &self.project_id,
            EventKind::FastpathDispatched,
            json!({"role": role}),
            Some(aid),
            None,
        )?;
        // US15：动手前先发不阻塞方案消息，负责人有打断窗口
        self.run_turn_agent(aid, input, attachments, true)
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
        self.route_body("owner", None, body, attachments, true)
    }

    fn route_body(
        &self,
        speaker: &str,
        speaker_id: Option<&str>,
        body: &str,
        attachments: &[crate::trace::AttachRef],
        from_owner: bool,
    ) -> Result<UnnamedRoute, ApiError> {
        if from_owner {
            if let Some(cmd) = crate::commands::parse_command(body) {
                // diagnostic-records 票 02：整句是指令 → 不进路由，也是一支「没派」。
                crate::diag::note(
                    "判定",
                    false,
                    Some(&self.project_id),
                    None,
                    None,
                    None,
                    "route",
                    &format!("command:{}", command_name(&cmd)),
                    std::time::Instant::now(),
                );
                return Ok(UnnamedRoute::Skipped);
            }
        }
        let roster = self.roster()?;
        // reliability 07: explicit instance tokens are authoritative even when
        // stale. Never ignore a deleted token and fall through to another agent.
        let explicit: Vec<(String, String)> = crate::commands::parse_tokens(body)
            .into_iter()
            .filter_map(|t| match t {
                crate::trace::MessageToken::Mention { agent_role } => agent_role
                    .strip_suffix(']')
                    .and_then(|t| t.rsplit_once('['))
                    .map(|(role, id)| (role.to_string(), id.to_string())),
                _ => None,
            })
            .collect();
        if !explicit.is_empty() {
            // Validate the entire selection before starting any side effect.
            for (role, id) in &explicit {
                let started = std::time::Instant::now();
                let valid = self.instance_role(id)? == *role;
                crate::diag::note(
                    if valid {
                        crate::diag::CLASS_JUDGE
                    } else {
                        crate::diag::CLASS_REJECT
                    },
                    !valid,
                    Some(&self.project_id),
                    Some(id),
                    None,
                    None,
                    "instance_mention",
                    if valid {
                        "exact_instance"
                    } else {
                        "role_changed"
                    },
                    started,
                );
                if !valid {
                    return Err(ApiError::BadInput("instance mention role changed".into()));
                }
            }
            let mut dispatched = std::collections::HashSet::new();
            for (_, id) in &explicit {
                if Some(id.as_str()) != speaker_id && dispatched.insert(id.clone()) {
                    self.dispatch_instance(id, body, attachments)?;
                }
            }
        }
        let mentions: Vec<_> = roster_mentions(body, &roster, speaker)
            .into_iter()
            .filter(|role| !explicit.iter().any(|(r, _)| r == role))
            .collect();
        if mentions.is_empty() && !explicit.is_empty() {
            return Ok(UnnamedRoute::Mentioned {
                roles: explicit.into_iter().map(|(r, _)| r).collect(),
            });
        }
        if !mentions.is_empty() {
            // 负责人的点名不进封闭选择，所以也没有「先不派活」这一项。
            let started = std::time::Instant::now();
            if !crate::pm_route::mention_wakes(from_owner) {
                // diagnostic-records 票 02：提到了但没派也要看得出点的是谁。
                crate::diag::note(
                    "判定",
                    false,
                    Some(&self.project_id),
                    None,
                    None,
                    None,
                    "mention",
                    &format!("held:{}", mentions.join(",")),
                    started,
                );
                return Ok(UnnamedRoute::KeptInChat { roles: mentions });
            }
            // diagnostic-records 票 02：原因码带派给谁（dispatch:角色），
            // 只写角色名不写正文。
            crate::diag::note(
                "判定",
                false,
                Some(&self.project_id),
                None,
                None,
                None,
                "mention",
                &format!("dispatch:{}", mentions.join(",")),
                started,
            );
            for role in &mentions {
                let candidates = self.role_instances(role)?;
                if matches!(self.agent_by_role(role), Err(ApiError::AmbiguousRole(_))) {
                    if candidates.is_empty() {
                        return Err(ApiError::NoProvider(role.clone()));
                    }
                    if roster.iter().any(|r| r == crate::pm_route::PM_ROLE) {
                        let result = self.closed_choice(
                            speaker,
                            body,
                            attachments,
                            from_owner,
                            &roster,
                            Some(&candidates),
                        )?;
                        if !matches!(result, UnnamedRoute::Dispatched { .. }) {
                            return Ok(result);
                        }
                    } else {
                        let names = candidates
                            .iter()
                            .map(|(id, r)| format!("@{r}[{id}]"))
                            .collect::<Vec<_>>()
                            .join(" ");
                        crate::diag::note(
                            crate::diag::CLASS_JUDGE,
                            false,
                            Some(&self.project_id),
                            None,
                            None,
                            None,
                            "instance_mention",
                            "owner_selection_required",
                            started,
                        );
                        let note = crate::owner_text::choose_instance(&names);
                        self.db.append_message(
                            &self.project_id,
                            crate::pm_route::WORKBENCH_AUTHOR,
                            &note,
                            &[],
                            &[],
                            None,
                            self.active_run()?.as_ref().map(|r| r.id.as_str()),
                        )?;
                        return Ok(UnnamedRoute::Noted);
                    }
                } else {
                    self.dispatch(role, body, attachments)?;
                }
            }
            return Ok(UnnamedRoute::Mentioned { roles: mentions });
        }
        // 建档后 stage_runs 为空（pending 只是读模型）。负责人没点名的开场白
        // 若直接做选择，局面是「没有进行中的阶段」，模型回先不派活，时间线上
        // 没有角色回复，接着失速收场（owner 2026-09-25：让我们开始吧）。
        // 先打开第一阶段，选择仍先不派活时交给该阶段激活名单的第一位。
        let kickoff = self.maybe_open_first_stage(from_owner)?;
        if roster.iter().any(|r| r == crate::pm_route::PM_ROLE) {
            let route =
                self.closed_choice(speaker, body, attachments, from_owner, &roster, None)?;
            // ADR 0074：正常派活时的「先不派活」仍是空转信号。调查里的先不派活
            // 不走这里（stall_investigate 直调 closed_choice 并就地收场）。
            if matches!(route, UnnamedRoute::Held { .. }) {
                if kickoff {
                    if let Some(role) = self.kickoff_lead(&roster) {
                        self.dispatch(&role, body, attachments)?;
                        return Ok(UnnamedRoute::Dispatched {
                            role,
                            via: "decision".into(),
                        });
                    }
                }
                self.stall_note_held(from_owner.then_some(body));
            }
            return Ok(route);
        }
        let handoff = std::time::Instant::now();
        match self.handoff_instance(&roster)? {
            Some((aid, role)) if Some(aid.as_str()) != speaker_id => {
                crate::diag::note(
                    crate::diag::CLASS_JUDGE,
                    false,
                    Some(&self.project_id),
                    None,
                    None,
                    None,
                    "route",
                    &format!("handoff:{role}"),
                    handoff,
                );
                self.dispatch_instance(&aid, body, attachments)?;
                Ok(UnnamedRoute::Dispatched {
                    role,
                    via: "fallback".into(),
                })
            }
            // 接话人就是刚说完的这位。再派一次不是下一位，是把同一回合再跑一遍。
            // 代价：再派 = 无人值守对着同一个人循环（false continue）；
            // 停 = 这一位已经接过话（false stop）。偏向停。出处：票 09。
            Some((_aid, role)) => {
                crate::diag::note(
                    crate::diag::CLASS_JUDGE,
                    false,
                    Some(&self.project_id),
                    None,
                    None,
                    None,
                    "route",
                    &format!("same_speaker:{role}"),
                    handoff,
                );
                Ok(UnnamedRoute::Skipped)
            }
            None => {
                crate::diag::note(
                    crate::diag::CLASS_JUDGE,
                    false,
                    Some(&self.project_id),
                    None,
                    None,
                    None,
                    "route",
                    "no_receiver",
                    handoff,
                );
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
        instances: Option<&[(String, String)]>,
    ) -> Result<UnnamedRoute, ApiError> {
        let _in_flight = InFlight::enter(&self.turn_depth);
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
            let started = std::time::Instant::now();
            crate::diag::note(
                "槽位",
                false,
                Some(&self.project_id),
                Some(&pm_id),
                None,
                None,
                "pm_route",
                "fallback_chat",
                started,
            );
            let slot = model_slot.unwrap_or_else(|| "default".into());
            let started = std::time::Instant::now();
            let p = crate::provider_config::resolve_slot(&self.providers, &slot)
                .cloned()
                .ok_or_else(|| ApiError::NoProvider(slot.clone()))?;
            // diagnostic-records 票 03：主对话槽也没绑，落到了 default。
            if crate::provider_config::fell_back_to_default(&self.providers, &slot) {
                crate::diag::slot_fallback(
                    Some(&self.project_id),
                    Some(&pm_id),
                    None,
                    "pm_route",
                    &slot,
                    started,
                );
            }
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
        let speaker_label = if from_owner { "The owner" } else { speaker };
        // 不能派给自己。2026-09-24：名单里有项目经理时，他选了自己并开出写盘回合。
        // 进度不靠他写文档，阶段、产物和待决卡已经看得见。
        let choosable: Vec<String> = match instances {
            Some(instances) => instances.iter().map(|(id, _)| id.clone()).collect(),
            None => roster
                .iter()
                .filter(|r| r.as_str() != crate::pm_route::PM_ROLE)
                .cloned()
                .collect(),
        };
        let prompt = crate::pm_route::choice_prompt(
            stage_name.as_deref(),
            &activation,
            &choosable,
            speaker_label,
            body,
        );
        let prompt = if let Some(instances) = instances {
            format!("Select exactly one agent instance ID from this list: {instances:?}. Return only its ID, no tools or HOLD. The owner named this role: {body}")
        } else {
            prompt
        };
        let req = crate::pm_route::choice_request(&slot_name, &prompt);
        let use_jev = provider.uses_decision_api();
        let run_id = run.as_ref().map(|r| r.id.as_str());
        // 选择也是一次模型派发：信封在调用前落，用量在成功后记到项目经理头上。
        // 不走 run_turn——那会把选择写成聊天回复。
        let trace_id = self.db.append_event(
            &self.project_id,
            EventKind::System,
            turn::request_envelope(0, &req, &req.messages, &[]),
            Some(&pm_id),
            run_id,
        )?;
        let trace = trace_id.to_string();
        let route_started = std::time::Instant::now();
        let usage_ctx = crate::tools::ToolContext {
            project_id: self.project_id.clone(),
            agent_id: pm_id.clone(),
            repo_root: self.repo_root.clone(),
            stage_run_id: run.as_ref().map(|r| r.id.clone()),
            owned_globs: vec![],
            tiers: crate::artifacts::TierMap::new(),
            sessions: Default::default(),
            caps: Default::default(),
            ..Default::default()
        };
        let resp = crate::usage::request(
            &self.db,
            &usage_ctx,
            &slot_name,
            "pm_route",
            provider.as_ref(),
            if use_jev { None } else { Some(&req) },
            || {
                if use_jev {
                    let state = crate::pm_route::choice_state(
                        stage_name.as_deref(),
                        &activation,
                        speaker_label,
                        body,
                    );
                    let mut options: Vec<(&str, &str)> = choosable
                        .iter()
                        .map(|r| (r.as_str(), "eligible target"))
                        .collect();
                    if instances.is_none() {
                        options.push((crate::pm_route::HOLD_TOKEN, "wake no one for now"));
                    }
                    provider.decide(&state, &options)
                } else {
                    provider.complete(&req)
                }
            },
        )
        .map_err(|e| ApiError::Decision(e.to_string()))?;
        let raw = crate::pm_route::choice_text(&resp);
        // reliability 07 / Q8: a mistaken refusal asks for selection; accepting
        // an out-of-role or stale ID starts an unauthorized instance. Fail closed.
        let choice = crate::pm_route::parse_route_choice(&raw, &choosable).filter(|c| {
            instances.is_none() || matches!(c, crate::pm_route::RouteChoice::Dispatch(id)
                if instances.is_some_and(|xs| xs.iter().any(|(aid,role)| aid == id && self.instance_role(aid).ok().as_ref() == Some(role))))
        });
        let mut eligible = choosable.clone();
        if instances.is_none() {
            eligible.push(crate::pm_route::HOLD.to_string());
        }
        let (held, rejected, role) = match &choice {
            Some(crate::pm_route::RouteChoice::Dispatch(role)) => {
                (false, false, Some(role.clone()))
            }
            Some(crate::pm_route::RouteChoice::Hold) => (true, false, None),
            None => (false, true, None),
        };
        // diagnostic-records 票 02：unparsed 记拒绝/Warn——模型回了花名册外
        // 的字符串等于这次路由被拒，关 Debug 也得看得见；dispatch 原因码带
        // 派给的角色，回答「派给谁」。
        crate::diag::note(
            if rejected { "拒绝" } else { "判定" },
            rejected,
            Some(&self.project_id),
            Some(&pm_id),
            run_id,
            Some(&trace),
            "pm_route",
            &if rejected {
                "unparsed".to_string()
            } else if held {
                "hold".into()
            } else {
                format!("dispatch:{}", role.as_deref().unwrap_or("?"))
            },
            route_started,
        );
        let mut payload = json!({
            "held": held,
            "rejected": rejected,
            "via": via,
            "role": role,
            "raw": raw,
        });
        if let Some(candidates) = instances {
            // reliability 07: role remains a role in the historical event
            // contract; the selected instance has its own field.
            let selected_agent_id = role.as_deref();
            let selected_role = candidates
                .iter()
                .find(|(id, _)| Some(id.as_str()) == selected_agent_id)
                .map(|(_, role)| role);
            payload["role"] = json!(selected_role);
            payload["agent_id"] = json!(selected_agent_id);
            payload["scope"] = json!("role_instances");
        }
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
                let role = if instances.is_some() {
                    let actual_role = self.instance_role(&role)?;
                    self.dispatch_instance(&role, body, attachments)?;
                    actual_role
                } else {
                    self.dispatch(&role, body, attachments)?;
                    role
                };
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

    /// 没有进行中的阶段时，负责人的下一句话打开流程的第一阶段。
    /// 已经开过、快速通道、或这一句来自角色续派，都不动指针。
    fn maybe_open_first_stage(&self, from_owner: bool) -> Result<bool, ApiError> {
        if !from_owner || self.active_run()?.is_some() {
            return Ok(false);
        }
        let Some(pack) = &self.pack else {
            return Ok(false);
        };
        if pack.stages.is_empty() {
            return Ok(false);
        }
        let opened = self.open_stage(0)?;
        Ok(!opened.skipped)
    }

    /// 刚打开的阶段里，花名册上的第一位。项目经理不接这手。
    fn kickoff_lead(&self, roster: &[String]) -> Option<String> {
        let stage = self.pack.as_ref()?.stages.first()?;
        stage
            .roles
            .iter()
            .find(|role| {
                role.as_str() != crate::pm_route::PM_ROLE && roster.iter().any(|r| r == *role)
            })
            .cloned()
    }

    /// 角色回合正常说完之后的下一手。链上限见 [`DISPATCH_CHAIN_CAP`]。
    fn continue_after_turn(
        &self,
        aid: &str,
        role: &str,
        watermark: i64,
    ) -> Result<UnnamedRoute, ApiError> {
        let Some(_guard) = ChainGuard::enter() else {
            log::warn!("dispatch chain stopped at {DISPATCH_CHAIN_CAP} after {role}");
            return Ok(UnnamedRoute::Skipped);
        };
        let text = self.text_since(aid, watermark)?;
        let body = if text.trim().is_empty() {
            "（没有可见回复）".to_string()
        } else {
            text
        };
        self.route_body(role, Some(aid), &body, &[], false)
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

    fn role_instances(&self, role: &str) -> Result<Vec<(String, String)>, ApiError> {
        let mut st = self.db.conn().prepare("SELECT id, role, model_slot FROM agents WHERE project_id=?1 AND role=?2 ORDER BY created_at,id")?;
        let rows = st
            .query_map(rusqlite::params![self.project_id, role], |r| {
                Ok((
                    r.get::<_, String>(0)?,
                    r.get::<_, String>(1)?,
                    r.get::<_, Option<String>>(2)?,
                ))
            })?
            .collect::<Result<Vec<_>, _>>()?;
        Ok(rows
            .into_iter()
            .filter(|(_, _, slot)| {
                crate::provider_config::resolve_slot(
                    &self.providers,
                    slot.as_deref().unwrap_or("default"),
                )
                .is_some()
            })
            .map(|(id, role, _)| (id, role))
            .collect())
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
    fn handoff_instance(&self, roster: &[String]) -> Result<Option<(String, String)>, ApiError> {
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
            return Ok(role
                .filter(|r| roster.iter().any(|x| x == r))
                .map(|role| (id, role)));
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
            Ok(Some((self.agent_by_role(first)?, first.clone())))
        } else {
            Ok(None)
        }
    }

    fn write_no_receiver_note(&self) -> Result<(), ApiError> {
        let run = self.active_run()?;
        self.db.append_message(
            &self.project_id,
            crate::pm_route::WORKBENCH_AUTHOR,
            crate::pm_route::no_receiver_note(),
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
        let instances: Vec<(String, String)> = {
            let mut st = self.db.conn().prepare(
                "SELECT id, role FROM agents WHERE project_id=?1 AND status='active' ORDER BY created_at, id")?;
            let rows = st
                .query_map([&self.project_id], |r| Ok((r.get(0)?, r.get(1)?)))?
                .collect::<Result<_, _>>()?;
            rows
        };
        let mut out = Vec::new();
        for (aid, role) in instances {
            out.push((role.clone(), self.run_turn_agent(&aid, input, &[], false)?));
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
    pub fn request_acceptance_exception(
        &self,
        expected: &str,
    ) -> Result<orchestra::ExceptionRequest, ApiError> {
        Ok(orchestra::request_acceptance_exception(
            &self.db,
            &self.project_id,
            self.pack()?,
            expected,
        )?)
    }
    pub fn accept_delivery_exception(
        &self,
        question: &str,
        expected: &str,
        selected: &[orchestra::ExceptionRequirement],
        reason: &str,
    ) -> Result<orchestra::ExceptionAcceptance, ApiError> {
        Ok(orchestra::accept_delivery_exception(
            &self.db,
            &self.project_id,
            self.pack()?,
            question,
            expected,
            selected,
            reason,
        )?)
    }
    pub fn stage_evidence(&self) -> Result<Option<orchestra::StageEvidence>, ApiError> {
        Ok(orchestra::stage_evidence(
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

    /// Reliability 19: legacy kind-only decisions cannot authorize delivery.
    pub fn skip_review(&self, _artifact_kind: &str) -> Result<(), ApiError> {
        Err(orchestra::OrchError::InvalidAcceptanceException.into())
    }

    pub fn cancel_acceptance_exception(&self, question: &str) -> Result<(), ApiError> {
        Ok(orchestra::cancel_acceptance_exception(
            &self.db,
            &self.project_id,
            question,
        )?)
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

    /// 恢复中断的阶段 run（票 37）：负责人按「继续」重激活。
    /// 票 NR-03：恢复即重触发——恢复卡携的归属 agent 自动起新回合；
    /// 原始指令上下文靠「游标没推进」自然重读（首个模型响应没到，
    /// 简报 cursor 未消费，原指令仍待在 unread 段）。卡无 agent
    /// （悬空 run）→ 解锁但不重启。重触发失败只留 warn——恢复本身
    /// 已完成，不回滚解锁状态。
    pub fn recover_run(&self, run_id: &str) -> Result<(), ApiError> {
        // 先读卡再恢复——answer_queued_where 销卡后归属就读不到了。
        let agent = orchestra::recovery_agent(&self.db, &self.project_id, run_id)?;
        orchestra::recover_run(&self.db, &self.project_id, run_id)?;
        if let Some(aid) = agent {
            match self
                .db
                .conn()
                .query_row("SELECT role FROM agents WHERE id=?1", [&aid], |r| {
                    r.get::<_, String>(0)
                }) {
                Ok(_) => {
                    // 按卡上 agent_id 精确重触发（同名角色的别的 Agent 不替班）。
                    if let Err(e) =
                        self.run_turn_agent(&aid, crate::turn::RECOVERY_NUDGE, &[], false)
                    {
                        log::warn!("retrigger after recover failed: agent={aid} run={run_id}: {e}");
                    }
                }
                Err(e) => {
                    log::warn!("retrigger after recover skipped: agent {aid} role lookup: {e}")
                }
            }
        }
        Ok(())
    }

    /// 角色设定起草。ADR 0069：用角色起草槽；没绑则落到默认槽。
    /// 不再用这个 Agent 自己的模型槽。人确认才写回。
    ///
    /// 回退只发生在 `resolve_slot`。请求上的槽名仍是角色起草槽，不再在这里
    /// 改写成 `default`。正常回退记 Debug。
    pub fn draft_role_def(&self, agent_id: &str, hint: &str) -> Result<String, ApiError> {
        let slot = crate::provider_config::ROLE_DRAFT_SLOT.to_string();
        let started = std::time::Instant::now();
        if crate::provider_config::fell_back_to_default(&self.providers, &slot) {
            crate::diag::slot_fallback(
                Some(&self.project_id),
                Some(agent_id),
                None,
                "role_draft",
                &slot,
                started,
            );
        }
        let provider = crate::provider_config::resolve_slot(&self.providers, &slot)
            .ok_or_else(|| ApiError::NoProvider(slot.clone()))?;
        let role: String =
            self.db
                .conn()
                .query_row("SELECT role FROM agents WHERE id=?1", [agent_id], |r| {
                    r.get(0)
                })?;
        let req = crate::provider::ChatRequest {
            model_slot: slot,
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
        let ctx = self.ctx_for(agent_id, None);
        let resp = crate::usage::request(
            &self.db,
            &ctx,
            &req.model_slot,
            "role_draft",
            provider.as_ref(),
            Some(&req),
            || provider.complete(&req),
        )
        .map_err(turn::TurnError::from)?;
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
            IntakePrepared::Call {
                request,
                provider,
                context,
            } => {
                let resp = match crate::usage::request(
                    &self.db,
                    &context,
                    &request.model_slot,
                    "opening_intake",
                    provider.as_ref(),
                    Some(&request),
                    || provider.complete(&request),
                ) {
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
            Some((
                self.agent_by_role(crate::pm_route::PM_ROLE)?,
                crate::pm_route::PM_ROLE.to_string(),
            ))
        } else {
            // 票 09 的接话人。没有进行中的阶段就没有「当前阶段第一位」，
            // 这里不改用流程包第 0 阶段顶上——那会跟票 09 分叉，说出一个
            // 当时并不会接话的角色。
            self.handoff_instance(&roster)?
        };
        let Some((agent_id, role)) = speaker else {
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
        let slot = self.speaker_chat_slot(&agent_id)?;
        let started = std::time::Instant::now();
        let Some(provider) = crate::provider_config::resolve_slot(&self.providers, &slot).cloned()
        else {
            return Err(ApiError::NoProvider(slot));
        };
        // diagnostic-records 票 03：接话人的主对话槽没绑，落到了 default。
        if crate::provider_config::fell_back_to_default(&self.providers, &slot) {
            crate::diag::slot_fallback(
                Some(&self.project_id),
                Some(&agent_id),
                None,
                "intake",
                &slot,
                started,
            );
        }
        // 决策槽只做封闭选择（票 08）。开场分析是读仓库后的说明，走主对话槽。
        // 被否决：没配主对话槽就改用 decision_slot——那会把「派给谁」的模型
        // 拿来写项目说明。
        let resolved = if crate::provider_config::fell_back_to_default(&self.providers, &slot) {
            "default".to_string()
        } else {
            slot.clone()
        };
        let name = self.project_name()?;
        let cmds = crate::intake::read_commands(&self.repo_root);
        let req = crate::provider::ChatRequest {
            model_slot: resolved,
            messages: vec![
                crate::provider::Message {
                    role: crate::provider::Role::System,
                    content: vec![crate::provider::ContentBlock::Text {
                        text: crate::intake::intake_prompt(),
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
        let context = Box::new(self.ctx_for(&agent_id, None));
        *self.intake_speaker.lock().unwrap() = Some((role, agent_id));
        Ok(IntakePrepared::Call {
            request: req,
            provider,
            context,
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
            crate::intake::no_intake_speaker_note(),
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

    // ---------- 失速监视（stall-watch 票 01–04 / ADR 0074） ----------
    //
    // 监视挂在工作台而不是角色上：没有任何 Agent 发言时它也得跑（ADR 0074）。
    // 壳层后台线程定时调 stall_tick；工作台锁被回合占着时那一拍直接跳过，
    // 核内再用 turn_depth 兜一层——两层都是「回合在飞不计时」。

    fn watch(&self) -> std::sync::MutexGuard<'_, crate::stallwatch::Watch> {
        self.stall.lock().unwrap_or_else(|e| e.into_inner())
    }

    /// 测试接缝：假装有回合在飞。
    #[cfg(test)]
    pub(crate) fn hold_in_flight(&self) -> impl Drop + '_ {
        InFlight::enter(&self.turn_depth)
    }

    fn stall_note_turn(
        &self,
        aid: &str,
        role: &str,
        instruction: String,
        watermark: i64,
        fp_before: Option<crate::stallwatch::Fingerprint>,
    ) {
        use crate::stallwatch::{Last, Seen, Segment};
        let replied = self
            .text_since(aid, watermark)
            .map(|t| !t.trim().is_empty())
            .unwrap_or(false);
        let snap = crate::stallwatch::fingerprint(&self.db, &self.project_id).unwrap_or_default();
        let progressed = fp_before.as_ref().is_some_and(|b| *b != snap);
        // 交了产物、盖了章、起了卡都是看得见的动静——不按「无回复」重触发。
        let last = if replied || progressed {
            Last::Replied { progressed }
        } else {
            Last::Silent {
                agent_id: aid.to_string(),
                role: role.to_string(),
                instruction: instruction.clone(),
            }
        };
        let at = self.stall_clock.now();
        let marker = crate::stallwatch::marker(&self.db, &self.project_id).ok();
        let mut w = self.watch();
        // 收场后的新回合把监视重新打开；并把 marker 钉到当下，免得
        // 下一拍 stall_tick 把刚记下的动静当成「新一轮边界」清掉。
        if w.status == crate::stallwatch::Status::Closed {
            w.reset_episode();
        }
        if progressed {
            w.seg = Segment::default();
        }
        w.instruction = instruction;
        w.last = Some(Seen { last, at, snap });
        if let Some(m) = marker {
            w.marker = Some(m);
        }
    }

    fn stall_note_held(&self, owner_body: Option<&str>) {
        use crate::stallwatch::{Last, Seen};
        let snap = crate::stallwatch::fingerprint(&self.db, &self.project_id).unwrap_or_default();
        let at = self.stall_clock.now();
        let marker = crate::stallwatch::marker(&self.db, &self.project_id).ok();
        let mut w = self.watch();
        if w.status == crate::stallwatch::Status::Closed {
            w.reset_episode();
        }
        if let Some(b) = owner_body {
            w.instruction = b.to_string();
        }
        w.last = Some(Seen {
            last: Last::Held,
            at,
            snap,
        });
        if let Some(m) = marker {
            w.marker = Some(m);
        }
    }

    /// 失速监视一拍。壳层定时调；测试配假钟直接调。
    /// 三支都不拨阶段指针、不盖章、不远程发布——动作只有重触发、
    /// 唤醒项目经理做封闭选择、入失速卡、写工作台注记。
    pub fn stall_tick(&self) -> Result<StallTick, ApiError> {
        use crate::stallwatch::{self as sw, Last, LastKind, Obs, Segment, Status, Verdict};
        if self.turn_depth.load(Ordering::SeqCst) > 0 {
            return Ok(StallTick::Wait("in_flight"));
        }
        let started = std::time::Instant::now();
        let now = self.stall_clock.now();
        let marker = sw::marker(&self.db, &self.project_id)?;
        let frozen = sw::frozen(&self.db, &self.project_id)?;
        let fp = sw::fingerprint(&self.db, &self.project_id)?;
        let owner_waits = sw::owner_waits(&self.db, &self.project_id)?;
        let review_rework = sw::review_rework(&self.db, &self.project_id)?;
        let has_pm = self.roster()?.iter().any(|r| r == crate::pm_route::PM_ROLE);
        let mut w = self.watch();
        if w.marker.as_ref() != Some(&marker) {
            // 第一拍只记边界，不当作「新一轮」。
            // 回合记账会把 marker 钉到当下，所以刚记下的动静不会被这里清掉。
            if w.marker.is_some() {
                w.reset_episode();
            }
            w.marker = Some(marker);
        }
        if frozen {
            w.frozen_seen = true;
        } else if w.frozen_seen {
            // 暂停期间时钟不走：恢复那一拍重新起算。
            w.frozen_seen = false;
            if let Some(s) = w.last.as_mut() {
                s.at = now;
            }
            if let Some(t) = w.investigation_pending_since.as_mut() {
                *t = now;
            }
        }
        if w.status == Status::Open {
            let moved = w.last.as_ref().is_some_and(|s| s.snap != fp);
            if moved {
                // 上次动静之后有进度（例如负责人从控制通道盖了章）：新的一段。
                if let Some(s) = w.last.as_mut() {
                    s.snap = fp.clone();
                    s.at = now;
                    s.last = Last::Replied { progressed: true };
                }
                w.seg = Segment::default();
                w.investigation_pending_since = None;
            }
        }
        let (last_kind, silent) = match w.last.as_ref() {
            None => (LastKind::None, None),
            Some(s) => (
                s.last.kind(),
                match &s.last {
                    Last::Silent {
                        agent_id,
                        role,
                        instruction,
                    } => Some((agent_id.clone(), role.clone(), instruction.clone())),
                    _ => None,
                },
            ),
        };
        let speaker_active = match &silent {
            Some((aid, _, _)) => {
                self.db
                    .conn()
                    .query_row("SELECT status FROM agents WHERE id=?1", [aid], |r| {
                        r.get::<_, String>(0)
                    })
                    .ok()
                    .as_deref()
                    == Some("active")
            }
            None => false,
        };
        let obs = Obs {
            in_flight: false,
            frozen,
            owner_waits,
            review_rework,
            subagent_pending: self.tasks.any_running_subagent(),
            speaker_active,
            has_pm,
            elapsed: w
                .last
                .as_ref()
                .map(|s| now.saturating_duration_since(s.at))
                .unwrap_or_default(),
            investigation_pending: w
                .investigation_pending_since
                .map(|t| now.saturating_duration_since(t)),
            budget: self.stall_policy.budget,
        };
        match sw::judge(&obs, last_kind, w.seg, w.status) {
            Verdict::Wait(reason) => Ok(StallTick::Wait(reason)),
            Verdict::Retrigger => {
                let Some((aid, role, instruction)) = silent else {
                    return Ok(StallTick::Wait("no_speaker"));
                };
                w.seg.retriggered = true;
                drop(w);
                self.stall_retrigger(&aid, &role, &instruction, "watch", started)
            }
            Verdict::Investigate => {
                w.seg.investigated = true;
                drop(w);
                self.stall_investigate("watch", started)
            }
            Verdict::Card { branch, retry } => {
                let speaker = silent.map(|(aid, role, _)| (aid, role));
                let instruction = w.instruction.clone();
                drop(w);
                self.stall_card(branch, retry, speaker, &instruction, has_pm, started)
            }
        }
    }

    /// 无回复：对同一个 Agent 新开一回合，和恢复卡「继续」同一条路
    /// （`run_turn_agent`，按确切 id）。不新落负责人消息——原指令只进简报。
    fn stall_retrigger(
        &self,
        aid: &str,
        _role: &str,
        instruction: &str,
        by: &str,
        started: std::time::Instant,
    ) -> Result<StallTick, ApiError> {
        let run = self.active_run()?;
        let run_id = run.as_ref().map(|r| r.id.as_str());
        self.db.append_event(
            &self.project_id,
            EventKind::System,
            json!({"kind": "stall_retrigger", "branch": "no_reply", "by": by}),
            Some(aid),
            run_id,
        )?;
        crate::diag::note(
            crate::diag::CLASS_JUDGE,
            false,
            Some(&self.project_id),
            Some(aid),
            run_id,
            None,
            "stall",
            &format!("retrigger:no_reply:{by}"),
            started,
        );
        self.watch().retrigger_original = Some(instruction.to_string());
        if let Err(e) = self.run_turn_agent(
            aid,
            &crate::stallwatch::retrigger_input(instruction),
            &[],
            false,
        ) {
            // 回合没跑起来也不回滚记账：下一拍仍无回复就入卡，不第三次自动重试。
            log::warn!("stall retrigger failed: agent={aid}: {e}");
        }
        Ok(StallTick::Retriggered {
            agent_id: aid.to_string(),
        })
    }

    /// 空转：唤醒项目经理做封闭选择（派给花名册里的一个角色，或先不派活）。
    /// 派活沿用项目经理既有路由 `closed_choice`；这里只收三种结局。
    fn stall_investigate(
        &self,
        by: &str,
        started: std::time::Instant,
    ) -> Result<StallTick, ApiError> {
        let run = self.active_run()?;
        let run_id = run.as_ref().map(|r| r.id.clone());
        let pm_id = self.agent_by_role(crate::pm_route::PM_ROLE).ok();
        let high_water: i64 = self.db.conn().query_row(
            "SELECT COALESCE(MAX(id),0) FROM events WHERE project_id=?1",
            [&self.project_id],
            |r| r.get(0),
        )?;
        self.db.append_event(
            &self.project_id,
            EventKind::System,
            json!({"kind": "stall_investigation", "branch": "idle_spin", "by": by}),
            pm_id.as_deref(),
            run_id.as_deref(),
        )?;
        crate::diag::note(
            crate::diag::CLASS_JUDGE,
            false,
            Some(&self.project_id),
            pm_id.as_deref(),
            run_id.as_deref(),
            None,
            "stall",
            &format!("investigate:idle_spin:{by}"),
            started,
        );
        let roster = self.roster()?;
        let instruction = self.watch().instruction.clone();
        let body = crate::stallwatch::investigation_body(self.stall_policy.budget, &instruction);
        let res = self.closed_choice(
            crate::stallwatch::INVESTIGATION_SPEAKER,
            &body,
            &[],
            false,
            &roster,
            None,
        );
        let retry = !self.watch().seg.retry_used;
        match res {
            Ok(UnnamedRoute::Dispatched { role, .. }) => Ok(StallTick::Investigated { role }),
            Ok(UnnamedRoute::Held { .. }) => {
                self.stall_close("hold", crate::owner_text::stall_hold_closed())?;
                Ok(StallTick::Closed)
            }
            // 回合结束却没有封闭选择：立刻入卡，不另等预算（票 04）。
            Ok(_) => self.stall_card(
                crate::stallwatch::Branch::InvestigationTimeout,
                retry,
                None,
                &instruction,
                true,
                started,
            ),
            Err(e) => {
                // 三种可能：选了但被派的回合出错（调查本身已有结论）；
                // 开了回合但没有结论（立刻入卡）；根本没开回合（等满预算再入卡）。
                let chose: Option<String> = self
                    .db
                    .conn()
                    .query_row(
                        "SELECT json_extract(payload,'$.role') FROM events
                         WHERE project_id=?1 AND id>?2 AND kind='pm_routed'
                           AND json_extract(payload,'$.choice') IS NOT NULL
                         ORDER BY id LIMIT 1",
                        rusqlite::params![self.project_id, high_water],
                        |r| r.get(0),
                    )
                    .ok()
                    .flatten();
                if let Some(role) = chose {
                    log::warn!("stall investigation dispatched {role} but the turn failed: {e}");
                    return Ok(StallTick::Investigated { role });
                }
                let opened: i64 = self.db.conn().query_row(
                    "SELECT COUNT(*) FROM events
                     WHERE project_id=?1 AND id>?2 AND kind='system'
                       AND json_extract(payload,'$.kind')='request_envelope'",
                    rusqlite::params![self.project_id, high_water],
                    |r| r.get(0),
                )?;
                if opened > 0 {
                    log::warn!("stall investigation ended without a choice: {e}");
                    return self.stall_card(
                        crate::stallwatch::Branch::InvestigationTimeout,
                        retry,
                        None,
                        &instruction,
                        true,
                        started,
                    );
                }
                log::warn!("stall investigation did not open: {e}");
                self.watch().investigation_pending_since = Some(self.stall_clock.now());
                Ok(StallTick::InvestigationPending)
            }
        }
    }

    /// 入失速卡 + 工作台注记。自治不代点：卡种 stall 不在任何放行面上。
    fn stall_card(
        &self,
        branch: crate::stallwatch::Branch,
        retry: bool,
        speaker: Option<(String, String)>,
        instruction: &str,
        has_pm: bool,
        started: std::time::Instant,
    ) -> Result<StallTick, ApiError> {
        use crate::stallwatch::Branch;
        let run = self.active_run()?;
        let run_id = run.as_ref().map(|r| r.id.clone());
        let pm = self
            .agent_by_role(crate::pm_route::PM_ROLE)
            .ok()
            .map(|id| (id, crate::pm_route::PM_ROLE.to_string()));
        // 无回复卡归那位没回复的 Agent（再试一次重触发它）；另两支归项目经理。
        let (agent, role) = match branch {
            Branch::NoReply => speaker.clone().unzip(),
            _ => pm.clone().unzip(),
        };
        let note = match branch {
            Branch::NoReply => {
                crate::owner_text::stall_no_reply_card(role.as_deref().unwrap_or_default())
            }
            Branch::IdleSpin if !has_pm => crate::owner_text::stall_idle_no_pm().to_string(),
            Branch::IdleSpin => crate::owner_text::stall_idle_after_investigation().to_string(),
            Branch::InvestigationTimeout => {
                crate::owner_text::stall_investigation_timeout().to_string()
            }
        };
        let qid = crate::cards::enqueue(
            &self.db,
            &self.project_id,
            agent.as_deref(),
            crate::cards::CardKind::Stall,
            json!({
                "branch": branch.as_str(),
                "retry": retry,
                "role": role,
                "instruction": instruction,
                "run_id": run_id,
            }),
            None,
        )?;
        self.db.append_message(
            &self.project_id,
            crate::pm_route::WORKBENCH_AUTHOR,
            &note,
            &[],
            &[],
            None,
            run_id.as_deref(),
        )?;
        self.db.append_event(
            &self.project_id,
            EventKind::System,
            json!({"kind": "stall_carded", "branch": branch.as_str(),
                   "question_id": qid, "retry": retry}),
            agent.as_deref(),
            run_id.as_deref(),
        )?;
        crate::diag::note(
            crate::diag::CLASS_JUDGE,
            true,
            Some(&self.project_id),
            agent.as_deref(),
            run_id.as_deref(),
            None,
            "stall",
            &format!("card:{}", branch.as_str()),
            started,
        );
        let mut w = self.watch();
        w.status = crate::stallwatch::Status::Carded;
        w.investigation_pending_since = None;
        Ok(StallTick::Carded {
            question_id: qid,
            branch,
            retry,
        })
    }

    /// 失速收场：写工作台注记，不入卡、不再叫醒，直到新的负责人消息或新的激活。
    fn stall_close(&self, reason: &str, note: &str) -> Result<(), ApiError> {
        let run = self.active_run()?;
        let run_id = run.as_ref().map(|r| r.id.as_str());
        self.db.append_message(
            &self.project_id,
            crate::pm_route::WORKBENCH_AUTHOR,
            note,
            &[],
            &[],
            None,
            run_id,
        )?;
        self.db.append_event(
            &self.project_id,
            EventKind::System,
            json!({"kind": "stall_closed", "reason": reason}),
            None,
            run_id,
        )?;
        let mut w = self.watch();
        w.status = crate::stallwatch::Status::Closed;
        w.seg = Default::default();
        w.investigation_pending_since = None;
        Ok(())
    }

    /// 失速卡「再试一次」：只重复刚失败的那一动——无回复重触发同一个 Agent，
    /// 空转/调查超时再唤醒项目经理一轮。同一段失速只给一轮，卡上不改派。
    pub fn stall_retry(&self, qid: &str) -> Result<StallTick, ApiError> {
        use crate::stallwatch::Branch;
        let card = crate::cards::get_queued(&self.db, qid, crate::cards::CardKind::Stall)?;
        let p = &card.payload;
        if p["retry"] != json!(true) {
            return Err(ApiError::BadInput("this stall card offers no retry".into()));
        }
        let branch = p["branch"]
            .as_str()
            .and_then(Branch::from_name)
            .ok_or_else(|| ApiError::BadInput("stall card without a branch".into()))?;
        let instruction = p["instruction"].as_str().unwrap_or_default().to_string();
        crate::cards::answer(&self.db, qid, "owner")?;
        let started = std::time::Instant::now();
        let fp = crate::stallwatch::fingerprint(&self.db, &self.project_id)?;
        {
            let mut w = self.watch();
            // 销掉的这张卡不算进度，否则下一拍会把「再试一次已用」清零。
            if let Some(s) = w.last.as_mut() {
                s.snap = fp;
            }
            w.status = crate::stallwatch::Status::Open;
            w.seg.retry_used = true;
            w.investigation_pending_since = None;
            if !instruction.is_empty() {
                w.instruction = instruction.clone();
            }
            match branch {
                Branch::NoReply => w.seg.retriggered = true,
                _ => w.seg.investigated = true,
            }
        }
        match branch {
            Branch::NoReply => {
                let aid = card
                    .agent_id
                    .clone()
                    .ok_or_else(|| ApiError::BadInput("no-reply card without an agent".into()))?;
                let role: String = self.db.conn().query_row(
                    "SELECT role FROM agents WHERE id=?1",
                    [&aid],
                    |r| r.get(0),
                )?;
                self.stall_retrigger(&aid, &role, &instruction, "owner", started)
            }
            _ => self.stall_investigate("owner", started),
        }
    }

    /// 失速卡「知道了」：这一次失速收场。
    pub fn stall_ack(&self, qid: &str) -> Result<(), ApiError> {
        crate::cards::get_queued(&self.db, qid, crate::cards::CardKind::Stall)?;
        crate::cards::answer(&self.db, qid, "owner")?;
        self.stall_close("ack", crate::owner_text::stall_ack_closed())
    }

    fn pack(&self) -> Result<&PackDef, ApiError> {
        self.pack.as_ref().ok_or(ApiError::NoStage)
    }

    pub(crate) fn active_run(&self) -> Result<Option<orchestra::StageRun>, ApiError> {
        Ok(self.db.active_stage_run(&self.project_id)?)
    }
}

// 文本指令与 token 解析已迁往 `commands.rs`（中立模块，arch-review 票 01）：
// turn.rs 曾为此反向依赖本门面（诊断卡 D08）。

/// Reliability 22: disclosure is available without a project and never returns
/// credential values, raw transport configuration or authentication URLs.
pub fn data_boundary(
    root: Option<&std::path::Path>,
    store: &dyn crate::credentials::CredentialStore,
) -> Result<crate::data_boundary::DataBoundary, ApiError> {
    let doc = crate::provider_config::load().map_err(|_| ApiError::DataBoundaryUnavailable)?;
    Ok(crate::data_boundary::read(root, store, &doc))
}

#[cfg(test)]
mod stall_tests;
#[cfg(test)]
mod tests;

#[cfg(test)]
mod evaluation_config_tests;
mod evaluation_isolation;
#[cfg(test)]
mod evaluation_isolation_tests;
#[cfg(test)]
mod evaluation_plan_tests;
mod evaluation_runner;
#[cfg(test)]
mod evaluation_tests;

impl Workbench {
    /// Open a local evaluation host without inherited MCP or credentials.
    pub fn open_evaluation_host(dir: &Path) -> Result<Self, ApiError> {
        Self::open_scoped(dir, "Task evaluation", &[], None, false)
    }

    /// Freeze a host-owned batch without executing a model preflight.
    pub fn freeze_evaluation(
        &self,
        request: &crate::evaluation::FreezeRequest,
        parent: Option<&str>,
    ) -> Result<crate::evaluation::EvaluationBatch, ApiError> {
        Ok(crate::evaluation::config::freeze(
            &self.db,
            &self.project_id,
            request,
            parent,
        )?)
    }

    pub fn evaluation_batch(
        &self,
        id: &str,
    ) -> Result<crate::evaluation::EvaluationBatch, ApiError> {
        Ok(crate::evaluation::config::read(&self.db, id)?)
    }

    pub fn check_evaluation_configuration(
        &self,
        id: &str,
        request: &crate::evaluation::FreezeRequest,
    ) -> Result<crate::evaluation::EvaluationBatch, ApiError> {
        Ok(crate::evaluation::config::check(
            &self.db,
            &self.project_id,
            id,
            request,
        )?)
    }

    pub fn record_evaluation_verification(
        &self,
        id: &str,
        observation: &crate::evaluation::VerificationObservation,
    ) -> Result<crate::evaluation::EvaluationBatch, ApiError> {
        Ok(crate::evaluation::config::observe(
            &self.db,
            &self.project_id,
            id,
            observation,
        )?)
    }

    /// Host-only fixture checks, never model-benefit evidence.
    pub fn check_evaluation_category(
        &self,
        corpus: &crate::evaluation::EvaluationCorpus,
    ) -> Result<crate::evaluation::CategoryReport, ApiError> {
        Ok(crate::evaluation::check_category(corpus)?)
    }

    /// Host-only debug runner; no arbitrary provider or real-model mode is accepted.
    /// Task-benefit-evaluation 01: paid admission stays closed until 08/11/12.
    pub fn evaluate_debug(
        &self,
        task: &crate::evaluation::EvaluationTask,
        writes: &std::collections::BTreeMap<String, String>,
    ) -> Result<crate::evaluation::EvaluationResult, ApiError> {
        use crate::evaluation as eval;
        eval::self_check(task)?;
        if writes
            .keys()
            .any(|p| !eval::safe_path(p) || !task.allowed_paths.contains(p))
        {
            crate::diag::note(
                crate::diag::CLASS_REJECT,
                true,
                Some(&self.project_id),
                None,
                None,
                None,
                "evaluation_debug",
                "write_outside_task",
                std::time::Instant::now(),
            );
            return Err(ApiError::BadInput(
                "debug write outside declared task scope".into(),
            ));
        }
        let id = format!("eval-{}", self.db.next_id("evaluation")?);
        self.run_scripted_evaluation(
            task,
            &id,
            None,
            &[eval::DebugActivation {
                request_baseline_merge: false,
                role: "后端".into(),
                writes: writes.clone(),
            }],
        )
    }

    pub fn evaluation_result(
        &self,
        id: &str,
    ) -> Result<crate::evaluation::EvaluationResult, ApiError> {
        Ok(crate::evaluation::read(&self.db, id)?)
    }
}

#[cfg(test)]
mod evaluation_human_tests;

mod evaluation_human;

#[cfg(test)]
mod evaluation_outcome_tests;

mod evaluation_outcome;

mod evaluation_budget;
#[cfg(test)]
mod evaluation_budget_tests;

mod evaluation_control;
#[cfg(test)]
mod evaluation_control_tests;
mod evaluation_recovery;
#[cfg(test)]
mod evaluation_recovery_tests;

#[cfg(test)]
mod evaluation_supplement_tests;

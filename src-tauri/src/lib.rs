//! Tauri 壳：command 只做转发，业务全在 hexagon-core 的 Workbench。
//! UI 的唯一通道是这些 command + 后续的事件推送 channel，没有旁路。
//!
//! 连接模型（ADR 0052 三通道）：`wb` 锁只守 turn/mutation 组命令；
//! 读组+控制组走 `conn` 里的第二条 Db 连接——回合占着 wb 时
//! UI 读取与干预指令（pause/resume/send_message/裁决）照常落地，
//! WAL + busy_timeout(5s) 兜底单写者争用（db.rs::init）。

use hexagon_core::api::Workbench;
use hexagon_core::errcode::ErrorCode;
use hexagon_core::PROJECT_ID;
use serde_json::Value;
use std::sync::Mutex;
use tauri::{Emitter, Manager};

/// IPC 错误信封（ADR 0054，arch-review 票 06）：Tauri 把 `Err` 面序列化成
/// `{code, message}` 对象，UI 对已知 code 走 `errors.<code>` i18n、未知
/// code 渲染 message。code 取 `ErrorCode`（变体稳定标识）；壳层自身错误
/// 一律 `internal`，不混进 core 词表。
#[derive(Debug, serde::Serialize, ts_rs::TS)]
#[ts(export, export_to = "../../ui/src/gen/")]
struct CmdError {
    code: String,
    message: String,
}

impl CmdError {
    fn internal(message: impl Into<String>) -> Self {
        Self {
            code: "internal".into(),
            message: message.into(),
        }
    }
}

// 命令体里 `?`/闭包回传已信封化的错误时保持码面原样穿透。
impl std::fmt::Display for CmdError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.message)
    }
}

impl ErrorCode for CmdError {
    fn code(&self) -> String {
        self.code.clone()
    }
}

fn cmd_err<E>(e: E) -> CmdError
where
    E: ErrorCode + std::fmt::Display,
{
    CmdError {
        code: e.code(),
        message: e.to_string(),
    }
}

struct AppState {
    wb: Mutex<Option<Workbench>>,
    /// 控制通道（ADR 0052）：与 wb 内连接并存的第二条 Db 连接。
    /// root 供需要 repo_root 的读/控命令（裁决 ctx、产物回退读盘、头像）。
    conn: Mutex<Option<ControlConn>>,
}

struct ControlConn {
    db: hexagon_core::db::Db,
    root: std::path::PathBuf,
}

/// 回合 delta → webview（turn-streaming 票 03）：所有 Workbench 构造点
/// 统一挂 emit。delta 是瞬时展示通道，持久层照旧走 events/messages。
fn attach_delta_hook(app: &tauri::AppHandle, wb: &Workbench) {
    let h = app.clone();
    wb.set_turn_delta_hook(Some(Box::new(move |d| {
        let _ = h.emit("turn-delta", d);
    })));
}

/// 打开控制通道：项目库的第二连接。与 wb 内连接同参数（WAL+busy_timeout）。
fn open_control(dir: &str) -> Result<ControlConn, CmdError> {
    Ok(ControlConn {
        db: hexagon_core::db::Db::open(std::path::Path::new(dir).join(".hexagon/state.db"))
            .map_err(cmd_err)?,
        root: std::path::PathBuf::from(dir),
    })
}

/// 读/控通道（ADR 0052）：不摸 wb——回合进行中照样读写。
/// 错误面在此统一信封化（ADR 0054）：闭包产任何 `ErrorCode` 错误，
/// 出列即 `{code,message}`。
fn with_conn<R, E>(
    state: &AppState,
    f: impl FnOnce(&hexagon_core::db::Db, &std::path::Path) -> Result<R, E>,
) -> Result<R, CmdError>
where
    E: ErrorCode + std::fmt::Display,
{
    let g = state
        .conn
        .lock()
        .map_err(|_| CmdError::internal("lock poisoned"))?;
    let c = g
        .as_ref()
        .ok_or_else(|| CmdError::internal("no project open"))?;
    f(&c.db, &c.root).map_err(cmd_err)
}

fn with_wb<R, E>(
    state: &AppState,
    f: impl FnOnce(&Workbench) -> Result<R, E>,
) -> Result<R, CmdError>
where
    E: ErrorCode + std::fmt::Display,
{
    let g = state
        .wb
        .lock()
        .map_err(|_| CmdError::internal("lock poisoned"))?;
    let wb = g
        .as_ref()
        .ok_or_else(|| CmdError::internal("no project open"))?;
    f(wb).map_err(cmd_err)
}

fn with_wb_mut<R, E>(
    state: &AppState,
    f: impl FnOnce(&mut Workbench) -> Result<R, E>,
) -> Result<R, CmdError>
where
    E: ErrorCode + std::fmt::Display,
{
    let mut g = state
        .wb
        .lock()
        .map_err(|_| CmdError::internal("lock poisoned"))?;
    let wb = g
        .as_mut()
        .ok_or_else(|| CmdError::internal("no project open"))?;
    f(wb).map_err(cmd_err)
}

#[tauri::command]
fn core_ping() -> String {
    hexagon_core::ping()
}

/// 沙箱实况（agent-senses 票 06）：纯平台探测，不触 state.wb（D01-exempt）。
#[tauri::command]
fn sandbox_status() -> hexagon_core::sandbox::SandboxStatus {
    hexagon_core::sandbox::status()
}

#[tauri::command]
fn open_project(
    app: tauri::AppHandle,
    state: tauri::State<AppState>,
    dir: String,
    name: String,
    roles: Vec<(String, String)>,
    pack_json: Option<String>,
) -> Result<(), CmdError> {
    let pack = pack_json
        .map(|s| serde_json::from_str(&s))
        .transpose()
        .map_err(cmd_err)?;
    // open_with（票 05）：open → 凭据库 → 供应商注册，三处 open 序列同一入口
    let wb = Workbench::open_with(
        &dir,
        &name,
        &roles,
        pack,
        hexagon_core::credentials::active(),
    )
    .map_err(cmd_err)?;
    attach_delta_hook(&app, &wb);
    *state
        .conn
        .lock()
        .map_err(|_| CmdError::internal("lock poisoned"))? = Some(open_control(&dir)?);
    *state
        .wb
        .lock()
        .map_err(|_| CmdError::internal("lock poisoned"))? = Some(wb);
    remember_recent(&app, &dir, &name, "pack");
    Ok(())
}

#[tauri::command]
fn timeline(
    state: tauri::State<AppState>,
    after: Option<i64>,
    limit: usize,
) -> Result<Vec<hexagon_core::trace::TimelineItem>, CmdError> {
    with_conn(&state, |db, _| {
        db.timeline(PROJECT_ID, after, limit, None).map_err(cmd_err)
    })
}

#[tauri::command]
fn send_message(
    state: tauri::State<AppState>,
    body: String,
    attachments: Vec<hexagon_core::trace::AttachRef>,
) -> Result<i64, CmdError> {
    // 控制通道（ADR 0052）：路由表归 commands::send_via_control——消息落库 +
    // 暂停/恢复 就地生效；其余指令（rewind/stamp/skip/override/install）
    // 返回上来排 wb 队列。两段分开拿锁：conn→wb 不嵌套。
    // 票 03：attachments 已先经 stage_attachment 落 .hexagon/inbox/。
    let (id, cmd) = with_conn(&state, |db, _| {
        hexagon_core::commands::send_via_control(db, PROJECT_ID, &body, &attachments)
            .map_err(cmd_err)
    })?;
    if let Some(other) = cmd {
        with_wb(&state, |wb| wb.dispatch_command(&other))?;
    }
    Ok(id)
}

/// 票 03：粘贴/拖拽图片暂存 .hexagon/inbox/——魔数嗅探+尺寸闸在核内，
/// 壳层只传字节。返回引用供 send_message 落行。
#[tauri::command]
fn stage_attachment(
    state: tauri::State<AppState>,
    name: String,
    bytes: Vec<u8>,
) -> Result<hexagon_core::trace::AttachRef, CmdError> {
    with_conn(&state, |db, root| {
        hexagon_core::commands::stage_attachment(db, root, &name, &bytes)
            .map_err(|e| CmdError::internal(e.to_string()))
    })
}

/// 票 03：发送失败/取消时清掉已暂存的附件文件（best-effort）。
#[tauri::command]
fn discard_attachments(
    state: tauri::State<AppState>,
    refs: Vec<hexagon_core::trace::AttachRef>,
) -> Result<(), CmdError> {
    with_conn(&state, |_db, root| {
        hexagon_core::commands::discard_attachments(root, &refs);
        Ok::<(), CmdError>(())
    })
}

#[tauri::command]
fn answer_permission(
    state: tauri::State<AppState>,
    question_id: String,
    allow: bool,
    remember_shape: Option<String>,
    scope: String,
) -> Result<(), CmdError> {
    with_wb(&state, |wb| {
        wb.answer_permission(&question_id, allow, remember_shape.as_deref(), &scope)
    })
}

#[tauri::command]
fn advance(
    state: tauri::State<AppState>,
) -> Result<hexagon_core::orchestra::StageAction, CmdError> {
    with_wb(&state, |wb| wb.advance())
}
#[tauri::command]
fn run_checks(
    state: tauri::State<AppState>,
) -> Result<hexagon_core::orchestra::CheckOutcome, CmdError> {
    with_wb(&state, |wb| wb.run_checks())
}
#[tauri::command]
fn stamp(state: tauri::State<AppState>) -> Result<hexagon_core::orchestra::StageAction, CmdError> {
    with_wb(&state, |wb| wb.stamp())
}
#[tauri::command]
fn rewind(
    state: tauri::State<AppState>,
    to_seq: usize,
) -> Result<hexagon_core::orchestra::StageAction, CmdError> {
    with_wb(&state, |wb| wb.rewind(to_seq))
}
#[tauri::command]
fn skip(state: tauri::State<AppState>) -> Result<hexagon_core::orchestra::StageAction, CmdError> {
    with_wb(&state, |wb| wb.skip())
}
#[tauri::command]
fn skip_review(state: tauri::State<AppState>, artifact_kind: String) -> Result<(), CmdError> {
    with_wb(&state, |wb| wb.skip_review(&artifact_kind))
}
#[tauri::command]
fn pause(state: tauri::State<AppState>) -> Result<(), CmdError> {
    // 暂停按钮是回合中的叫停通道——必须走控制连接，wb 锁正被回合占着
    with_conn(&state, |db, _| {
        hexagon_core::orchestra::pause(db, PROJECT_ID).map_err(cmd_err)
    })
}
#[tauri::command]
fn resume(state: tauri::State<AppState>) -> Result<(), CmdError> {
    with_conn(&state, |db, _| {
        hexagon_core::orchestra::resume(db, PROJECT_ID).map_err(cmd_err)
    })
}
#[tauri::command]
fn sleep_all(state: tauri::State<AppState>) -> Result<(), CmdError> {
    // 全员休眠是干预指令（ADR 0052 控制组）：回合中途也要能落
    with_conn(&state, |db, _| {
        hexagon_core::orchestra::sleep_all(db, PROJECT_ID).map_err(cmd_err)
    })
}

#[tauri::command]
fn artifacts(
    state: tauri::State<AppState>,
) -> Result<Vec<hexagon_core::artifacts::ArtifactRow>, CmdError> {
    with_conn(&state, |db, _| {
        hexagon_core::artifacts::query(db, PROJECT_ID, None, None, None, None).map_err(cmd_err)
    })
}
#[tauri::command]
fn artifact_content(state: tauri::State<AppState>, path: String) -> Result<String, CmdError> {
    with_conn(&state, |db, root| {
        hexagon_core::artifacts::content(db, root, PROJECT_ID, &path).map_err(cmd_err)
    })
}
#[tauri::command]
fn team(state: tauri::State<AppState>) -> Result<Vec<hexagon_core::orchestra::TeamRow>, CmdError> {
    with_conn(&state, |db, root| {
        hexagon_core::orchestra::team(db, PROJECT_ID, root).map_err(cmd_err)
    })
}
#[tauri::command]
fn stage_status(
    state: tauri::State<AppState>,
) -> Result<Vec<hexagon_core::orchestra::StageRow>, CmdError> {
    with_conn(&state, |db, _| {
        hexagon_core::orchestra::stage_status(db, PROJECT_ID).map_err(cmd_err)
    })
}
#[tauri::command]
fn pending_questions(
    state: tauri::State<AppState>,
) -> Result<Vec<hexagon_core::cards::QueuedCard>, CmdError> {
    with_conn(&state, |db, _| {
        db.queued_questions(PROJECT_ID).map_err(cmd_err)
    })
}
/// 已记权限规则列表（ui-audit-2 票 03：设置-权限分区审计面）。
#[tauri::command]
fn permission_rules(
    state: tauri::State<AppState>,
) -> Result<Vec<hexagon_core::permissions::PermissionRuleRow>, CmdError> {
    with_conn(&state, |db, _| {
        hexagon_core::permissions::list_rules(db, PROJECT_ID).map_err(cmd_err)
    })
}

/// 撤销已记规则：false（不存在/不属本项目）报 internal 而非静默成功——
/// 「没删却说删了」比报错更糟（fail-closed 偏向显式失败）。
#[tauri::command]
fn revoke_permission_rule(state: tauri::State<AppState>, rule_id: String) -> Result<(), CmdError> {
    with_conn(&state, |db, _| -> Result<(), CmdError> {
        let ok =
            hexagon_core::permissions::revoke_rule(db, PROJECT_ID, &rule_id).map_err(cmd_err)?;
        if ok {
            Ok(())
        } else {
            Err(CmdError::internal(format!(
                "permission rule not found: {rule_id}"
            )))
        }
    })
}

/// 技能清单（ui-audit-2 票 04 → global-config 票 03）：有项目 = 全局∪项目
/// 合并视图；无项目 = 全局层（~/.hexagon/skills）。enabled 看全局静音文件。
#[tauri::command]
fn list_skills(
    state: tauri::State<AppState>,
) -> Result<Vec<hexagon_core::skills::SkillRow>, CmdError> {
    let g = state
        .conn
        .lock()
        .map_err(|_| CmdError::internal("lock poisoned"))?;
    Ok(match g.as_ref() {
        Some(c) => hexagon_core::skills::list_all(&c.root),
        None => hexagon_core::skills::list_global(),
    })
}

/// 技能包文件树（global-config 票 03 详情面）：有界列相对路径。
/// 无项目时只解析全局技能。
#[tauri::command]
fn skill_files(state: tauri::State<AppState>, name: String) -> Result<Vec<String>, CmdError> {
    let g = state
        .conn
        .lock()
        .map_err(|_| CmdError::internal("lock poisoned"))?;
    Ok(hexagon_core::skills::skill_files(
        &name,
        g.as_ref().map(|c| c.root.as_path()),
    ))
}

/// 读技能包内文件（渲染面）：core 内做逃逸检查 + 256KB 截断。
#[tauri::command]
fn read_skill_file(
    state: tauri::State<AppState>,
    name: String,
    rel: String,
) -> Result<String, CmdError> {
    let g = state
        .conn
        .lock()
        .map_err(|_| CmdError::internal("lock poisoned"))?;
    hexagon_core::skills::read_skill_file(&name, &rel, g.as_ref().map(|c| c.root.as_path()))
        .map_err(|e| CmdError::internal(e.to_string()))
}

/// 新建/覆写全局技能（无项目可用）：~/.hexagon/skills/<name>/SKILL.md。
#[tauri::command]
fn save_global_skill(name: String, description: String, body: String) -> Result<(), CmdError> {
    hexagon_core::skills::save_global_skill(&name, &description, &body)
        .map_err(|e| CmdError::internal(e.to_string()))
}

// ---------- 外部技能扫描/导入（global-config 票 04） ----------

/// 扫描主流平台本机技能目录（cursor/claude/agents/devin/windsurf/codex），
/// 按技能名去重；conflict = 与本机全局同名（导入时跳过）。
#[tauri::command]
fn scan_external_skills() -> Vec<hexagon_core::skills::ExtSkillRow> {
    hexagon_core::skills::scan_external_skills()
}

/// 批量导入外部技能目录 → ~/.hexagon/skills/。同名冲突跳过不覆盖。
#[tauri::command]
fn import_skills(paths: Vec<String>) -> Result<hexagon_core::skills::ImportReport, CmdError> {
    hexagon_core::skills::import_skills(&paths).map_err(|e| CmdError::internal(e.to_string()))
}

/// 从文件夹或 ZIP 安装技能包（返回落地的技能名）。
#[tauri::command]
fn install_skill_path(path: String) -> Result<String, CmdError> {
    hexagon_core::skills::install_skill_from_path(&path)
        .map_err(|e| CmdError::internal(e.to_string()))
}

/// composer `#` 路径补全（ui-audit-2 票 09）：仓根有界遍历，conn 通道。
#[tauri::command]
fn repo_paths(state: tauri::State<AppState>, query: String) -> Result<Vec<String>, CmdError> {
    with_conn(&state, |_db, root| {
        Ok::<_, CmdError>(hexagon_core::commands::repo_paths(root, &query, 60))
    })
}

/// MCP 服务实况（ui-audit-2 票 06）：需读运行中宿主的状态，走 wb 通道。
#[tauri::command]
fn mcp_services(
    state: tauri::State<AppState>,
) -> Result<Vec<hexagon_core::mcp::McpServiceRow>, CmdError> {
    with_wb(&state, |wb| Ok::<_, CmdError>(wb.mcp_services()))
}

// ---------- MCP 全局清单（global-config 票 05，ADR 0057） ----------

/// 全量服务清单（无项目可用）：全局 ∪ 项目（同名项目覆盖全局），
/// 含禁用与远程条目；origin 标来源。配置清单≠授权（ADR 0010）。
#[tauri::command]
fn list_mcp_entries(
    state: tauri::State<AppState>,
) -> Result<Vec<hexagon_core::mcp::McpEntryRow>, CmdError> {
    // wb 只借 repo_root 读路径，缺省=只看全局层——projectless 可列。
    let root = state
        .wb
        .lock()
        .ok()
        .and_then(|g| g.as_ref().map(|wb| wb.repo_root.clone()));
    Ok(hexagon_core::mcp::list_mcp_entries(root.as_deref()))
}

/// 新建/覆写全局 MCP 服务（upsert 按名；写 ~/.hexagon/mcp.json）。
/// 生效时机 = 重开项目（一期不热重启宿主）。
#[tauri::command]
fn save_mcp_service(spec: hexagon_core::mcp::McpSpec) -> Result<(), CmdError> {
    hexagon_core::mcp::save_global_mcp(&spec).map_err(CmdError::internal)
}

/// 删全局服务（幂等；项目层条目不受此命令影响——项目文件手改）。
#[tauri::command]
fn delete_mcp_service(name: String) -> Result<(), CmdError> {
    hexagon_core::mcp::delete_global_mcp(&name).map_err(CmdError::internal)
}

// ---------- MCP 外部扫描/导入 + 市场（global-config 票 06） ----------

/// 扫主流平台本机 MCP 配置（cursor/claude/claude-desktop/windsurf/gemini/
/// devin/codex），按 名+传输签名 去重；conflict = 与全局清单同名。
#[tauri::command]
fn scan_external_mcp() -> Vec<hexagon_core::mcp::ExtMcpRow> {
    hexagon_core::mcp::scan_external_mcp()
}

/// 批量导入勾选服务 → ~/.hexagon/mcp.json；同名跳过不覆盖。
#[tauri::command]
fn import_mcp(specs: Vec<hexagon_core::mcp::McpSpec>) -> hexagon_core::skills::ImportReport {
    hexagon_core::mcp::import_mcp(&specs)
}

/// 公共 MCP 市场（票 06）：系统默认浏览器打开。URL 是常量非用户输入——
/// 不给任意 URL 开口子（避免变成「以默认浏览器打开任意链接」原语）。
const MCP_MARKET_URL: &str = "https://mcp.higress.ai/";

#[tauri::command]
fn open_mcp_market() -> Result<(), CmdError> {
    open::that(MCP_MARKET_URL).map_err(|e| CmdError::internal(e.to_string()))
}

/// 全局技能开关（ADR 0057）：写 ~/.hexagon/skill-mutes.json——真全局文件，
/// 无项目可用；回合注入时 会话集∪遗留"*"∪全局 生效。
#[tauri::command]
fn set_skill_muted(name: String, enabled: bool) -> Result<(), CmdError> {
    hexagon_core::skills::set_globally_muted(&name, enabled)
        .map_err(|e| CmdError::internal(e.to_string()))
}

#[tauri::command]
fn usage(state: tauri::State<AppState>) -> Result<hexagon_core::usage::UsageSummary, CmdError> {
    with_conn(&state, |db, _| {
        hexagon_core::usage::project_summary(db, PROJECT_ID).map_err(cmd_err)
    })
}

#[tauri::command]
fn open_stage(
    state: tauri::State<AppState>,
    seq: usize,
) -> Result<hexagon_core::api::OpenStageOutcome, CmdError> {
    with_wb(&state, |wb| wb.open_stage(seq))
}

#[tauri::command]
fn recover_run(state: tauri::State<AppState>, run_id: String) -> Result<(), CmdError> {
    with_wb(&state, |wb| wb.recover_run(&run_id))
}

#[tauri::command]
fn override_checks(
    state: tauri::State<AppState>,
    reason: String,
) -> Result<hexagon_core::orchestra::OverrideOutcome, CmdError> {
    with_wb(&state, |wb| wb.override_checks(&reason))
}

// ui-audit-2 票 08 裁决：request_install IPC 已删——composer `/install <描述>`
// 经 TextCommand::Install → install::request_install 同函数直达，专设 IPC
// 是重复入口；owner 发起安装的规范面就是 composer。

#[tauri::command]
fn agent_detail(
    state: tauri::State<AppState>,
    agent_id: String,
) -> Result<hexagon_core::roles::AgentDetail, CmdError> {
    with_conn(&state, |db, _| {
        hexagon_core::roles::agent_detail(db, PROJECT_ID, &agent_id).map_err(cmd_err)
    })
}

#[tauri::command]
fn update_agent(
    state: tauri::State<AppState>,
    agent_id: String,
    patch: hexagon_core::roles::AgentPatch,
) -> Result<(), CmdError> {
    // 纯 DB 变更——控制通道直落（票 05：不占 wb 锁）
    with_conn(&state, |db, _| {
        hexagon_core::roles::update_agent_def(db, PROJECT_ID, &agent_id, &patch).map_err(cmd_err)
    })
}

#[tauri::command]
fn create_role(
    state: tauri::State<AppState>,
    def: hexagon_core::presets::RoleDef,
) -> Result<String, CmdError> {
    with_conn(&state, |db, _| {
        hexagon_core::roles::create_role(db, PROJECT_ID, &def).map_err(cmd_err)
    })
}

#[tauri::command]
fn set_agent_grants(
    state: tauri::State<AppState>,
    agent_id: String,
    kind: String,
    names: Vec<String>,
) -> Result<(), CmdError> {
    with_conn(&state, |db, _| {
        hexagon_core::roles::set_grants(db, &agent_id, &kind, &names).map_err(cmd_err)
    })
}

#[tauri::command]
fn draft_role_def(
    state: tauri::State<AppState>,
    agent_id: String,
    hint: String,
) -> Result<String, CmdError> {
    with_wb(&state, |wb| wb.draft_role_def(&agent_id, &hint))
}

#[tauri::command]
fn pack_draft(state: tauri::State<AppState>) -> Result<hexagon_core::orchestra::PackDef, CmdError> {
    with_conn(&state, |_, root| {
        hexagon_core::packedit::load_draft(root).map_err(cmd_err)
    })
}

#[tauri::command]
fn save_pack_draft(state: tauri::State<AppState>, pack_json: String) -> Result<(), CmdError> {
    // 草稿写 .hexagon/pack.json（非 active 副本）——控制通道可落
    with_conn(&state, |db, root| {
        let pack = hexagon_core::packedit::parse_draft(&pack_json).map_err(cmd_err)?;
        hexagon_core::packedit::save_draft(db, root, PROJECT_ID, &pack).map_err(cmd_err)
    })
}

/// 个人模板存 ~/.config/hexagon/templates/——项目无关，不需要任何项目态。
#[tauri::command]
fn save_pack_template(pack_json: String) -> Result<String, CmdError> {
    let pack = hexagon_core::packedit::parse_draft(&pack_json).map_err(cmd_err)?;
    hexagon_core::packedit::save_template(&pack)
        .map(|p| p.to_string_lossy().to_string())
        .map_err(cmd_err)
}

/// 个人模板列表：同上，项目无关。
#[tauri::command]
fn pack_templates() -> Result<Vec<String>, CmdError> {
    hexagon_core::packedit::list_templates().map_err(cmd_err)
}

/// 按名载入个人模板（ui-audit-2 票 08：PackEditor「载入模板」的数据口）。
#[tauri::command]
fn pack_template(name: String) -> Result<hexagon_core::orchestra::PackDef, CmdError> {
    hexagon_core::packedit::load_template(&name).map_err(cmd_err)
}

#[tauri::command]
fn export_pack_yaml(state: tauri::State<AppState>, dest: String) -> Result<(), CmdError> {
    with_conn(&state, |_, root| {
        hexagon_core::packedit::export_yaml(root, std::path::Path::new(&dest)).map_err(cmd_err)
    })
}

#[tauri::command]
fn resolve_install(
    state: tauri::State<AppState>,
    qid: String,
    allow: bool,
) -> Result<hexagon_core::install::InstallOutcome, CmdError> {
    // 放行/驳回 + 落盘执行全在 Db+repo_root——控制通道直落（票 05）
    with_conn(&state, |db, root| {
        hexagon_core::install::resolve_install(db, PROJECT_ID, root, &qid, allow).map_err(cmd_err)
    })
}

#[tauri::command]
fn export_events(
    state: tauri::State<AppState>,
    path: String,
    stage_run_id: Option<String>,
    agent_id: Option<String>,
    kinds: Option<Vec<String>>,
) -> Result<usize, CmdError> {
    let kinds = kinds
        .map(|ks| {
            ks.iter()
                .map(|k| {
                    serde_json::from_value::<hexagon_core::trace::EventKind>(serde_json::json!(k))
                        .map_err(cmd_err)
                })
                .collect::<Result<Vec<_>, CmdError>>()
        })
        .transpose()?;
    with_conn(&state, |db, _| {
        db.export_events(
            PROJECT_ID,
            std::path::Path::new(&path),
            &hexagon_core::trace::ExportFilter {
                stage_run_id,
                agent_id,
                kinds,
            },
        )
        .map_err(cmd_err)
    })
}

#[tauri::command]
fn autonomy(state: tauri::State<AppState>) -> Result<String, CmdError> {
    with_conn(&state, |db, _| {
        hexagon_core::autonomy::level(db, PROJECT_ID).map_err(cmd_err)
    })
}

#[tauri::command]
fn set_autonomy(state: tauri::State<AppState>, level: String) -> Result<(), CmdError> {
    // 档位是 projects 行旋钮（控制组）：回合途中改档即时落库，下回合生效
    with_conn(&state, |db, _| {
        hexagon_core::autonomy::set_level(db, PROJECT_ID, &level).map_err(cmd_err)
    })
}

/// 审查者档位读（ui-audit-2 票 07）：projects 行字段，conn 通道即可。
#[tauri::command]
fn reviewer_mode(state: tauri::State<AppState>) -> Result<String, CmdError> {
    with_conn(&state, |db, _| {
        hexagon_core::autonomy::reviewer_mode(db, PROJECT_ID).map_err(cmd_err)
    })
}

/// 审查者档位写：live 只在 autonomy ≥ L1 生效（L0+live 在裁决内退化为
/// shadow——后端 adjudicate 拦，UI 也明示），这里只校验词表。
#[tauri::command]
fn set_reviewer_mode(state: tauri::State<AppState>, mode: String) -> Result<(), CmdError> {
    with_conn(&state, |db, _| {
        hexagon_core::autonomy::set_reviewer_mode(db, PROJECT_ID, &mode).map_err(cmd_err)
    })
}

#[tauri::command]
fn owner_away(state: tauri::State<AppState>) -> Result<(), CmdError> {
    with_conn(&state, |db, _| {
        hexagon_core::autonomy::leave(db, PROJECT_ID)
            .map(|_| ())
            .map_err(cmd_err)
    })
}

#[tauri::command]
fn owner_back(
    state: tauri::State<AppState>,
) -> Result<hexagon_core::autonomy::ReturnSummary, CmdError> {
    with_conn(&state, |db, _| {
        hexagon_core::autonomy::back(db, PROJECT_ID).map_err(cmd_err)
    })
}

#[tauri::command]
fn reject_stamp(
    state: tauri::State<AppState>,
) -> Result<hexagon_core::orchestra::StageAction, CmdError> {
    with_wb(&state, |wb| wb.reject_stamp())
}

/// ADR 0052 wb 组：context_overflow 放行时内联续跑一回合——必须持 wb 锁。
/// 异步化与否见 report.md 待验证假设，本票不碰语义。
#[tauri::command]
fn adjudicate_flag(
    state: tauri::State<AppState>,
    qid: String,
    agree: bool,
) -> Result<hexagon_core::api::AdjudicateOutcome, CmdError> {
    with_wb(&state, |wb| wb.adjudicate_flag(&qid, agree))
}

#[tauri::command]
fn proposals(
    state: tauri::State<AppState>,
) -> Result<Vec<hexagon_core::proposals::ProposalRow>, CmdError> {
    with_conn(&state, |db, _| {
        hexagon_core::proposals::list(db, PROJECT_ID).map_err(cmd_err)
    })
}

/// in_review 提案的负责人裁决（ui-audit-2 票 08）：原签名要 reviewer_agent——
/// 但复审 agent 从无工具可调 review（in_review 曾是无出口死态），唯一真实
/// 裁决面是 owner，署名 owner 比冒名复审 agent 诚实。
#[tauri::command]
fn review_proposal(
    state: tauri::State<AppState>,
    proposal_id: String,
    pass: bool,
    reason: String,
) -> Result<(), CmdError> {
    with_conn(&state, |db, root| {
        let ctx = hexagon_core::tools::ToolContext::owner(db, root);
        hexagon_core::proposals::review(db, &ctx, &proposal_id, pass, &reason).map_err(cmd_err)
    })
    .map(|_| ())
}

#[tauri::command]
fn confirm_proposal(state: tauri::State<AppState>, qid: String) -> Result<String, CmdError> {
    with_conn(&state, |db, root| {
        let ctx = hexagon_core::tools::ToolContext::owner(db, root);
        hexagon_core::proposals::activate(db, &ctx, &qid).map_err(cmd_err)
    })
}

#[tauri::command]
fn reject_proposal(
    state: tauri::State<AppState>,
    qid: String,
    reason: String,
) -> Result<(), CmdError> {
    with_conn(&state, |db, root| {
        let ctx = hexagon_core::tools::ToolContext::owner(db, root);
        hexagon_core::proposals::reject_at_stamp(db, &ctx, &qid, &reason).map_err(cmd_err)
    })
}

/// 票 07：不变量伴随件手动体检入口——返回违规数（0=trace 自洽），
/// 违规明细落 invariant_violation 事件。
/// ADR 0052 wb 组：回合中段跑会把在途回合误报成悬挂回合——要静止态。
#[tauri::command]
fn invariant_check(state: tauri::State<AppState>) -> Result<usize, CmdError> {
    // 独立路径重校验 trace——纯 Db 读+违规事件写，走控制通道（票 05 移出 wb 组）
    with_conn(&state, |db, _| {
        hexagon_core::invariant::check_and_log(db, PROJECT_ID).map_err(cmd_err)
    })
}

/// 票 10：政策研发提案的确定性入口——旋钮编辑 JSON + 场景 JSON →
/// 副本改旋钮 → 回放 → 携证据+judge 判定进普通提案队列。
/// 产出物无特权通道（submit 全校验+负责人盖章不变）。
/// ADR 0052 wb 组：内联双回放要锁 wb——异步化见 report.md 待验证假设。
#[tauri::command]
fn policydev_propose(
    state: tauri::State<AppState>,
    edits: Vec<Value>,
    scenario: Value,
    motive: String,
) -> Result<String, CmdError> {
    let sc: hexagon_core::scenario::Scenario = serde_json::from_value(scenario).map_err(cmd_err)?;
    with_wb(&state, |wb| wb.policydev_propose(&edits, &sc, &motive))
}

#[tauri::command]
fn rollback_proposal(state: tauri::State<AppState>, proposal_id: String) -> Result<(), CmdError> {
    with_conn(&state, |db, root| {
        let ctx = hexagon_core::tools::ToolContext::owner(db, root);
        hexagon_core::proposals::rollback(db, &ctx, &proposal_id).map_err(cmd_err)
    })
}

#[tauri::command]
fn request_publish(state: tauri::State<AppState>, remote: String) -> Result<String, CmdError> {
    // 发布请求只是入队待决卡（控制组）；push 在 confirm_publish（wb 组，要 creds）
    with_conn(&state, |db, _| {
        hexagon_core::publish::request(db, PROJECT_ID, &remote).map_err(cmd_err)
    })
}

#[tauri::command]
fn confirm_publish(
    state: tauri::State<AppState>,
    qid: String,
) -> Result<hexagon_core::publish::PublishOutcome, CmdError> {
    with_wb(&state, |wb| wb.confirm_publish(&qid))
}

#[tauri::command]
fn reject_publish(state: tauri::State<AppState>, qid: String) -> Result<(), CmdError> {
    with_conn(&state, |db, _| {
        hexagon_core::publish::reject(db, PROJECT_ID, &qid).map_err(cmd_err)
    })
}

#[tauri::command]
fn set_agent_avatar(
    state: tauri::State<AppState>,
    agent_id: String,
    data_url: String,
) -> Result<(), CmdError> {
    // 纯文件写（avatars/ 目录）——控制通道取 root 即可
    with_conn(&state, |_, root| {
        hexagon_core::roles::set_agent_avatar(root, &agent_id, &data_url).map_err(cmd_err)
    })
}

#[tauri::command]
fn agent_avatar(
    state: tauri::State<AppState>,
    agent_id: String,
) -> Result<Option<String>, CmdError> {
    with_conn(&state, |_, root| {
        hexagon_core::roles::agent_avatar(root, &agent_id).map_err(cmd_err)
    })
}

#[tauri::command]
fn artifact_content_at(
    state: tauri::State<AppState>,
    path: String,
    version: i64,
) -> Result<Option<String>, CmdError> {
    with_conn(&state, |db, root| {
        hexagon_core::artifacts::content_at(db, root, PROJECT_ID, &path, version).map_err(cmd_err)
    })
}

#[tauri::command]
fn set_agent_sleeping(
    state: tauri::State<AppState>,
    agent_id: String,
    sleeping: bool,
) -> Result<(), CmdError> {
    // 休眠/唤醒是干预指令（ADR 0052 控制组）
    with_conn(&state, |db, _| {
        hexagon_core::orchestra::set_agent_sleeping(db, PROJECT_ID, &agent_id, sleeping)
            .map_err(cmd_err)
    })
}

#[tauri::command]
fn set_usage_limit(
    state: tauri::State<AppState>,
    limit_cents: Option<i64>,
) -> Result<(), CmdError> {
    with_conn(&state, |db, _| {
        hexagon_core::usage::set_limit(db, PROJECT_ID, limit_cents).map_err(cmd_err)
    })
}

#[tauri::command]
fn usage_series(
    state: tauri::State<AppState>,
    granularity: String,
    from: Option<String>,
    to: Option<String>,
) -> Result<Vec<hexagon_core::usage::UsageBucket>, CmdError> {
    with_conn(&state, |db, _| {
        hexagon_core::usage::series(db, PROJECT_ID, &granularity, from.as_deref(), to.as_deref())
            .map_err(cmd_err)
    })
}

/// 日志开关：app 级设置持久化在 config 目录，开发期默认开（Debug），release 默认 Info。
fn log_enabled_path(app: &tauri::AppHandle) -> Option<std::path::PathBuf> {
    app.path()
        .app_config_dir()
        .ok()
        .map(|d| d.join("settings.json"))
}

// ---------- 启动页 / 最近项目（票 29）：app 级 JSON，不进项目库 ----------

fn recents_path(app: &tauri::AppHandle) -> Option<std::path::PathBuf> {
    app.path()
        .app_config_dir()
        .ok()
        .map(|d| d.join("recent_projects.json"))
}

/// 最近项目条目（ADR 0054）：recents.json 行与 IPC 返回同一形状。
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, ts_rs::TS)]
#[ts(export, export_to = "../../ui/src/gen/")]
struct RecentProject {
    dir: String,
    name: String,
    mode: String,
    #[ts(type = "number")] // JS number 域
    opened_at: u64,
}

fn read_recents(app: &tauri::AppHandle) -> Vec<RecentProject> {
    recents_path(app)
        .and_then(|p| std::fs::read_to_string(p).ok())
        .and_then(|t| serde_json::from_str(&t).ok())
        .unwrap_or_default()
}

fn remember_recent(app: &tauri::AppHandle, dir: &str, name: &str, mode: &str) {
    let mut rs = read_recents(app);
    rs.retain(|r| r.dir != dir);
    rs.insert(
        0,
        RecentProject {
            dir: dir.into(),
            name: name.into(),
            mode: mode.into(),
            opened_at: std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_secs())
                .unwrap_or(0),
        },
    );
    rs.truncate(10);
    if let Some(p) = recents_path(app) {
        if let Some(parent) = p.parent() {
            let _ = std::fs::create_dir_all(parent);
        }
        let _ = std::fs::write(&p, serde_json::to_string(&rs).unwrap_or_default());
    }
}

#[tauri::command]
fn recent_projects(app: tauri::AppHandle) -> Vec<RecentProject> {
    read_recents(&app)
        .into_iter()
        .filter(|r| std::path::Path::new(&r.dir).is_dir())
        .collect()
}

/// 重新打开已有项目：钉住的包副本恢复 pack，fastpath 项目无副本即 None。
#[tauri::command]
fn open_recent(
    app: tauri::AppHandle,
    state: tauri::State<AppState>,
    dir: String,
) -> Result<(), CmdError> {
    // ADR 0060：已有工作台走打开。落库/花名册不在这里重跑；name 以库内为准。
    let mut wb = hexagon_core::setup::open_existing(&dir).map_err(cmd_err)?;
    wb.attach_providers(hexagon_core::credentials::active());
    attach_delta_hook(&app, &wb);
    *state
        .conn
        .lock()
        .map_err(|_| CmdError::internal("lock poisoned"))? = Some(open_control(&dir)?);
    let info = hexagon_core::orchestra::project_info(&wb.db, PROJECT_ID).map_err(cmd_err)?;
    *state
        .wb
        .lock()
        .map_err(|_| CmdError::internal("lock poisoned"))? = Some(wb);
    remember_recent(&app, &dir, &info.name, &info.mode);
    Ok(())
}

/// 关闭当前项目回启动页（不删任何数据）。
#[tauri::command]
fn close_project(state: tauri::State<AppState>) -> Result<(), CmdError> {
    *state
        .wb
        .lock()
        .map_err(|_| CmdError::internal("lock poisoned"))? = None;
    *state
        .conn
        .lock()
        .map_err(|_| CmdError::internal("lock poisoned"))? = None;
    Ok(())
}

fn load_log_enabled(app: &tauri::AppHandle) -> bool {
    log_enabled_path(app)
        .and_then(|p| std::fs::read_to_string(p).ok())
        .and_then(|t| serde_json::from_str::<Value>(&t).ok())
        .and_then(|v| v["log_enabled"].as_bool())
        .unwrap_or(cfg!(debug_assertions)) // dev 默认开，release 默认关
}

#[tauri::command]
fn set_log_enabled(app: tauri::AppHandle, enabled: bool) -> Result<(), CmdError> {
    let level = if enabled {
        log::LevelFilter::Debug
    } else {
        log::LevelFilter::Warn
    };
    log::set_max_level(level);
    let p = log_enabled_path(&app).ok_or_else(|| CmdError::internal("no config dir"))?;
    if let Some(parent) = p.parent() {
        std::fs::create_dir_all(parent).map_err(cmd_err)?;
    }
    std::fs::write(&p, serde_json::json!({"log_enabled": enabled}).to_string()).map_err(cmd_err)?;
    log::info!("log level set to {level}");
    Ok(())
}

#[tauri::command]
fn log_enabled(app: tauri::AppHandle) -> bool {
    load_log_enabled(&app)
}

// ---------- 项目向导（票 24）：开项目前的检查/选择/建项目，不需要 wb ----------

#[tauri::command]
fn inspect_dir(dir: String) -> hexagon_core::setup::DirReport {
    hexagon_core::setup::inspect_dir(&dir)
}

#[tauri::command]
fn preset_roles() -> Result<Vec<hexagon_core::presets::RoleDef>, CmdError> {
    hexagon_core::presets::preset_roles().map_err(cmd_err)
}

// ---------- 角色模板库（ADR 0057：~/.hexagon/roles.json，无项目可用） ----------

#[tauri::command]
fn list_role_templates() -> Result<Vec<hexagon_core::templates::RoleTemplate>, CmdError> {
    hexagon_core::templates::role_templates().map_err(cmd_err)
}

#[tauri::command]
fn save_role_template(def: hexagon_core::presets::RoleDef) -> Result<(), CmdError> {
    hexagon_core::templates::save_role_template(&def).map_err(cmd_err)
}

/// 只删自定义层；覆盖内置的同名项删除后内置复活（就近优先自然结果）。
#[tauri::command]
fn delete_role_template(name: String) -> Result<(), CmdError> {
    hexagon_core::templates::delete_role_template(&name).map_err(cmd_err)
}

#[tauri::command]
fn preset_packs() -> Result<Vec<hexagon_core::orchestra::PackDef>, CmdError> {
    hexagon_core::presets::preset_packs().map_err(cmd_err)
}

// ---------- 供应商配置（设置页模型区 / 启动页共用）----------
// ui-audit-2 票 08 裁决：check_model_keys/set_model_key 已删——槽位级
// model/{slot} 凭据模型被 provider/{id} 供应商级 key 取代（require_model_key
// 无调用方），留着是误导性死面。

/// 供应商文档：非密字段 + 各供应商 key 是否已存（key 明文永不回传）+ 槽位绑定表。
/// 实现归 provider_admin（票 05）——壳层只转发。
#[tauri::command]
fn list_providers() -> Result<hexagon_core::provider_admin::ProvidersView, CmdError> {
    hexagon_core::provider_admin::list(&*hexagon_core::credentials::active()).map_err(cmd_err)
}

/// 刷新运行中 Workbench 的供应商注册（保存/删除/绑定变更后热生效）。
fn refresh_providers(state: &AppState) {
    // D01-ok: mutation 路径——供应商变更后热挂接走 wb.reload_providers。
    let mut g = match state.wb.lock() {
        // D01-ok: mutation helper
        Ok(g) => g,
        Err(_) => return,
    };
    if let Some(wb) = g.as_mut() {
        wb.reload_providers();
    }
}

/// 保存供应商 + 可选 key；开着的项目热刷新。
#[tauri::command]
fn save_provider(
    state: tauri::State<AppState>,
    provider: hexagon_core::provider_admin::ProviderDef,
    secret: Option<String>,
) -> Result<(), CmdError> {
    hexagon_core::provider_admin::save(provider, secret, &*hexagon_core::credentials::active())
        .map_err(cmd_err)?;
    refresh_providers(&state);
    Ok(())
}

/// 删供应商（级联解绑槽位）+ 热刷新；keychain 不动。
#[tauri::command]
fn delete_provider(state: tauri::State<AppState>, id: String) -> Result<(), CmdError> {
    hexagon_core::provider_admin::delete(&id).map_err(cmd_err)?;
    refresh_providers(&state);
    Ok(())
}

/// 槽位绑定（供应商必须存在）+ 热刷新。
#[tauri::command]
fn set_slot_binding(
    state: tauri::State<AppState>,
    slot: String,
    provider_id: String,
    model: String,
) -> Result<(), CmdError> {
    hexagon_core::provider_admin::set_binding(&slot, &provider_id, &model).map_err(cmd_err)?;
    refresh_providers(&state);
    Ok(())
}

/// 解绑槽位 + 热刷新。
#[tauri::command]
fn remove_slot_binding(state: tauri::State<AppState>, slot: String) -> Result<(), CmdError> {
    hexagon_core::provider_admin::remove_binding(&slot).map_err(cmd_err)?;
    refresh_providers(&state);
    Ok(())
}

/// 拉取/检测供应商模型目录：GET /models；key 从 keychain 现取，缺 key 直报。
/// 返回 ModelEntry 表（id + 分组 + 推断能力），UI 合并进供应商配置。
#[tauri::command]
fn fetch_provider_models(
    id: String,
) -> Result<Vec<hexagon_core::provider_admin::ModelEntry>, CmdError> {
    hexagon_core::provider_admin::fetch_models(&id, &*hexagon_core::credentials::active())
        .map_err(cmd_err)
}

#[tauri::command]
fn agents_md_draft(name: String) -> String {
    hexagon_core::setup::agents_md_draft(&name)
}

#[derive(serde::Deserialize)]
#[serde(rename_all = "camelCase")]
#[derive(ts_rs::TS)]
#[ts(export, export_to = "../../ui/src/gen/")]
struct CreateProjectOpts {
    dir: String,
    name: String,
    roles: Vec<String>,
    /// 角色定制覆盖（ADR 0057）：自定义模板与向导改过的角色传完整定义；
    /// 未列名字走内置目录。壳层不读全局模板文件，保持 core 纯函数。
    #[serde(default)]
    #[ts(optional)]
    role_overrides: Option<Vec<hexagon_core::presets::RoleDef>>,
    #[ts(optional)]
    pack_name: Option<String>,
    #[ts(optional)]
    fastpath_role: Option<String>,
    init_git: bool,
    #[ts(optional)]
    agents_md: Option<String>,
}

#[tauri::command]
fn create_project(
    app: tauri::AppHandle,
    state: tauri::State<AppState>,
    opts: CreateProjectOpts,
    // 票 14：每步做完就推一帧。同步命令跑在阻塞池，不占渲染线程；
    // 被否决的替代是界面定时器，或等到本命令返回才一次性打勾。
    on_progress: tauri::ipc::Channel<hexagon_core::setup::CreateStep>,
) -> Result<(), CmdError> {
    use hexagon_core::setup;
    // 负责人已确认的说明文件先落盘（已存在会被 write_agents_md 拒绝，不覆盖）
    if let Some(md) = &opts.agents_md {
        setup::write_agents_md(&opts.dir, md).map_err(cmd_err)?;
    }
    let pack = opts
        .pack_name
        .as_deref()
        .map(|n| {
            hexagon_core::presets::preset_packs()
                .map_err(cmd_err)?
                .into_iter()
                .find(|p| p.name == n)
                .ok_or_else(|| CmdError::internal(format!("未知流程包: {n}")))
        })
        .transpose()?;
    // 壳层唯一保留的 provider_config:: 直调：create_project 建档校验要文档现状
    let pdoc = hexagon_core::provider_config::load().unwrap_or_default();
    let mut wb = setup::create_project_reporting(
        &opts.dir,
        &opts.name,
        &opts.roles,
        opts.role_overrides.as_deref().unwrap_or(&[]),
        pack.as_ref(),
        opts.fastpath_role.as_deref(),
        opts.init_git,
        &*hexagon_core::credentials::active(),
        &pdoc,
        |step| {
            let _ = on_progress.send(step);
        },
    )
    .map_err(cmd_err)?;
    // 接线尾步归 Workbench：凭据库 + 按文档注册运行槽位（票 05）
    wb.attach_providers(hexagon_core::credentials::active());
    attach_delta_hook(&app, &wb);
    *state
        .conn
        .lock()
        .map_err(|_| CmdError::internal("lock poisoned"))? = Some(open_control(&opts.dir)?);
    *state
        .wb
        .lock()
        .map_err(|_| CmdError::internal("lock poisoned"))? = Some(wb); // D01-ok: 创建成功才装入工作台
    remember_recent(
        &app,
        &opts.dir,
        &opts.name,
        if opts.pack_name.is_some() {
            "pack"
        } else {
            "fastpath"
        },
    );
    Ok(())
}

#[tauri::command]
fn project_open(state: tauri::State<AppState>) -> bool {
    // D01-exempt: read 侧 is_some 探针——只为答「有没有开着的项目」，
    // 不调用 wb 方法，D01 禁的是 read 组借 wb 做事，不是看这个槽位空不空。
    state.wb.lock().map(|g| g.is_some()).unwrap_or(false) // D01-exempt: is_some probe
}

// ---------- 快速通道（票 26）----------

#[tauri::command]
fn project_info(
    state: tauri::State<AppState>,
) -> Result<hexagon_core::orchestra::ProjectInfo, CmdError> {
    with_conn(&state, |db, _| {
        hexagon_core::orchestra::project_info(db, PROJECT_ID).map_err(cmd_err)
    })
}

#[tauri::command]
fn dispatch(
    state: tauri::State<AppState>,
    role: String,
    input: String,
    attachments: Vec<hexagon_core::trace::AttachRef>,
) -> Result<hexagon_core::turn::TurnOutcome, CmdError> {
    with_wb(&state, |wb| wb.dispatch(&role, &input, &attachments))
}

#[tauri::command]
fn upgrade_to_pack(state: tauri::State<AppState>, pack_name: String) -> Result<(), CmdError> {
    let pack = hexagon_core::presets::preset_packs()
        .map_err(cmd_err)?
        .into_iter()
        .find(|p| p.name == pack_name)
        .ok_or_else(|| CmdError::internal(format!("未知流程包: {pack_name}")))?;
    with_wb_mut(&state, |wb| wb.upgrade_to_pack(pack))
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .plugin(
            tauri_plugin_log::Builder::new()
                .targets([
                    tauri_plugin_log::Target::new(tauri_plugin_log::TargetKind::Stdout),
                    tauri_plugin_log::Target::new(tauri_plugin_log::TargetKind::LogDir {
                        file_name: Some("hexagon".into()),
                    }),
                    tauri_plugin_log::Target::new(tauri_plugin_log::TargetKind::Webview),
                ])
                .max_file_size(5_000_000)
                .rotation_strategy(tauri_plugin_log::RotationStrategy::KeepSome(5))
                .level(log::LevelFilter::Debug)
                .build(),
        )
        .setup(|app| {
            // dev 壳默认凭据文件缝（仅 debug 构建）：macOS 对 adhoc 签名
            // 二进制的 SecKeychain 写入静默丢写（credentials.rs
            // OsKeychain::set 写后回读实证），不设默认则 `npm run dev`
            // 每次存 key 必报 keychain unavailable。显式 env 优先；
            // 要测真钥匙串 `HEXAGON_CREDENTIALS_PATH=keychain` 起 dev。
            // release 壳不做此兜底——生产包保持 fail-closed 走 OsKeychain。
            #[cfg(debug_assertions)]
            if std::env::var_os("HEXAGON_CREDENTIALS_PATH").is_none_or(|v| v.is_empty()) {
                if let Some(p) = hexagon_core::credentials::dev_file_path() {
                    log::info!("dev build: credentials file store at {}", p.display());
                    std::env::set_var("HEXAGON_CREDENTIALS_PATH", &p);
                }
            }
            // 启动时按持久化设置收口级别（plugin 初始给 Debug 以便捕获启动日志）
            let enabled = load_log_enabled(app.handle());
            log::set_max_level(if enabled {
                log::LevelFilter::Debug
            } else {
                log::LevelFilter::Warn
            });
            log::info!("hexagon-bot starting, log_enabled={enabled}");
            Ok(())
        })
        .manage(AppState {
            wb: Mutex::new(None),
            conn: Mutex::new(None),
        })
        .invoke_handler(tauri::generate_handler![
            core_ping,
            sandbox_status,
            open_project,
            timeline,
            send_message,
            stage_attachment,
            discard_attachments,
            answer_permission,
            advance,
            run_checks,
            stamp,
            rewind,
            skip,
            skip_review,
            pause,
            resume,
            sleep_all,
            artifacts,
            artifact_content,
            team,
            stage_status,
            pending_questions,
            permission_rules,
            revoke_permission_rule,
            list_skills,
            set_skill_muted,
            skill_files,
            read_skill_file,
            save_global_skill,
            scan_external_skills,
            import_skills,
            install_skill_path,
            mcp_services,
            list_mcp_entries,
            save_mcp_service,
            delete_mcp_service,
            scan_external_mcp,
            import_mcp,
            open_mcp_market,
            repo_paths,
            usage,
            open_stage,
            recover_run,
            override_checks,
            agent_detail,
            update_agent,
            create_role,
            set_agent_grants,
            draft_role_def,
            pack_draft,
            save_pack_draft,
            save_pack_template,
            pack_templates,
            pack_template,
            export_pack_yaml,
            resolve_install,
            export_events,
            autonomy,
            set_autonomy,
            reviewer_mode,
            set_reviewer_mode,
            owner_away,
            owner_back,
            reject_stamp,
            adjudicate_flag,
            proposals,
            review_proposal,
            confirm_proposal,
            reject_proposal,
            invariant_check,
            policydev_propose,
            rollback_proposal,
            request_publish,
            confirm_publish,
            reject_publish,
            set_agent_avatar,
            agent_avatar,
            artifact_content_at,
            set_agent_sleeping,
            set_usage_limit,
            usage_series,
            set_log_enabled,
            log_enabled,
            inspect_dir,
            preset_roles,
            list_role_templates,
            save_role_template,
            delete_role_template,
            preset_packs,
            list_providers,
            save_provider,
            delete_provider,
            set_slot_binding,
            remove_slot_binding,
            fetch_provider_models,
            agents_md_draft,
            create_project,
            project_open,
            project_info,
            dispatch,
            upgrade_to_pack,
            recent_projects,
            open_recent,
            close_project,
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}

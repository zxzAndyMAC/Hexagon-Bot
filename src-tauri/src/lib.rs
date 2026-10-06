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
    generation: u64,
}

static CONTROL_GENERATION: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(1);

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, ts_rs::TS)]
#[serde(deny_unknown_fields)]
#[ts(export, export_to = "../../ui/src/gen/")]
struct ProjectIdentity {
    project_root: String,
    #[ts(type = "number")]
    generation: u64,
}

#[derive(Debug, Clone, serde::Serialize, ts_rs::TS)]
#[ts(export, export_to = "../../ui/src/gen/")]
struct ProjectTurnDelta {
    identity: ProjectIdentity,
    // ts-rs 11 resolves dependency export_to in this crate's cwd. Inline the
    // core-derived shape instead of importing a path outside this workspace.
    #[ts(inline)]
    delta: hexagon_core::turn::TurnDelta,
}

#[derive(Debug, Clone, serde::Serialize, ts_rs::TS)]
#[ts(export, export_to = "../../ui/src/gen/")]
struct ProjectToolOutputDelta {
    identity: ProjectIdentity,
    #[ts(inline)] // same cross-crate export path constraint as ProjectTurnDelta
    delta: hexagon_core::sessions::ToolOutputDelta,
}

impl ControlConn {
    fn identity(&self) -> ProjectIdentity {
        ProjectIdentity {
            project_root: self.root.to_string_lossy().into_owned(),
            generation: self.generation,
        }
    }
}

// Foundation Spec P0 / 2026-10-06: q1 and p1 are local to each database.
// Clearing UI projections cannot revoke an IPC queued for a previous project.
// Native CommandArg preserves the originating token without a command-name map.
struct ProjectState<'a> {
    state: &'a AppState,
    identity: Option<ProjectIdentity>,
}

impl std::ops::Deref for ProjectState<'_> {
    type Target = AppState;
    fn deref(&self) -> &Self::Target {
        self.state
    }
}

fn project_identity_argument(
    body: &tauri::ipc::InvokeBody,
) -> Result<Option<ProjectIdentity>, CmdError> {
    let reject = |reason| {
        hexagon_core::diag::note(
            hexagon_core::diag::CLASS_REJECT,
            true,
            None,
            None,
            None,
            None,
            "project_identity_argument",
            reason,
            std::time::Instant::now(),
        );
        CmdError {
            code: "project_changed".into(),
            message: "project identity required or malformed".into(),
        }
    };
    let tauri::ipc::InvokeBody::Json(body) = body else {
        return Err(reject("raw_payload"));
    };
    let value = body
        .get("_project")
        .ok_or_else(|| reject("identity_missing"))?;
    serde_json::from_value(value.clone()).map_err(|_| reject("identity_malformed"))
}

impl<'a, R: tauri::Runtime> tauri::ipc::CommandArg<'a, R> for ProjectState<'a> {
    fn from_command(
        command: tauri::ipc::CommandItem<'a, R>,
    ) -> Result<Self, tauri::ipc::InvokeError> {
        let identity = project_identity_argument(command.message.payload())
            .map_err(tauri::ipc::InvokeError::from)?;
        let state =
            <tauri::State<'a, AppState> as tauri::ipc::CommandArg<'a, R>>::from_command(command)?;
        Ok(Self {
            state: state.inner(),
            identity,
        })
    }
}

#[tauri::command]
fn project_identity(state: tauri::State<AppState>) -> Result<Option<ProjectIdentity>, CmdError> {
    let connection = state
        .conn
        .lock()
        .map_err(|_| CmdError::internal("lock poisoned"))?;
    Ok(connection.as_ref().map(ControlConn::identity))
}

fn with_project_conn<R, E>(
    state: &ProjectState<'_>,
    f: impl FnOnce(&hexagon_core::db::Db, &std::path::Path) -> Result<R, E>,
) -> Result<R, CmdError>
where
    E: ErrorCode + std::fmt::Display,
{
    with_control(state, |connection| {
        check_project_identity(Some(connection), state.identity.as_ref(), false)?;
        f(&connection.db, &connection.root).map_err(cmd_err)
    })
}

fn with_wb<R, E>(
    state: &ProjectState<'_>,
    f: impl FnOnce(&Workbench) -> Result<R, E>,
) -> Result<R, CmdError>
where
    E: ErrorCode + std::fmt::Display,
{
    with_unscoped_wb(state, |wb| {
        check_project_state(state, false)?;
        f(wb).map_err(cmd_err)
    })
}

fn check_project_state(state: &ProjectState<'_>, allow_absent: bool) -> Result<(), CmdError> {
    let connection = state
        .conn
        .lock()
        .map_err(|_| CmdError::internal("lock poisoned"))?;
    check_project_identity(connection.as_ref(), state.identity.as_ref(), allow_absent)
}

fn check_project_identity(
    connection: Option<&ControlConn>,
    expected: Option<&ProjectIdentity>,
    allow_absent: bool,
) -> Result<(), CmdError> {
    let started = std::time::Instant::now();
    let reason = match (connection, expected) {
        (Some(current), Some(expected))
            if current.root.to_string_lossy() == expected.project_root
                && current.generation == expected.generation =>
        {
            "matched"
        }
        (None, None) if allow_absent => "no_previous_project",
        (Some(current), Some(expected))
            if current.root.to_string_lossy() == expected.project_root =>
        {
            "generation_changed"
        }
        (Some(_), Some(_)) => "root_changed",
        _ => "identity_missing",
    };
    let accepted = matches!(reason, "matched" | "no_previous_project");
    // False rejection costs one retry; false acceptance can approve a new
    // project's local q1. Never canonicalize/open a client-provided root here.
    hexagon_core::diag::note(
        if accepted {
            hexagon_core::diag::CLASS_JUDGE
        } else {
            hexagon_core::diag::CLASS_REJECT
        },
        !accepted,
        Some(PROJECT_ID),
        None,
        None,
        None,
        "project_identity",
        reason,
        started,
    );
    if accepted {
        Ok(())
    } else {
        Err(CmdError {
            code: "project_changed".into(),
            message: "project changed before command execution".into(),
        })
    }
}

/// 回合 delta → webview（turn-streaming 票 03）：所有 Workbench 构造点
/// 统一挂 emit。delta 是瞬时展示通道，持久层照旧走 events/messages。
/// 票 04 同纪律加挂 tool-output：bash 执行中 stdout/stderr 逐段推。
fn attach_delta_hook(app: &tauri::AppHandle, wb: &Workbench, identity: &ProjectIdentity) {
    // I1 / 2026-10-06: old queued IPC frames survived clearing the UI and
    // reused a0 in the next project. Freeze the producing connection, never
    // look up the current connection at emission time or relabel old output.
    let turn_identity = identity.clone();
    let h = app.clone();
    wb.set_turn_delta_hook(Some(Box::new(move |d| {
        let _ = h.emit(
            "turn-delta",
            ProjectTurnDelta {
                identity: turn_identity.clone(),
                delta: d.clone(),
            },
        );
    })));
    let output_identity = identity.clone();
    let h2 = app.clone();
    wb.set_tool_output_hook(Some(Box::new(move |d| {
        let _ = h2.emit(
            "tool-output",
            ProjectToolOutputDelta {
                identity: output_identity.clone(),
                delta: d.clone(),
            },
        );
    })));
}

/// 打开控制通道：项目库的第二连接。与 wb 内连接同参数（WAL+busy_timeout）。
fn open_control(dir: &str) -> Result<ControlConn, CmdError> {
    Ok(ControlConn {
        db: hexagon_core::db::Db::open(std::path::Path::new(dir).join(".hexagon/state.db"))
            .map_err(cmd_err)?,
        root: std::path::Path::new(dir)
            .canonicalize()
            .map_err(|error| CmdError::internal(error.to_string()))?,
        generation: CONTROL_GENERATION.fetch_add(1, std::sync::atomic::Ordering::Relaxed),
    })
}

/// Project controls must stop while the old connection is still available.
fn stop_control(connection: &Option<ControlConn>) -> Result<(), CmdError> {
    hexagon_core::api::desktop_preview_stop().map_err(CmdError::internal)?;
    if let Some(old) = connection {
        hexagon_core::api::desktop_close_project(&old.root).map_err(|message| CmdError {
            code: "project_close_pending".into(),
            message,
        })?;
        hexagon_core::api::browser_close_project(&old.root);
    }
    Ok(())
}

#[cfg(test)]
fn replace_control(state: &AppState, next: Option<ControlConn>) -> Result<(), CmdError> {
    let mut connection = state
        .conn
        .lock()
        .map_err(|_| CmdError::internal("lock poisoned"))?;
    stop_control(&connection)?;
    *connection = next;
    Ok(())
}

/// Benchmark I1: swapping conn before acquiring wb let the old turn keep running
/// behind a new project's control plane. Reuse the pause gate to stop that turn,
/// then install both handles under their locks. An unconfirmed native drain keeps
/// the original project open; it is never reported as a completed cancellation.
fn replace_project(
    state: &AppState,
    next_wb: Option<Workbench>,
    next: Option<ControlConn>,
    expected: &Option<ProjectIdentity>,
) -> Result<(), CmdError> {
    let started = std::time::Instant::now();
    let previous = {
        let connection = state
            .conn
            .lock()
            .map_err(|_| CmdError::internal("lock poisoned"))?;
        check_project_identity(connection.as_ref(), expected.as_ref(), true)?;
        let previous = connection
            .as_ref()
            .map(|old| {
                let paused =
                    hexagon_core::orchestra::is_paused(&old.db, PROJECT_ID).map_err(cmd_err)?;
                if !paused {
                    hexagon_core::orchestra::pause(&old.db, PROJECT_ID).map_err(cmd_err)?;
                }
                Ok::<_, CmdError>((old.generation, paused))
            })
            .transpose()?;
        stop_control(&connection)?;
        previous
    };
    let mut workbench = state
        .wb
        .lock()
        .map_err(|_| CmdError::internal("lock poisoned"))?; // D01-ok: project replacement waits for the paused old turn
    let mut connection = state
        .conn
        .lock()
        .map_err(|_| CmdError::internal("lock poisoned"))?;
    if connection.as_ref().map(|old| old.generation) != previous.map(|(generation, _)| generation) {
        hexagon_core::diag::note(
            hexagon_core::diag::CLASS_REJECT,
            true,
            Some(PROJECT_ID),
            None,
            None,
            None,
            "project_replacement",
            "generation_changed",
            started,
        );
        return Err(CmdError {
            code: "project_changed".into(),
            message: "project changed while stopping the old turn".into(),
        });
    }
    if let Some(old_wb) = workbench.as_ref() {
        old_wb.stop_owned_work();
    }
    stop_control(&connection)?;
    if let (Some(old), Some((_, false))) = (connection.as_ref(), previous) {
        // No fallible operation after replacing either handle. The old parent
        // is excluded by wb and its children are halted before restoring pause.
        hexagon_core::orchestra::resume(&old.db, PROJECT_ID).map_err(cmd_err)?;
    }
    *connection = next;
    // Drop sends halt and closes model-owned processes. This is not proof of
    // remote billing cancellation, nor of a noncooperative child thread joining.
    drop(workbench.take());
    *workbench = next_wb;
    Ok(())
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
    with_control(state, |connection| f(&connection.db, &connection.root))
}

fn with_control<R, E>(
    state: &AppState,
    f: impl FnOnce(&ControlConn) -> Result<R, E>,
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
    f(c).map_err(cmd_err)
}

fn with_unscoped_wb<R, E>(
    state: &AppState,
    f: impl FnOnce(&Workbench) -> Result<R, E>,
) -> Result<R, CmdError>
where
    E: ErrorCode + std::fmt::Display,
{
    let g = state
        .wb
        .lock()
        .map_err(|_| CmdError::internal("lock poisoned"))?; // D01-ok: shared Workbench lock for project commands
    let wb = g
        .as_ref()
        .ok_or_else(|| CmdError::internal("no project open"))?;
    f(wb).map_err(cmd_err)
}

fn with_wb_mut<R, E>(
    state: &ProjectState<'_>,
    f: impl FnOnce(&mut Workbench) -> Result<R, E>,
) -> Result<R, CmdError>
where
    E: ErrorCode + std::fmt::Display,
{
    let mut g = state
        .wb
        .lock()
        .map_err(|_| CmdError::internal("lock poisoned"))?; // D01-ok: mutable project command under the Workbench lock
    let wb = g
        .as_mut()
        .ok_or_else(|| CmdError::internal("no project open"))?;
    check_project_state(state, false)?;
    f(wb).map_err(cmd_err)
}

#[tauri::command]
fn core_ping() -> String {
    hexagon_core::ping()
}

/// 沙箱实况（agent-senses 票 06）：纯平台探测，不触 state.wb（D01-exempt）。
/// Read-only host disclosure works with or without a project, without the turn lock.
#[tauri::command]
fn data_boundary(
    state: tauri::State<AppState>,
) -> Result<hexagon_core::data_boundary::DataBoundary, CmdError> {
    let root = state
        .conn
        .lock()
        .map_err(|_| CmdError::internal("lock poisoned"))?
        .as_ref()
        .map(|c| c.root.clone());
    hexagon_core::api::data_boundary(root.as_deref(), &*hexagon_core::credentials::active())
        .map_err(cmd_err)
}

// Read/control host commands: independent of project state and the turn lock.
fn desktop_library_path(app: &tauri::AppHandle) -> Result<std::path::PathBuf, CmdError> {
    if cfg!(debug_assertions) {
        Ok(std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../native/computer-use/.build/debug/libHexagonComputerUse.dylib"))
    } else {
        Ok(app
            .path()
            .resource_dir()
            .map_err(|e| CmdError::internal(e.to_string()))?
            .join("libHexagonComputerUse.dylib"))
    }
}

#[tauri::command]
fn design_direction(
    state: tauri::State<AppState>,
) -> Result<hexagon_core::design::DesignDirection, CmdError> {
    with_conn(&state, |db, _| {
        hexagon_core::api::design_direction(db, PROJECT_ID).map_err(cmd_err)
    })
}
#[tauri::command]
fn choose_design_direction(
    app: tauri::AppHandle,
    state: ProjectState<'_>,
    question_id: String,
    revision: i64,
    option_id: Option<String>,
    existing_guidance: Option<String>,
) -> Result<hexagon_core::design::DesignDirection, CmdError> {
    let direction = with_project_conn(&state, |db, _| {
        let direction = hexagon_core::api::choose_design_direction(
            db,
            PROJECT_ID,
            &question_id,
            revision,
            option_id.as_deref(),
            existing_guidance.as_deref(),
        )
        .map_err(cmd_err)?;
        Ok::<_, CmdError>(direction)
    })?;
    let revision = direction.revision;
    // Fullstack QA #04: status=active is not a queued turn. Follow the same
    // short control/write then asynchronous turn pattern as owner messages.
    let app = app.clone();
    let identity = state.identity.clone();
    tauri::async_runtime::spawn(async move {
        let managed = app.state::<AppState>();
        let state = ProjectState {
            state: managed.inner(),
            identity,
        };
        let result = with_wb(&state, |wb| wb.continue_design_direction(revision));
        if let Err(error) = result {
            log::warn!("design continuation: {error}");
        }
    });
    Ok(direction)
}
#[tauri::command]
fn desktop_screenshot(
    state: ProjectState<'_>,
    name: String,
    expected_project_root: String,
) -> Result<hexagon_core::desktop::DesktopScreenshot, CmdError> {
    with_project_conn(&state, |db, root| {
        hexagon_core::api::desktop_screenshot(db, root, &name, &expected_project_root)
            .map_err(CmdError::internal)
    })
}

#[tauri::command]
fn desktop_status(
    state: tauri::State<AppState>,
) -> Result<hexagon_core::desktop::DesktopStatus, CmdError> {
    with_conn(&state, |db, root| {
        hexagon_core::api::desktop_status(db, root).map_err(CmdError::internal)
    })
}
// QA readiness 2026-10-06: credential lookup can wait on the OS. Keep it off
// the UI thread and outside the short project connection lock used by Pause.
#[tauri::command(async)]
fn computer_models(
    state: tauri::State<AppState>,
) -> Result<hexagon_core::provider_admin::ComputerModels, CmdError> {
    let context = with_conn(&state, |db, root| {
        hexagon_core::api::computer_model_context(db, root).map_err(CmdError::internal)
    })?;
    hexagon_core::api::computer_models(context).map_err(CmdError::internal)
}
#[tauri::command]
fn desktop_control(
    state: ProjectState<'_>,
    action: hexagon_core::desktop::DesktopControl,
    expected_project_root: String,
) -> Result<hexagon_core::desktop::DesktopStatus, CmdError> {
    with_project_conn(&state, |db, root| {
        // Ticket 07: check and mutation share the same current-project lock.
        hexagon_core::api::desktop_check_project(root, &expected_project_root)
            .map_err(CmdError::internal)?;
        hexagon_core::api::desktop_control(db, root, action).map_err(CmdError::internal)
    })
}

#[tauri::command]
fn desktop_permissions(
    app: tauri::AppHandle,
) -> Result<hexagon_core::desktop::DesktopPermissions, CmdError> {
    Ok(hexagon_core::api::desktop_permissions(
        &desktop_library_path(&app)?,
    ))
}

#[tauri::command]
fn desktop_open_settings(
    app: tauri::AppHandle,
    permission: hexagon_core::desktop::DesktopPermission,
) -> Result<(), CmdError> {
    hexagon_core::api::desktop_request_permission(&desktop_library_path(&app)?, permission)
        .map_err(CmdError::internal)?;
    open::that(hexagon_core::api::desktop_settings_url(permission))
        .map_err(|e| CmdError::internal(e.to_string()))
}

#[tauri::command]
fn sandbox_status() -> hexagon_core::sandbox::SandboxStatus {
    hexagon_core::sandbox::status()
}

// 2026-09-22：打开项目（建库、钥匙串、供应商）若留在主线程，窗口在
// 命令返回前不能重画。和 send_message 同一处：标 async 丢到运行时线程。
#[tauri::command(async)]
fn open_project(
    app: tauri::AppHandle,
    state: ProjectState<'_>,
    dir: String,
    name: String,
    roles: Vec<(String, String)>,
    pack_json: Option<String>,
) -> Result<(), CmdError> {
    check_project_state(&state, true)?;
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
    let control = open_control(&dir)?;
    attach_delta_hook(&app, &wb, &control.identity());
    replace_project(&state, Some(wb), Some(control), &state.identity)?;
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

// D01 read: metadata refresh cannot acquire or drive a model turn.
#[tauri::command(async)]
fn timeline_window_metadata(
    state: tauri::State<AppState>,
    request: hexagon_core::trace::window::TimelineWindowMetadataRequest,
) -> Result<hexagon_core::trace::window::TimelineWindowMetadata, CmdError> {
    with_conn(&state, |db, root| {
        hexagon_core::api::timeline_window_metadata(db, root, &request).map_err(CmdError::internal)
    })
}

// D01 read: only the current control connection; no workbench/turn lock.
#[tauri::command(async)]
fn timeline_window(
    state: tauri::State<AppState>,
    request: hexagon_core::trace::window::TimelineWindowRequest,
) -> Result<hexagon_core::trace::window::TimelineWindowPage, CmdError> {
    with_conn(&state, |db, root| {
        hexagon_core::api::timeline_window(db, root, &request).map_err(CmdError::internal)
    })
}

// D01 read: only the current control connection; no workbench/turn lock.
#[tauri::command(async)]
fn timeline_facts(
    state: tauri::State<AppState>,
    request: hexagon_core::trace::window::TimelineFactsRequest,
) -> Result<hexagon_core::trace::window::TimelineFacts, CmdError> {
    with_conn(&state, |db, root| {
        hexagon_core::api::timeline_facts(db, root, &request).map_err(CmdError::internal)
    })
}

// D01 read: only the current control connection; no workbench/turn lock.
#[tauri::command(async)]
fn timeline_nodes(
    state: tauri::State<AppState>,
    request: hexagon_core::trace::window::TimelineNodesRequest,
) -> Result<hexagon_core::trace::window::TimelineNodesPage, CmdError> {
    with_conn(&state, |db, root| {
        hexagon_core::api::timeline_nodes(db, root, &request).map_err(CmdError::internal)
    })
}

// 2026-09-22：默认同步命令在主线程上跑完才把 IPC 还回去（tauri-macros
// ExecutionContext::Blocking）。回合链和模型流都堵在这里时，窗口事件循环
// 不转，turn-delta 和进度帧要等命令返回才一次性画出来。被否决的替代是
// 界面定时器假装在流。`command(async)` 把同步函数丢到运行时线程，主线程
// 继续收事件。
#[tauri::command(async)]
fn send_message(
    app: tauri::AppHandle,
    state: ProjectState<'_>,
    body: String,
    attachments: Vec<hexagon_core::trace::AttachRef>,
    element_ids: Option<Vec<String>>,
    expected_project_root: Option<String>,
) -> Result<i64, CmdError> {
    // 控制通道（ADR 0052）：路由表归 commands::send_via_control——消息落库 +
    // 暂停/恢复 就地生效；其余指令（rewind/stamp/skip/override/install）
    // 返回上来排 wb 队列。两段分开拿锁：conn→wb 不嵌套。
    // 票 03：attachments 已先经 stage_attachment 落 .hexagon/inbox/。
    let element_scope = if element_ids.as_ref().is_some_and(|ids| !ids.is_empty()) {
        let expected = expected_project_root
            .as_deref()
            .ok_or_else(|| CmdError::internal("project identity required"))?;
        let root = with_project_conn(&state, |_, root| Ok::<_, CmdError>(root.to_path_buf()))?;
        Some(
            hexagon_core::browser_elements::send_scope(&root, expected)
                .map_err(CmdError::internal)?,
        )
    } else {
        None
    };
    let (id, cmd) = with_project_conn(&state, |db, root| {
        let (id, cmd) = if let Some(ids) = element_ids.as_ref().filter(|ids| !ids.is_empty()) {
            let expected = expected_project_root
                .as_deref()
                .ok_or_else(|| CmdError::internal("project identity required"))?;
            hexagon_core::browser_elements::send(
                db,
                root,
                expected,
                &body,
                &attachments,
                ids,
                element_scope
                    .as_ref()
                    .ok_or_else(|| CmdError::internal("browser scope missing"))?,
            )
            .map_err(CmdError::internal)?
        } else {
            hexagon_core::commands::send_via_control(db, PROJECT_ID, &body, &attachments)
                .map_err(cmd_err)?
        };
        Ok::<_, CmdError>((id, cmd))
    })?;
    // Return after persistence so the composer stays responsive. Live acceptance
    // #16: once the queue drains, route by ID to avoid re-running injected steering.
    let app = app.clone();
    let identity = state.identity.clone();
    tauri::async_runtime::spawn(async move {
        let managed = app.state::<AppState>();
        let state = ProjectState {
            state: managed.inner(),
            identity,
        };
        let res = with_wb(&state, |wb| {
            if let Some(other) = cmd {
                wb.dispatch_command(&other).map_err(cmd_err)
            } else {
                wb.route_queued_owner(id).map(|_| ()).map_err(cmd_err)
            }
        });
        if let Err(e) = res {
            log::warn!("send follow-up: {e}");
        }
    });
    Ok(id)
}

/// Owner-only selection: snapshot root before waiting on the browser runtime.
#[tauri::command(async)]
fn browser_selection_start(
    state: ProjectState<'_>,
    expected_project_root: String,
    session_id: String,
    labels: serde_json::Value,
) -> Result<(), CmdError> {
    let root = with_project_conn(&state, |_, root| Ok::<_, CmdError>(root.to_path_buf()))?;
    hexagon_core::browser_elements::start(&root, &expected_project_root, &session_id, labels)
        .map_err(CmdError::internal)
}
#[tauri::command(async)]
fn browser_selection_poll(
    state: ProjectState<'_>,
    expected_project_root: String,
    session_id: String,
) -> Result<Vec<hexagon_core::browser_elements::ElementRef>, CmdError> {
    let root = with_project_conn(&state, |_, root| Ok::<_, CmdError>(root.to_path_buf()))?;
    let result = hexagon_core::browser_elements::poll(&root, &expected_project_root, &session_id)
        .map_err(CmdError::internal)?;
    with_project_conn(&state, |_, current| {
        hexagon_core::desktop::actions::verify_project(current, &expected_project_root)
            .map_err(CmdError::internal)
    })?;
    Ok(result)
}
#[tauri::command]
fn browser_selection_discard(
    state: ProjectState<'_>,
    expected_project_root: String,
    ids: Vec<String>,
) -> Result<(), CmdError> {
    with_project_conn(&state, |_, root| {
        hexagon_core::desktop::actions::verify_project(root, &expected_project_root)
            .map_err(CmdError::internal)?;
        hexagon_core::browser_elements::discard(root, &ids).map_err(CmdError::internal)
    })
}

/// 票 03：粘贴/拖拽图片暂存 .hexagon/inbox/——魔数嗅探+尺寸闸在核内，
/// 壳层只传字节。返回引用供 send_message 落行。
#[tauri::command]
fn stage_attachment(
    state: ProjectState<'_>,
    name: String,
    bytes: Vec<u8>,
) -> Result<hexagon_core::trace::AttachRef, CmdError> {
    with_project_conn(&state, |db, root| {
        hexagon_core::commands::stage_attachment(db, root, &name, &bytes)
            .map_err(|e| CmdError::internal(e.to_string()))
    })
}

/// 票 03：发送失败/取消时清掉已暂存的附件文件（best-effort）。
#[tauri::command]
fn discard_attachments(
    state: ProjectState<'_>,
    refs: Vec<hexagon_core::trace::AttachRef>,
) -> Result<(), CmdError> {
    with_project_conn(&state, |_db, root| {
        hexagon_core::commands::discard_attachments(root, &refs);
        Ok::<(), CmdError>(())
    })
}

#[tauri::command(async)]
fn answer_permission(
    state: ProjectState<'_>,
    question_id: String,
    allow: bool,
    remember_shape: Option<String>,
    scope: String,
) -> Result<(), CmdError> {
    with_wb(&state, |wb| {
        wb.answer_permission_and_continue(&question_id, allow, remember_shape.as_deref(), &scope)
    })
}

#[tauri::command(async)]
fn allow_project_permission(state: ProjectState<'_>, question_id: String) -> Result<(), CmdError> {
    with_wb(&state, |wb| {
        wb.allow_project_permission_and_continue(&question_id)
    })
}

// Live acceptance #15: delivery hashing/check execution must not block the window.
#[tauri::command(async)]
fn advance(state: ProjectState<'_>) -> Result<hexagon_core::orchestra::StageAction, CmdError> {
    with_wb(&state, |wb| wb.advance())
}
// Live acceptance #15: delivery hashing/check execution must not block the window.
#[tauri::command(async)]
fn run_checks(
    state: ProjectState<'_>,
    expected_project_root: Option<String>,
) -> Result<hexagon_core::orchestra::CheckOutcome, CmdError> {
    with_wb(&state, |wb| {
        if let Some(expected) = &expected_project_root {
            hexagon_core::api::desktop_check_project(&wb.repo_root, expected)
                .map_err(hexagon_core::api::ApiError::BadInput)?;
        }
        wb.run_checks()
    })
}
// Live acceptance #15: delivery hashing/check execution must not block the window.
#[tauri::command(async)]
fn stamp(state: ProjectState<'_>) -> Result<hexagon_core::orchestra::StageAction, CmdError> {
    with_wb(&state, |wb| wb.stamp())
}
#[tauri::command]
fn rewind(
    state: ProjectState<'_>,
    to_seq: usize,
) -> Result<hexagon_core::orchestra::StageAction, CmdError> {
    with_wb(&state, |wb| wb.rewind(to_seq))
}
#[tauri::command]
fn skip(state: ProjectState<'_>) -> Result<hexagon_core::orchestra::StageAction, CmdError> {
    with_wb(&state, |wb| wb.skip())
}
#[tauri::command]
fn skip_review(state: ProjectState<'_>, artifact_kind: String) -> Result<(), CmdError> {
    with_wb(&state, |wb| wb.skip_review(&artifact_kind))
}
#[tauri::command]
fn pause(state: ProjectState<'_>) -> Result<(), CmdError> {
    // 暂停按钮是回合中的叫停通道——必须走控制连接，wb 锁正被回合占着
    with_project_conn(&state, |db, _| {
        hexagon_core::orchestra::pause(db, PROJECT_ID).map_err(cmd_err)
    })
}
#[tauri::command]
fn resume(state: ProjectState<'_>) -> Result<(), CmdError> {
    with_project_conn(&state, |db, _| {
        hexagon_core::orchestra::resume(db, PROJECT_ID).map_err(cmd_err)
    })
}
#[tauri::command]
fn sleep_all(state: ProjectState<'_>) -> Result<(), CmdError> {
    // 全员休眠是干预指令（ADR 0052 控制组）：回合中途也要能落
    with_project_conn(&state, |db, _| {
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
// 2026-10-01 live acceptance #15: hashing a nested web project on the
// main thread froze the window. A separate connection also keeps the control
// mutex available to pause/sleep/poll while this expensive read is running.
#[tauri::command(async)]
fn stage_evidence(
    state: tauri::State<AppState>,
) -> Result<Option<hexagon_core::orchestra::StageEvidence>, CmdError> {
    let root = with_conn(&state, |_, root| Ok::<_, CmdError>(root.to_path_buf()))?;
    let db = hexagon_core::db::Db::open(root.join(".hexagon/state.db")).map_err(cmd_err)?;
    let pack = hexagon_core::orchestra::PackDef::pinned(&root).map_err(cmd_err)?;
    hexagon_core::orchestra::stage_evidence(&db, PROJECT_ID, &pack).map_err(cmd_err)
}
#[tauri::command]
fn quality_configuration(
    state: tauri::State<AppState>,
) -> Result<hexagon_core::orchestra::QualityConfiguration, CmdError> {
    with_conn(&state, |db, root| {
        let pack = hexagon_core::orchestra::PackDef::pinned(root).map_err(cmd_err)?;
        hexagon_core::orchestra::quality_configuration(db, PROJECT_ID, &pack).map_err(cmd_err)
    })
}
#[tauri::command]
fn current_process_pack(
    state: tauri::State<AppState>,
) -> Result<hexagon_core::orchestra::PackDef, CmdError> {
    with_conn(&state, |db, root| {
        let pack = hexagon_core::orchestra::PackDef::pinned(root).map_err(cmd_err)?;
        hexagon_core::orchestra::current_process_pack(db, PROJECT_ID, &pack).map_err(cmd_err)
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
    with_conn(&state, |db, root| {
        let mut cards = db.queued_questions(PROJECT_ID).map_err(cmd_err)?;
        let project_root = root
            .canonicalize()
            .map_err(|error| CmdError::internal(error.to_string()))?
            .to_string_lossy()
            .into_owned();
        // Live acceptance 2026-10-01: bind recovery clicks to the same snapshot's
        // host identity; a later independent identity read could target a new project.
        for card in &mut cards {
            if card.kind == "recovery" {
                if let Some(payload) = card.payload.as_object_mut() {
                    payload.insert("host_project_root".into(), project_root.clone().into());
                }
            }
        }
        Ok::<_, CmdError>(cards)
    })
}
#[tauri::command]
fn permission_shape_suggestion(
    state: tauri::State<AppState>,
    question_id: String,
) -> Result<Option<hexagon_core::permission_suggestion::PermissionShapeSuggestion>, CmdError> {
    with_conn(&state, |db, _| {
        hexagon_core::api::permission_shape_suggestion(db, PROJECT_ID, &question_id)
            .map_err(cmd_err)
    })
}

/// Owner project mode read/control uses the read connection, never the turn lock.
#[tauri::command]
fn approval_mode(
    state: tauri::State<AppState>,
) -> Result<hexagon_core::approval_mode::ApprovalModeStatus, CmdError> {
    with_conn(&state, |db, root| {
        let mut status = hexagon_core::api::approval_mode(db, PROJECT_ID).map_err(cmd_err)?;
        status.project_root = Some(
            root.canonicalize()
                .map_err(|error| CmdError::internal(error.to_string()))?
                .to_string_lossy()
                .into_owned(),
        );
        Ok::<_, CmdError>(status)
    })
}
#[tauri::command]
fn set_approval_mode(
    state: ProjectState<'_>,
    mode: hexagon_core::approval_mode::ApprovalMode,
    expected_project_root: String,
) -> Result<hexagon_core::approval_mode::ApprovalModeStatus, CmdError> {
    with_project_conn(&state, |db, root| {
        // 2026-10-01 review: UI generation only rejects late pixels, not writes.
        // Validate the initiating workspace under the same lock as the mutation.
        hexagon_core::api::set_project_approval_mode(
            db,
            PROJECT_ID,
            root,
            &expected_project_root,
            mode,
        )
        .map_err(cmd_err)
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
fn revoke_permission_rule(state: ProjectState<'_>, rule_id: String) -> Result<(), CmdError> {
    with_project_conn(&state, |db, _| -> Result<(), CmdError> {
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
#[tauri::command(async)]
fn list_skills(
    state: tauri::State<AppState>,
) -> Result<Vec<hexagon_core::skills::SkillRow>, CmdError> {
    // Issue09: snapshot the project root, then release the connection lock
    // before filesystem discovery. No SQLite access is needed for these reads.
    let root = state
        .conn
        .lock()
        .map_err(|_| CmdError::internal("lock poisoned"))?
        .as_ref()
        .map(|c| c.root.clone());
    Ok(match root.as_deref() {
        Some(root) => hexagon_core::skills::list_all(root),
        None => hexagon_core::skills::list_global(),
    })
}

/// 技能包文件树（global-config 票 03 详情面）：有界列相对路径。
/// 无项目时只解析全局技能。
#[tauri::command(async)]
fn skill_files(state: tauri::State<AppState>, name: String) -> Result<Vec<String>, CmdError> {
    // Issue09: snapshot the project root, then release the connection lock
    // before filesystem discovery. No SQLite access is needed for these reads.
    let root = state
        .conn
        .lock()
        .map_err(|_| CmdError::internal("lock poisoned"))?
        .as_ref()
        .map(|c| c.root.clone());
    Ok(hexagon_core::skills::skill_files(&name, root.as_deref()))
}

/// 读技能包内文件（渲染面）：core 内做逃逸检查 + 256KB 截断。
#[tauri::command(async)]
fn read_skill_file(
    state: tauri::State<AppState>,
    name: String,
    rel: String,
) -> Result<String, CmdError> {
    // Issue09: snapshot the project root, then release the connection lock
    // before filesystem discovery. No SQLite access is needed for these reads.
    let root = state
        .conn
        .lock()
        .map_err(|_| CmdError::internal("lock poisoned"))?
        .as_ref()
        .map(|c| c.root.clone());
    hexagon_core::skills::read_skill_file(&name, &rel, root.as_deref())
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

// ---------- 仓库文件树（hands-free 票 11，ADR 0062）----------
// 读组 list/read 与控制组 create/write 都只借 conn.root 碰磁盘。
// 不进回合、不拿工作台锁：读组禁止借那把锁做事，写路径同样不必拿它。

/// 仓库目录一层（读组）。
#[tauri::command]
fn list_repo_dir(
    state: tauri::State<AppState>,
    rel: String,
) -> Result<Vec<hexagon_core::files::RepoEntry>, CmdError> {
    with_conn(&state, |_db, root| {
        hexagon_core::files::list_repo_dir(root, &rel)
    })
}

/// 读仓库文本（读组）。二进制与超限在核里拒绝。
#[tauri::command]
fn read_repo_file(state: tauri::State<AppState>, path: String) -> Result<String, CmdError> {
    with_conn(&state, |_db, root| {
        hexagon_core::files::read_repo_file(root, &path)
    })
}

/// Versioned owner read (read group): incarnation is captured under conn lock.
#[tauri::command]
fn read_repo_file_snapshot(
    state: tauri::State<AppState>,
    path: String,
    expected_project_root: String,
) -> Result<hexagon_core::files::RepoFileSnapshot, CmdError> {
    read_repo_file_snapshot_for_state(&state, &path, &expected_project_root)
}

fn read_repo_file_snapshot_for_state(
    state: &AppState,
    path: &str,
    expected_project_root: &str,
) -> Result<hexagon_core::files::RepoFileSnapshot, CmdError> {
    with_control(state, |connection| {
        hexagon_core::files::read_repo_file_snapshot(
            &connection.root,
            connection.generation,
            path,
            expected_project_root,
        )
    })
}

/// 文件在 git HEAD 的版本（读组，文件页对比基线）。
/// 非仓/未跟踪/无 HEAD → None，基线缺失不是错误。
#[tauri::command]
fn repo_file_head(state: tauri::State<AppState>, path: String) -> Result<Option<String>, CmdError> {
    with_conn(&state, |_db, root| {
        hexagon_core::files::repo_file_at_head(root, &path)
    })
}

/// 新建空文件（控制组，落盘）。
#[tauri::command]
fn create_repo_file(state: ProjectState<'_>, path: String) -> Result<(), CmdError> {
    with_project_conn(&state, |_db, root| {
        hexagon_core::files::create_repo_file(root, &path)
    })
}

/// 新建目录（控制组，落盘）。
#[tauri::command]
fn create_repo_dir(state: ProjectState<'_>, path: String) -> Result<(), CmdError> {
    with_project_conn(&state, |_db, root| {
        hexagon_core::files::create_repo_dir(root, &path)
    })
}

/// 小改写回（控制组，落盘）。不写穿符号链接。
#[tauri::command]
fn write_repo_file(
    state: ProjectState<'_>,
    snapshot: hexagon_core::files::RepoFileSnapshot,
    content: String,
) -> Result<(), CmdError> {
    let generation = state
        .identity
        .as_ref()
        .ok_or_else(|| CmdError {
            code: "project_changed".into(),
            message: "project identity required".into(),
        })?
        .generation;
    with_project_conn(&state, |_, root| {
        hexagon_core::files::write_repo_file_snapshot(root, generation, &snapshot, &content)
    })
}

#[cfg(test)]
fn write_repo_file_for_state(
    state: &AppState,
    snapshot: &hexagon_core::files::RepoFileSnapshot,
    content: &str,
) -> Result<(), CmdError> {
    with_control(state, |connection| {
        hexagon_core::files::write_repo_file_snapshot(
            &connection.root,
            connection.generation,
            snapshot,
            content,
        )
    })
}

/// MCP 服务实况（ui-audit-2 票 06）：需读运行中宿主的状态，走 wb 通道。
/// 2026-09-22：顺手把后台握手完成的工具装进注册表，调用方不用等打开。
#[tauri::command(async)]
fn mcp_services(
    state: tauri::State<AppState>,
) -> Result<Vec<hexagon_core::mcp::McpServiceRow>, CmdError> {
    with_unscoped_wb(&state, |wb| Ok::<_, CmdError>(wb.mcp_services()))
}

// ---------- MCP 全局清单（global-config 票 05，ADR 0057） ----------

/// 全量服务清单（无项目可用）：全局 ∪ 项目（同名项目覆盖全局），
/// 含禁用与远程条目；origin 标来源。配置清单≠授权（ADR 0010）。
#[tauri::command(async)]
fn list_mcp_entries(
    state: tauri::State<AppState>,
) -> Result<Vec<hexagon_core::mcp::McpEntryRow>, CmdError> {
    mcp_entries_for_state(&state)
}

fn mcp_entries_for_state(
    state: &AppState,
) -> Result<Vec<hexagon_core::mcp::McpEntryRow>, CmdError> {
    // Fullstack QA #03 (2026-10-03): opening the MCP grant picker during a
    // model turn blocked the macOS event loop on wb. Snapshot the short control
    // lane root, then release it before filesystem discovery (also projectless).
    let root = state
        .conn
        .lock()
        .map_err(|_| CmdError::internal("lock poisoned"))?
        .as_ref()
        .map(|c| c.root.clone());
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
fn import_mcp(references: Vec<String>) -> hexagon_core::skills::ImportReport {
    hexagon_core::mcp::import_mcp_references(&references)
}

/// 公共 MCP 市场（票 06）：系统默认浏览器打开。URL 是常量非用户输入——
/// 不给任意 URL 开口子（避免变成「以默认浏览器打开任意链接」原语）。
// Owner issue16 (2026-10-02): installation guidance is deterministic UI,
// not an LLM action. A fixed official URL avoids an arbitrary URL opener.
const PLAYWRIGHT_EXTENSION_URL: &str = "https://chromewebstore.google.com/detail/playwright-extension/mmlmfjhmonkocbjadbfplnigmagldckm";

#[tauri::command]
fn open_browser_extension_store() -> Result<(), CmdError> {
    open::that(PLAYWRIGHT_EXTENSION_URL).map_err(|e| CmdError::internal(e.to_string()))
}

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

/// 设置「提示词」页目录（prompt-engineering 票 11）：纯常量，不触工作台。
#[tauri::command]
fn prompt_catalog() -> Vec<hexagon_core::prompts::PromptEntry> {
    hexagon_core::prompts::catalog()
}

/// 提示词参考译文：宿主级 translate 槽（没绑落 default），不属于任何项目，
/// 不触工作台。要调模型，丢到运行时线程免堵主线程。
#[tauri::command(async)]
fn translate_prompts(lang: String, force: bool) -> hexagon_core::prompts::TranslateOutcome {
    hexagon_core::prompts::translate(&lang, force)
}

/// 界面语言进核心（prompt-engineering 票 06）：写宿主 ~/.hexagon/ui.json，
/// 回合读它决定回复语言。无项目可用，不触工作台。
#[tauri::command]
fn set_ui_language(code: String) -> Result<(), CmdError> {
    hexagon_core::uilang::set_language(&code).map_err(|e| CmdError::internal(e.to_string()))
}

#[tauri::command]
fn usage(state: tauri::State<AppState>) -> Result<hexagon_core::usage::UsageSummary, CmdError> {
    with_conn(&state, |db, _| {
        hexagon_core::usage::project_summary(db, PROJECT_ID).map_err(cmd_err)
    })
}

#[tauri::command]
fn usage_context_pressure(
    state: tauri::State<AppState>,
) -> Result<hexagon_core::usage::ContextPressure, CmdError> {
    with_conn(&state, |db, _| {
        hexagon_core::usage::context_pressure(db, PROJECT_ID).map_err(cmd_err)
    })
}

#[tauri::command]
fn open_stage(
    state: ProjectState<'_>,
    seq: usize,
) -> Result<hexagon_core::api::OpenStageOutcome, CmdError> {
    with_wb(&state, |wb| wb.open_stage(seq))
}

#[tauri::command]
fn recover_run(state: ProjectState<'_>, run_id: String) -> Result<(), CmdError> {
    with_wb(&state, |wb| wb.recover_run(&run_id))
}

// Live acceptance #15: delivery hashing/check execution must not block the window.
#[tauri::command(async)]
fn request_acceptance_exception(
    state: ProjectState<'_>,
    expected: String,
) -> Result<hexagon_core::orchestra::ExceptionRequest, CmdError> {
    with_wb(&state, |wb| wb.request_acceptance_exception(&expected))
}
#[tauri::command(async)]
fn confirm_performance_baseline(
    state: ProjectState<'_>,
    measurement: i64,
    expected: String,
    reason: String,
) -> Result<hexagon_core::orchestra::PerformanceBaselineConfirmation, CmdError> {
    with_wb(&state, |wb| {
        wb.confirm_performance_baseline(measurement, &expected, &reason)
    })
}
#[tauri::command(async)]
fn update_quality_commands(
    state: ProjectState<'_>,
    seq: usize,
    expected: String,
    expected_project_root: String,
    commands: std::collections::BTreeMap<hexagon_core::orchestra::QualityCategory, String>,
) -> Result<hexagon_core::orchestra::QualityConfiguration, CmdError> {
    with_wb(&state, |wb| {
        hexagon_core::api::desktop_check_project(&wb.repo_root, &expected_project_root)
            .map_err(hexagon_core::api::ApiError::BadInput)?;
        wb.update_quality_commands(seq, &expected, &commands)
    })
}
#[tauri::command(async)]
fn cancel_quality_revalidation(
    state: ProjectState<'_>,
    question: String,
    expected_project_root: String,
) -> Result<(), CmdError> {
    with_wb(&state, |wb| {
        hexagon_core::api::desktop_check_project(&wb.repo_root, &expected_project_root)
            .map_err(hexagon_core::api::ApiError::BadInput)?;
        wb.cancel_quality_revalidation(&question)
    })
}
#[tauri::command(async)]
fn confirm_quality_revalidation(
    state: ProjectState<'_>,
    question: String,
    expected: String,
    expected_project_root: String,
) -> Result<hexagon_core::orchestra::StageAction, CmdError> {
    with_wb(&state, |wb| {
        hexagon_core::api::desktop_check_project(&wb.repo_root, &expected_project_root)
            .map_err(hexagon_core::api::ApiError::BadInput)?;
        wb.confirm_quality_revalidation(&question, &expected)
    })
}
// Live acceptance #15: delivery hashing/check execution must not block the window.
#[tauri::command(async)]
fn accept_delivery_exception(
    state: ProjectState<'_>,
    question: String,
    expected: String,
    selected: Vec<hexagon_core::orchestra::ExceptionRequirement>,
    reason: String,
) -> Result<hexagon_core::orchestra::ExceptionAcceptance, CmdError> {
    with_wb(&state, |wb| {
        wb.accept_delivery_exception(&question, &expected, &selected, &reason)
    })
}
#[tauri::command]
fn cancel_acceptance_exception(state: ProjectState<'_>, question: String) -> Result<(), CmdError> {
    with_wb(&state, |wb| wb.cancel_acceptance_exception(&question))
}

#[tauri::command]
fn override_checks(
    state: ProjectState<'_>,
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
    state: ProjectState<'_>,
    agent_id: String,
    patch: hexagon_core::roles::AgentPatch,
) -> Result<(), CmdError> {
    // 纯 DB 变更——控制通道直落（票 05：不占 wb 锁）
    with_project_conn(&state, |db, _| {
        hexagon_core::roles::update_agent_def(db, PROJECT_ID, &agent_id, &patch).map_err(cmd_err)
    })
}

#[tauri::command]
fn create_role(
    state: ProjectState<'_>,
    def: hexagon_core::presets::RoleDef,
) -> Result<String, CmdError> {
    with_project_conn(&state, |db, _| {
        hexagon_core::roles::create_role(db, PROJECT_ID, &def).map_err(cmd_err)
    })
}

#[tauri::command]
fn set_agent_grants(
    state: ProjectState<'_>,
    agent_id: String,
    kind: String,
    names: Vec<String>,
) -> Result<(), CmdError> {
    with_project_conn(&state, |db, _| {
        hexagon_core::roles::set_grants(db, &agent_id, &kind, &names).map_err(cmd_err)
    })
}

/// 票 04：技能/MCP 授权确认。L4 在核内自动写入项目 grants；L0–L3 只入队。
#[tauri::command]
fn request_grant(
    state: ProjectState<'_>,
    agent_id: String,
    kind: String,
    name: String,
) -> Result<hexagon_core::grants::GrantOutcome, CmdError> {
    with_project_conn(&state, |db, _| {
        hexagon_core::grants::request(db, PROJECT_ID, &agent_id, &kind, &name).map_err(cmd_err)
    })
}

#[tauri::command]
fn confirm_grant(
    state: ProjectState<'_>,
    qid: String,
    allow: bool,
) -> Result<hexagon_core::grants::GrantOutcome, CmdError> {
    with_project_conn(&state, |db, _| {
        hexagon_core::grants::confirm(db, PROJECT_ID, &qid, allow).map_err(cmd_err)
    })
}

#[tauri::command]
fn draft_role_def(
    state: ProjectState<'_>,
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
fn save_pack_draft(state: ProjectState<'_>, pack_json: String) -> Result<(), CmdError> {
    // 草稿写 .hexagon/pack.json（非 active 副本）——控制通道可落
    with_project_conn(&state, |db, root| {
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
fn export_pack_yaml(state: ProjectState<'_>, dest: String) -> Result<(), CmdError> {
    with_project_conn(&state, |_, root| {
        hexagon_core::packedit::export_yaml(root, std::path::Path::new(&dest)).map_err(cmd_err)
    })
}

#[tauri::command]
fn resolve_install(
    state: ProjectState<'_>,
    qid: String,
    allow: bool,
) -> Result<hexagon_core::install::InstallOutcome, CmdError> {
    // 放行/驳回 + 落盘执行全在 Db+repo_root——控制通道直落（票 05）
    with_project_conn(&state, |db, root| {
        hexagon_core::install::resolve_install(db, PROJECT_ID, root, &qid, allow).map_err(cmd_err)
    })
}

#[tauri::command]
fn export_events(
    state: ProjectState<'_>,
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
    with_project_conn(&state, |db, _| {
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
fn set_autonomy(state: ProjectState<'_>, level: String) -> Result<(), CmdError> {
    // 档位是 projects 行旋钮（控制组）：回合途中改档即时落库，下回合生效
    with_project_conn(&state, |db, _| {
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
fn set_reviewer_mode(state: ProjectState<'_>, mode: String) -> Result<(), CmdError> {
    with_project_conn(&state, |db, _| {
        hexagon_core::autonomy::set_reviewer_mode(db, PROJECT_ID, &mode).map_err(cmd_err)
    })
}

#[tauri::command]
fn owner_away(state: ProjectState<'_>) -> Result<(), CmdError> {
    with_project_conn(&state, |db, _| {
        hexagon_core::autonomy::leave(db, PROJECT_ID)
            .map(|_| ())
            .map_err(cmd_err)
    })
}

#[tauri::command]
fn owner_back(state: ProjectState<'_>) -> Result<hexagon_core::autonomy::ReturnSummary, CmdError> {
    with_project_conn(&state, |db, _| {
        hexagon_core::autonomy::back(db, PROJECT_ID).map_err(cmd_err)
    })
}

#[tauri::command]
fn reject_stamp(
    state: ProjectState<'_>,
    stage: Option<String>,
    note: Option<String>,
) -> Result<hexagon_core::orchestra::StageAction, CmdError> {
    with_wb(&state, |wb| match (stage.as_deref(), note.as_deref()) {
        (Some(stage), Some(note)) => wb.reject_final(stage, note),
        _ => wb.reject_stamp(),
    })
}

/// ADR 0052 wb 组：context_overflow 放行时内联续跑一回合——必须持 wb 锁。
/// 异步化与否见 report.md 待验证假设，本票不碰语义。
#[tauri::command]
fn adjudicate_flag(
    state: ProjectState<'_>,
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

#[tauri::command]
fn curate_legacy_experience(
    state: ProjectState<'_>,
    request: hexagon_core::experience::ExperienceCuration,
) -> Result<hexagon_core::experience::ExperienceSubmission, CmdError> {
    with_wb(&state, |wb| wb.curate_legacy_experience(&request))
        .map(|proposal_id| hexagon_core::experience::ExperienceSubmission { proposal_id })
}

#[tauri::command]
fn revoke_experience(
    state: ProjectState<'_>,
    request: hexagon_core::experience::ExperienceRevocation,
) -> Result<hexagon_core::experience::ExperienceEntryView, CmdError> {
    with_wb(&state, |wb| wb.revoke_experience(&request))
}

#[tauri::command]
fn project_skill_document(
    state: tauri::State<AppState>,
    skill: String,
) -> Result<hexagon_core::experience::ProjectSkillDocument, CmdError> {
    with_conn(&state, |db, root| {
        hexagon_core::experience::read_project_skill(
            db,
            &hexagon_core::tools::ToolContext::owner(db, root),
            &skill,
        )
        .map_err(cmd_err)
    })
}
#[tauri::command]
fn save_project_skill_document(
    state: ProjectState<'_>,
    document: hexagon_core::experience::ProjectSkillDocument,
) -> Result<hexagon_core::experience::ProjectSkillDocument, CmdError> {
    with_wb(&state, |wb| wb.save_project_skill_document(&document))
}

#[tauri::command]
fn experience_limits(
    state: tauri::State<AppState>,
) -> Result<hexagon_core::experience::ExperienceLimits, CmdError> {
    with_conn(&state, |db, _| {
        hexagon_core::experience::loading_limits(db, PROJECT_ID).map_err(cmd_err)
    })
}
#[tauri::command]
fn set_experience_limits(
    state: ProjectState<'_>,
    limits: hexagon_core::experience::ExperienceLimits,
) -> Result<hexagon_core::experience::ExperienceLimits, CmdError> {
    with_wb(&state, |wb| wb.set_experience_limits(&limits))
}

#[tauri::command]
fn recover_experience(
    state: ProjectState<'_>,
) -> Result<Vec<hexagon_core::experience::ExperienceRecovery>, CmdError> {
    with_wb_mut(&state, |wb| wb.recover_experience())
}

#[tauri::command]
fn experience_source_document(
    state: tauri::State<AppState>,
    request: hexagon_core::experience::ExperienceSourceRequest,
) -> Result<hexagon_core::experience::ExperienceSourceDocument, CmdError> {
    with_conn(&state, |db, root| {
        hexagon_core::experience::source_document(
            db,
            &hexagon_core::tools::ToolContext::owner(db, root),
            &request,
        )
        .map_err(cmd_err)
    })
}

#[tauri::command]
fn experience_history(
    state: tauri::State<AppState>,
    request: hexagon_core::experience::ExperienceHistoryRequest,
) -> Result<hexagon_core::experience::ExperienceHistoryPage, CmdError> {
    with_conn(&state, |db, root| {
        hexagon_core::experience::history(
            db,
            &hexagon_core::tools::ToolContext::owner(db, root),
            &request,
        )
        .map_err(cmd_err)
    })
}

#[tauri::command]
fn experience_entries(
    state: tauri::State<AppState>,
    skill: String,
) -> Result<Vec<hexagon_core::experience::ExperienceEntryView>, CmdError> {
    with_conn(&state, |db, root| {
        hexagon_core::experience::entries(
            db,
            &hexagon_core::tools::ToolContext::owner(db, root),
            &skill,
        )
        .map_err(cmd_err)
    })
}

#[tauri::command]
fn experience_proposal(
    state: tauri::State<AppState>,
    proposal_id: String,
) -> Result<Option<hexagon_core::experience::ExperienceProposalView>, CmdError> {
    with_conn(&state, |db, _| {
        hexagon_core::experience::proposal_view(db, PROJECT_ID, &proposal_id).map_err(cmd_err)
    })
}

#[tauri::command]
fn propose_experience_entry(
    state: ProjectState<'_>,
    agent_id: String,
    request: hexagon_core::experience::ExperienceRequest,
) -> Result<hexagon_core::experience::ExperienceSubmission, CmdError> {
    with_wb(&state, |wb| {
        wb.propose_experience_entry(&agent_id, &request)
    })
    .map(|proposal_id| hexagon_core::experience::ExperienceSubmission { proposal_id })
}

/// in_review 提案的负责人裁决（ui-audit-2 票 08）：原签名要 reviewer_agent——
/// 但复审 agent 从无工具可调 review（in_review 曾是无出口死态），唯一真实
/// 裁决面是 owner，署名 owner 比冒名复审 agent 诚实。
#[tauri::command]
fn review_proposal(
    state: ProjectState<'_>,
    proposal_id: String,
    pass: bool,
    reason: String,
) -> Result<(), CmdError> {
    // D01-ok: 执行判定要读工作台里的 Jev 槽，壳层不自己选模型。
    with_wb(&state, |wb| wb.review_proposal(&proposal_id, pass, &reason))
}

#[tauri::command]
fn confirm_proposal(state: ProjectState<'_>, qid: String) -> Result<String, CmdError> {
    with_wb_mut(&state, |wb| wb.confirm_proposal(&qid))
}

#[tauri::command]
fn reject_proposal(state: ProjectState<'_>, qid: String, reason: String) -> Result<(), CmdError> {
    with_project_conn(&state, |db, root| {
        let ctx = hexagon_core::tools::ToolContext::owner(db, root);
        hexagon_core::proposals::reject_at_stamp(db, &ctx, &qid, &reason).map_err(cmd_err)
    })
}

/// 票 07：不变量伴随件手动体检入口——返回违规数（0=trace 自洽），
/// 违规明细落 invariant_violation 事件。
/// ADR 0052 wb 组：回合中段跑会把在途回合误报成悬挂回合——要静止态。
#[tauri::command]
fn invariant_check(state: ProjectState<'_>) -> Result<usize, CmdError> {
    // 独立路径重校验 trace——纯 Db 读+违规事件写，走控制通道（票 05 移出 wb 组）
    with_project_conn(&state, |db, _| {
        hexagon_core::invariant::check_and_log(db, PROJECT_ID).map_err(cmd_err)
    })
}

/// 票 10：流程优化提案的确定性入口——旋钮编辑 JSON + 场景 JSON →
/// 副本改旋钮 → 回放 → 携证据+judge 判定进普通提案队列。
/// 产出物无特权通道（submit 全校验+负责人盖章不变）。
/// ADR 0052 wb 组：内联双回放要锁 wb——异步化见 report.md 待验证假设。
#[tauri::command]
fn policydev_propose(
    state: ProjectState<'_>,
    edits: Vec<Value>,
    scenario: Value,
    motive: String,
) -> Result<String, CmdError> {
    let sc: hexagon_core::scenario::Scenario = serde_json::from_value(scenario).map_err(cmd_err)?;
    with_wb(&state, |wb| wb.policydev_propose(&edits, &sc, &motive))
}

#[tauri::command]
fn rollback_proposal(state: ProjectState<'_>, proposal_id: String) -> Result<(), CmdError> {
    with_wb_mut(&state, |wb| wb.rollback_proposal(&proposal_id))
}

#[tauri::command]
fn request_publish(state: ProjectState<'_>, remote: String) -> Result<String, CmdError> {
    // 发布请求只是入队待决卡（控制组）；push 在 confirm_publish（wb 组，要 creds）
    with_project_conn(&state, |db, _| {
        hexagon_core::publish::request(db, PROJECT_ID, &remote).map_err(cmd_err)
    })
}

#[tauri::command]
fn confirm_publish(
    state: ProjectState<'_>,
    qid: String,
) -> Result<hexagon_core::publish::PublishOutcome, CmdError> {
    with_wb(&state, |wb| wb.confirm_publish(&qid))
}

#[tauri::command]
fn reject_publish(state: ProjectState<'_>, qid: String) -> Result<(), CmdError> {
    with_project_conn(&state, |db, _| {
        hexagon_core::publish::reject(db, PROJECT_ID, &qid).map_err(cmd_err)
    })
}

#[tauri::command]
fn set_agent_avatar(
    state: ProjectState<'_>,
    agent_id: String,
    data_url: String,
) -> Result<(), CmdError> {
    // 纯文件写（avatars/ 目录）——控制通道取 root 即可
    with_project_conn(&state, |_, root| {
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
    state: ProjectState<'_>,
    agent_id: String,
    sleeping: bool,
) -> Result<(), CmdError> {
    // 休眠/唤醒是干预指令（ADR 0052 控制组）
    with_project_conn(&state, |db, _| {
        hexagon_core::orchestra::set_agent_sleeping(db, PROJECT_ID, &agent_id, sleeping)
            .map_err(cmd_err)
    })
}

#[tauri::command]
fn set_usage_limit(state: ProjectState<'_>, limit_cents: Option<i64>) -> Result<(), CmdError> {
    with_project_conn(&state, |db, _| {
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

/// recents.json 行（持久化形状，不导出）。
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
struct RecentStored {
    dir: String,
    name: String,
    mode: String,
    opened_at: u64,
}

/// 最近项目条目（ADR 0054）：IPC 返回形状。exists 不落盘、读列表时现算——
/// 磁盘上删掉目录后条目仍在，由启动页红色标出并给移除钮
///（owner 反馈：静默消失让人以为数据丢了）。
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, ts_rs::TS)]
#[ts(export, export_to = "../../ui/src/gen/")]
struct RecentProject {
    dir: String,
    name: String,
    mode: String,
    #[ts(type = "number")] // JS number 域
    opened_at: u64,
    exists: bool,
}

fn read_recents(app: &tauri::AppHandle) -> Vec<RecentStored> {
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
        RecentStored {
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
    // 不再 is_dir 过滤：已删目录也返回（exists=false），启动页标红 + 移除钮
    // 由人清理。过滤掉是隐形丢条目——目录没了之后点开报错且无入口删除。
    read_recents(&app)
        .into_iter()
        .map(|r| RecentProject {
            exists: std::path::Path::new(&r.dir).is_dir(),
            dir: r.dir,
            name: r.name,
            mode: r.mode,
            opened_at: r.opened_at,
        })
        .collect()
}

/// 启动页最近列表的手动移除（×钮）：只改 recents.json，不碰磁盘上的项目。
#[tauri::command]
fn remove_recent(app: tauri::AppHandle, dir: String) {
    let mut rs = read_recents(&app);
    rs.retain(|r| r.dir != dir);
    if let Some(p) = recents_path(&app) {
        let _ = std::fs::write(&p, serde_json::to_string(&rs).unwrap_or_default());
    }
}

/// 重新打开已有项目：钉住的包副本恢复 pack，fastpath 项目无副本即 None。
/// 2026-09-22：不标 async 时整段打开堵在主线程，启动页和向导都会卡死。
#[tauri::command(async)]
fn open_recent(
    app: tauri::AppHandle,
    state: ProjectState<'_>,
    dir: String,
) -> Result<(), CmdError> {
    check_project_state(&state, true)?;
    // ADR 0060：已有工作台走打开。落库/花名册不在这里重跑；name 以库内为准。
    let mut wb = hexagon_core::setup::open_existing(&dir).map_err(cmd_err)?;
    wb.attach_providers(hexagon_core::credentials::active());
    let info = hexagon_core::orchestra::project_info(&wb.db, PROJECT_ID).map_err(cmd_err)?;
    let control = open_control(&dir)?;
    attach_delta_hook(&app, &wb, &control.identity());
    replace_project(&state, Some(wb), Some(control), &state.identity)?;
    remember_recent(&app, &dir, &info.name, &info.mode);
    Ok(())
}

/// 关闭当前项目回启动页（不删任何数据）。
/// 2026-09-23：必须 async——wb drop 链会跑 McpHost 子进程清理，
/// 同步命令钉在主线程上，任何停顿都冻结整个 UI（与 open_recent
/// 2026-09-22 注释同一事故形态）。
#[tauri::command(async)]
fn close_project(state: ProjectState<'_>) -> Result<(), CmdError> {
    replace_project(&state, None, None, &state.identity)
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
    // diagnostic-records 票 04：拨动开关留一条「宿主」记录能看出开或关。
    // 关掉要先记再收口（收口后 Debug 不落盘）；开着要收口后再记才落得上。
    let t = std::time::Instant::now();
    if !enabled {
        hexagon_core::diag::host("log_toggle", "off", t);
    }
    log::set_max_level(level);
    if enabled {
        hexagon_core::diag::host("log_toggle", "on", t);
    }
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

/// 设置「日志」页读回（diagnostic-records 票 01）：转发 core 的同一份
/// JSONL 账本。project=None → 只回宿主记录；class=None → 全部四类。
/// 开关的 Debug 过滤在 core 读回侧收口，壳层不重复过滤。
#[tauri::command]
fn diagnostic_records(
    project: Option<String>,
    class: Option<String>,
) -> Vec<hexagon_core::diag::DiagRecord> {
    hexagon_core::diag::records(project.as_deref(), class.as_deref())
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

/// web 搜索槽（code-search 票 03）：配后端 + 可选 key + 热刷新；
/// key 进 keychain（`search/<backend>`），不进 providers.json。
#[tauri::command]
fn save_search_backend(
    state: tauri::State<AppState>,
    backend: String,
    endpoint: Option<String>,
    secret: Option<String>,
) -> Result<(), CmdError> {
    hexagon_core::provider_admin::save_search(
        &backend,
        endpoint,
        secret,
        &*hexagon_core::credentials::active(),
    )
    .map_err(cmd_err)?;
    refresh_providers(&state);
    Ok(())
}

/// 摘掉 web 搜索槽 + 热刷新（keychain 里的 key 保留）。
#[tauri::command]
fn remove_search_backend(state: tauri::State<AppState>) -> Result<(), CmdError> {
    hexagon_core::provider_admin::remove_search().map_err(cmd_err)?;
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

/// 票 16：一句话经主对话模型起草项目说明。不写磁盘——落盘仍是确认后的
/// create_project → write_agents_md。项目还不存在，这条命令不进工作台。
#[tauri::command(async)]
fn optimize_agents_md(
    name: String,
    sentence: String,
    qa: Option<Vec<hexagon_core::setup::BriefQA>>,
) -> Result<String, CmdError> {
    // ADR 0069：项目说明槽，没绑则落到默认槽。不是 Jev，也不是角色起草槽。
    let provider = hexagon_core::provider_admin::authoring_provider(
        hexagon_core::credentials::active(),
        hexagon_core::provider_config::BRIEF_SLOT,
    )
    .map_err(cmd_err)?;
    hexagon_core::setup::optimize_agents_md(
        &name,
        &sentence,
        qa.as_deref().unwrap_or(&[]),
        provider.as_ref(),
    )
    .map_err(cmd_err)
}

/// 2026-09-25 答问优化流：起草 AGENTS.md 前先出答问题目（题数不设上限，每题带选项）。
/// 解析失败/空数组都回落到无答问直出——界面按「跳过讨论」处理，不挡优化。
#[tauri::command(async)]
fn brief_questions(
    name: String,
    sentence: String,
) -> Result<Vec<hexagon_core::setup::BriefQuestion>, CmdError> {
    let provider = hexagon_core::provider_admin::authoring_provider(
        hexagon_core::credentials::active(),
        hexagon_core::provider_config::BRIEF_SLOT,
    )
    .map_err(cmd_err)?;
    hexagon_core::setup::brief_questions(&name, &sentence, provider.as_ref()).map_err(cmd_err)
}

/// 按项目说明批量起草勾选角色的职责段落（走角色起草槽）。
/// 只回种子草稿——界面决定哪些落 roleOverrides；已定制的角色不入参。
/// ADR 0075：不再产归属 globs（空目录上纯属虚构），字段恒空。
#[tauri::command(async)]
fn draft_role_defs(
    brief: String,
    roles: Vec<hexagon_core::presets::RoleDef>,
) -> Result<Vec<hexagon_core::setup::RoleSeedDraft>, CmdError> {
    let provider = hexagon_core::provider_admin::authoring_provider(
        hexagon_core::credentials::active(),
        hexagon_core::provider_config::ROLE_DRAFT_SLOT,
    )
    .map_err(cmd_err)?;
    hexagon_core::setup::draft_role_defs(&brief, &roles, provider.as_ref()).map_err(cmd_err)
}

/// 按项目说明起草流程包。用流程起草槽，没绑则落到默认槽。
#[tauri::command(async)]
fn read_instruction_file(dir: String) -> Result<String, CmdError> {
    hexagon_core::setup::read_instruction_file(std::path::Path::new(&dir)).map_err(cmd_err)
}

#[tauri::command(async)]
fn draft_role_duty(name: String, hint: String) -> Result<String, CmdError> {
    let provider = hexagon_core::provider_admin::authoring_provider(
        hexagon_core::credentials::active(),
        hexagon_core::provider_config::ROLE_DRAFT_SLOT,
    )
    .map_err(cmd_err)?;
    hexagon_core::setup::draft_role_duty(&name, &hint, provider.as_ref()).map_err(cmd_err)
}

#[tauri::command(async)]
fn draft_flow(
    sentence: String,
    roles: Vec<String>,
) -> Result<hexagon_core::orchestra::PackDef, CmdError> {
    let provider = hexagon_core::provider_admin::authoring_provider(
        hexagon_core::credentials::active(),
        hexagon_core::provider_config::FLOW_DRAFT_SLOT,
    )
    .map_err(cmd_err)?;
    hexagon_core::setup::draft_flow(&sentence, &roles, provider.as_ref()).map_err(cmd_err)
}

// 同 send_message（2026-09-22）：不标 async 时创建整段堵在主线程，
// on_progress 的五步要等命令返回才一起亮。票 14 当时写「同步命令跑在
// 阻塞池」是错的，默认是 Blocking。
#[tauri::command(async)]
fn create_project(
    app: tauri::AppHandle,
    state: ProjectState<'_>,
    opts: hexagon_core::setup::CreateProjectOpts,
    on_progress: tauri::ipc::Channel<hexagon_core::setup::CreateStep>,
) -> Result<(), CmdError> {
    check_project_state(&state, true)?;
    use hexagon_core::setup;
    // 说明文件在核里、而且在「空不空」判定之后才写。先写的话，空目录的
    // 一句话会把目录变成非空，误走开场分析（票 17）。
    let pack = if let Some(pack) = opts.pack.clone() {
        Some(pack)
    } else {
        opts.pack_name
            .as_deref()
            .map(|n| {
                hexagon_core::presets::preset_packs()
                    .map_err(cmd_err)?
                    .into_iter()
                    .find(|p| p.name == n)
                    .ok_or_else(|| CmdError::internal(format!("未知流程包: {n}")))
            })
            .transpose()?
    };
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
        opts.autonomy.as_deref(),
        opts.agents_md.as_deref(),
        |step| {
            let _ = on_progress.send(step);
        },
    )
    .map_err(cmd_err)?;
    // 接线尾步归 Workbench：凭据库 + 按文档注册运行槽位（票 05）
    wb.attach_providers(hexagon_core::credentials::active());
    let control = open_control(&opts.dir)?;
    attach_delta_hook(&app, &wb, &control.identity());
    replace_project(&state, Some(wb), Some(control), &state.identity)?;
    remember_recent(
        &app,
        &opts.dir,
        &opts.name,
        if opts.pack.is_some() || opts.pack_name.is_some() {
            "pack"
        } else {
            "fastpath"
        },
    );
    Ok(())
}

/// 票 17：进工作台之后的只读开场分析。模型调用不占工作台锁，
/// 负责人这时仍能经控制连接把字写进时间线。
#[tauri::command(async)]
fn run_opening_intake(state: ProjectState<'_>) -> Result<(), CmdError> {
    use hexagon_core::api::IntakePrepared;
    let prepared = with_wb(&state, |wb| wb.prepare_opening_intake())?;
    match prepared {
        IntakePrepared::Finished(_) => Ok(()),
        IntakePrepared::Call {
            request,
            provider,
            context,
        } => {
            let resp = match hexagon_core::usage::complete_project_request(
                &context,
                provider.as_ref(),
                &request,
                "opening_intake",
            ) {
                Ok(resp) => resp,
                Err(e) => {
                    let _ = with_wb(&state, |wb| wb.abort_opening_intake());
                    return Err(cmd_err(e));
                }
            };
            let text = hexagon_core::intake::response_text(&resp);
            with_wb(&state, |wb| match wb.commit_opening_intake(&text) {
                Ok(_) => Ok(()),
                Err(error) => {
                    let _ = wb.abort_opening_intake();
                    Err(error)
                }
            })
        }
    }
}

/// 负责人点头后才把开场草案写成 AGENTS.md。L4 不走这里。
#[tauri::command]
fn confirm_intake_brief(state: ProjectState<'_>) -> Result<(), CmdError> {
    with_wb(&state, |wb| wb.confirm_intake_brief())
}

/// 还有没有等人点头的草案。读控制连接，不进工作台锁。
#[tauri::command]
fn intake_draft_pending(state: tauri::State<AppState>) -> Result<bool, CmdError> {
    with_conn(&state, |db, _| -> Result<bool, hexagon_core::db::DbError> {
        let draft: Option<String> = db.conn().query_row(
            "SELECT intake_draft FROM projects WHERE id=?1",
            [PROJECT_ID],
            |r| r.get(0),
        )?;
        Ok(draft.is_some_and(|s| !s.trim().is_empty()))
    })
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

#[tauri::command(async)]
fn reconcile_tool_action(state: ProjectState<'_>, action_id: String) -> Result<(), CmdError> {
    with_wb(&state, |wb| match wb.reconcile_tool_action(&action_id) {
        // A completed read-only check may leave uncertainty. The refreshed card
        // shows its evidence; it is not a failed UI command or execution success.
        Err(hexagon_core::api::ApiError::Tool(hexagon_core::tools::ToolError::OutcomeUnknown(
            _,
        ))) => Ok(()),
        result => result.map(|_| ()),
    })
}

#[tauri::command(async)]
fn abandon_tool_action(
    state: ProjectState<'_>,
    action_id: String,
    reason: String,
    expected_project_root: String,
) -> Result<(), CmdError> {
    // Live acceptance 2026-10-01: pure owner resolution must not queue behind
    // another role's whole model turn. Execution/reconciliation remain on wb.
    with_project_conn(&state, |db, root| {
        hexagon_core::api::abandon_tool_action_control(
            db,
            PROJECT_ID,
            root,
            &expected_project_root,
            &action_id,
            &reason,
        )
    })
}

#[tauri::command(async)]
fn retry_tool_action(
    state: ProjectState<'_>,
    action_id: String,
    reason: String,
    accepts_duplicate: bool,
) -> Result<(), CmdError> {
    with_wb(&state, |wb| {
        wb.retry_tool_action(&action_id, &reason, accepts_duplicate)
            .map(|_| ())
    })
}

#[tauri::command(async)]
fn resume_tool_action(state: ProjectState<'_>, action_id: String) -> Result<(), CmdError> {
    with_wb(&state, |wb| wb.resume_tool_action(&action_id).map(|_| ()))
}

#[tauri::command(async)]
fn dispatch(
    state: ProjectState<'_>,
    role: String,
    input: String,
    attachments: Vec<hexagon_core::trace::AttachRef>,
) -> Result<hexagon_core::turn::TurnOutcome, CmdError> {
    with_wb(&state, |wb| wb.dispatch(&role, &input, &attachments))
}

#[tauri::command]
fn upgrade_to_pack(state: ProjectState<'_>, pack_name: String) -> Result<(), CmdError> {
    let pack = hexagon_core::presets::preset_packs()
        .map_err(cmd_err)?
        .into_iter()
        .find(|p| p.name == pack_name)
        .ok_or_else(|| CmdError::internal(format!("未知流程包: {pack_name}")))?;
    with_wb_mut(&state, |wb| wb.upgrade_to_pack(pack))
}

/// 失速卡「再试一次」（stall-watch 票 02/04）：会跑回合或唤醒项目经理，turn 组。
#[tauri::command(async)]
fn stall_retry(state: ProjectState<'_>, question_id: String) -> Result<(), CmdError> {
    with_wb(&state, |wb| wb.stall_retry(&question_id).map(|_| ()))
}

/// 失速卡「知道了」：写注记、销卡、收场。要改工作台里的监视状态，走 wb。
#[tauri::command(async)]
fn stall_ack(state: ProjectState<'_>, question_id: String) -> Result<(), CmdError> {
    with_wb(&state, |wb| wb.stall_ack(&question_id))
}

/// 失速监视的节拍（ADR 0074：监视必须在没有任何 Agent 发言时也能跑）。
/// try_lock 拿不到 = 有回合或裁决占着工作台 = 回合在飞，这一拍不计。
/// 被否决：阻塞 lock——节拍会排在回合后面，回合一结束就补跑一拍，
/// 等于把在飞时间算进时钟。
const STALL_TICK: std::time::Duration = std::time::Duration::from_secs(2);

fn spawn_stall_watch(app: tauri::AppHandle) {
    std::thread::spawn(move || {
        loop {
            std::thread::sleep(STALL_TICK);
            let state = app.state::<AppState>();
            let Ok(g) = state.wb.try_lock() else {
                // D01-ok: 失速节拍是 mutation 路径（可能重触发/入卡）；拿不到锁即回合在飞
                continue;
            };
            if let Some(wb) = g.as_ref() {
                if let Err(e) = wb.stall_tick() {
                    log::warn!("stall tick: {e}");
                }
            }
        }
    });
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
                    // 2026-09-22：Debug 全灌进 webview 时，一轮工具调用的 ureq 日志
                    // 就能把界面刷死。文件和 stdout 仍留 Debug。
                    tauri_plugin_log::Target::new(tauri_plugin_log::TargetKind::Webview)
                        .filter(|meta| meta.level() <= log::Level::Warn),
                ])
                .max_file_size(5_000_000)
                .rotation_strategy(tauri_plugin_log::RotationStrategy::KeepSome(5))
                .level(log::LevelFilter::Debug)
                .build(),
        )
        .setup(|app| {
            // Computer Use has a fixed bundled path. Missing components remain
            // visible as unavailable; startup itself still works without them.
            if let Ok(path) = desktop_library_path(app.handle()) {
                if let Err(error) = hexagon_core::api::desktop_initialize(&path) {
                    log::warn!("computer-use component unavailable: {error}");
                }
            }
            let browser_worker = if cfg!(debug_assertions) {
                std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
                    .join("../native/browser/worker.mjs")
            } else {
                app.path()
                    .resource_dir()
                    .unwrap_or_default()
                    .join("browser/worker.mjs")
            };
            let bundled_node = browser_worker
                .parent()
                .unwrap_or(std::path::Path::new(""))
                .join(if cfg!(windows) { "node.exe" } else { "node" });
            let host_node = [
                "/opt/homebrew/bin/node",
                "/usr/local/bin/node",
                "/usr/bin/node",
            ]
            .iter()
            .map(std::path::PathBuf::from)
            .find(|p| p.is_file());
            if let Some(node) = bundled_node.is_file().then_some(bundled_node).or(host_node) {
                if let Err(error) = hexagon_core::api::browser_initialize(&node, &browser_worker) {
                    log::warn!("browser runtime unavailable: {error}");
                }
            }
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
            // diagnostic-records 票 04：结构化记录与文本日志同一目录、
            // 同一开关闸（上面的 set_max_level 之后写，关着就不落盘）。
            if let Ok(dir) = app.path().app_log_dir() {
                hexagon_core::diag::set_dir(&dir);
            }
            let boot = std::time::Instant::now();
            hexagon_core::diag::host("startup", "app_start", boot);
            hexagon_core::diag::host(
                "credentials",
                hexagon_core::credentials::backend_kind(),
                boot,
            );
            let sb = hexagon_core::sandbox::status();
            hexagon_core::diag::host(
                "sandbox",
                &if sb.available {
                    sb.mode
                } else {
                    format!("unavailable:{}", sb.mode)
                },
                boot,
            );
            spawn_stall_watch(app.handle().clone());
            Ok(())
        })
        .manage(AppState {
            wb: Mutex::new(None),
            conn: Mutex::new(None),
        })
        .invoke_handler(tauri::generate_handler![
            project_identity,
            core_ping,
            sandbox_status,
            design_direction,
            choose_design_direction,
            desktop_preview_target,
            desktop_preview,
            desktop_preview_stop,
            desktop_preview_focus,
            desktop_screenshot,
            desktop_status,
            computer_models,
            browser_status,
            browser_open,
            browser_detach,
            browser_preview,
            browser_focus,
            desktop_control,
            desktop_permissions,
            desktop_open_settings,
            data_boundary,
            open_project,
            timeline,
            timeline_window,
            timeline_window_metadata,
            timeline_facts,
            timeline_nodes,
            send_message,
            browser_selection_start,
            browser_selection_poll,
            browser_selection_discard,
            stage_attachment,
            discard_attachments,
            answer_permission,
            allow_project_permission,
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
            stage_evidence,
            quality_configuration,
            current_process_pack,
            pending_questions,
            permission_rules,
            permission_shape_suggestion,
            approval_mode,
            set_approval_mode,
            revoke_permission_rule,
            list_skills,
            set_skill_muted,
            set_ui_language,
            prompt_catalog,
            translate_prompts,
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
            open_browser_extension_store,
            repo_paths,
            list_repo_dir,
            read_repo_file,
            read_repo_file_snapshot,
            repo_file_head,
            create_repo_file,
            create_repo_dir,
            write_repo_file,
            usage,
            usage_context_pressure,
            open_stage,
            recover_run,
            reconcile_tool_action,
            abandon_tool_action,
            retry_tool_action,
            resume_tool_action,
            stall_retry,
            stall_ack,
            override_checks,
            request_acceptance_exception,
            confirm_performance_baseline,
            update_quality_commands,
            cancel_quality_revalidation,
            confirm_quality_revalidation,
            accept_delivery_exception,
            cancel_acceptance_exception,
            agent_detail,
            update_agent,
            create_role,
            set_agent_grants,
            request_grant,
            confirm_grant,
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
            experience_proposal,
            experience_entries,
            experience_history,
            experience_source_document,
            recover_experience,
            experience_limits,
            project_skill_document,
            revoke_experience,
            curate_legacy_experience,
            save_project_skill_document,
            set_experience_limits,
            propose_experience_entry,
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
            diagnostic_records,
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
            save_search_backend,
            remove_search_backend,
            fetch_provider_models,
            agents_md_draft,
            optimize_agents_md,
            brief_questions,
            draft_role_defs,
            draft_flow,
            read_instruction_file,
            draft_role_duty,
            create_project,
            run_opening_intake,
            confirm_intake_brief,
            intake_draft_pending,
            project_open,
            project_info,
            dispatch,
            upgrade_to_pack,
            recent_projects,
            open_recent,
            remove_recent,
            close_project,
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}

#[tauri::command]
fn desktop_preview_target(
    state: tauri::State<AppState>,
    expected_project_root: String,
) -> Result<Option<hexagon_core::desktop::preview::NativePreviewTarget>, CmdError> {
    with_conn(&state, |db, root| {
        hexagon_core::api::desktop_preview_target(db, root, &expected_project_root)
            .map_err(CmdError::internal)
    })
}
#[tauri::command]
async fn desktop_preview(
    state: ProjectState<'_>,
    expected_project_root: String,
    window_id: u32,
    process_id: i32,
) -> Result<hexagon_core::desktop::preview::NativePreviewFrame, CmdError> {
    let target = with_project_conn(&state, |db, root| {
        hexagon_core::api::desktop_preview_validate(
            db,
            root,
            &expected_project_root,
            window_id,
            process_id,
        )
        .map_err(CmdError::internal)
    })?;
    let before = target.clone();
    // Never retain the project connection lock while waiting for screen capture.
    let frame =
        tauri::async_runtime::spawn_blocking(move || hexagon_core::api::desktop_preview(&target))
            .await
            .map_err(|error| CmdError::internal(error.to_string()))?
            .map_err(CmdError::internal)?;
    let current = with_project_conn(&state, |db, root| {
        hexagon_core::api::desktop_preview_validate(
            db,
            root,
            &expected_project_root,
            window_id,
            process_id,
        )
        .map_err(CmdError::internal)
    })?;
    if current != before {
        return Err(CmdError::internal("preview target changed"));
    }
    Ok(frame)
}
#[tauri::command]
fn desktop_preview_stop(
    state: ProjectState<'_>,
    expected_project_root: String,
) -> Result<(), CmdError> {
    with_project_conn(&state, |_, root| {
        hexagon_core::api::desktop_check_project(root, &expected_project_root)
            .map_err(CmdError::internal)?;
        hexagon_core::api::desktop_preview_stop().map_err(CmdError::internal)
    })
}
#[tauri::command]
async fn desktop_preview_focus(
    state: ProjectState<'_>,
    expected_project_root: String,
    window_id: u32,
    process_id: i32,
) -> Result<(), CmdError> {
    let target = with_project_conn(&state, |db, root| {
        hexagon_core::api::desktop_preview_validate(
            db,
            root,
            &expected_project_root,
            window_id,
            process_id,
        )
        .map_err(CmdError::internal)
    })?;
    tauri::async_runtime::spawn_blocking(move || hexagon_core::api::desktop_preview_focus(&target))
        .await
        .map_err(|error| CmdError::internal(error.to_string()))?
        .map_err(CmdError::internal)
}

fn browser_project(
    state: &ProjectState<'_>,
    expected: &str,
    require_consent: bool,
) -> Result<std::path::PathBuf, CmdError> {
    with_project_conn(state, |db, root| {
        hexagon_core::api::desktop_check_project(root, expected).map_err(CmdError::internal)?;
        if require_consent
            && !hexagon_core::api::desktop_status(db, root)
                .map_err(CmdError::internal)?
                .enabled
        {
            return Err(CmdError::internal(
                "enable project computer and screenshot sharing first",
            ));
        }
        Ok(root.to_path_buf())
    })
}

#[tauri::command]
async fn browser_status(
    state: ProjectState<'_>,
    expected_project_root: String,
) -> Result<Option<hexagon_core::browser::BrowserSession>, CmdError> {
    let root = browser_project(&state, &expected_project_root, false)?;
    let result =
        tauri::async_runtime::spawn_blocking(move || hexagon_core::api::browser_status(&root))
            .await
            .map_err(|e| CmdError::internal(e.to_string()))?
            .map_err(CmdError::internal)?;
    browser_project(&state, &expected_project_root, false)?;
    Ok(result)
}
#[tauri::command]
async fn browser_open(
    state: ProjectState<'_>,
    expected_project_root: String,
    mode: hexagon_core::browser::BrowserMode,
    labels: hexagon_core::browser::BrowserLabels,
) -> Result<hexagon_core::browser::BrowserSession, CmdError> {
    // Launch/extension handshake can wait; never retain the project connection
    // mutex while doing browser I/O, so pause/revoke stays immediately available.
    let root = browser_project(&state, &expected_project_root, true)?;
    let cleanup = root.clone();
    let result = tauri::async_runtime::spawn_blocking(move || {
        hexagon_core::api::browser_open(&root, mode, labels)
    })
    .await
    .map_err(|e| CmdError::internal(e.to_string()))?
    .map_err(CmdError::internal)?;
    if let Err(error) = browser_project(&state, &expected_project_root, true) {
        // I1 scope review: same-root reopening may already own a new session.
        // Clean up only this completed launch, never every session at its root.
        let _ = hexagon_core::api::browser_detach(&cleanup, &result.session_id);
        return Err(error);
    }
    Ok(result)
}
#[tauri::command]
async fn browser_detach(
    state: ProjectState<'_>,
    expected_project_root: String,
    session_id: String,
) -> Result<(), CmdError> {
    let root = browser_project(&state, &expected_project_root, false)?;
    tauri::async_runtime::spawn_blocking(move || {
        hexagon_core::api::browser_detach(&root, &session_id)
    })
    .await
    .map_err(|e| CmdError::internal(e.to_string()))?
    .map_err(CmdError::internal)
}
#[tauri::command]
async fn browser_preview(
    state: ProjectState<'_>,
    expected_project_root: String,
    session_id: String,
) -> Result<hexagon_core::browser::BrowserPreview, CmdError> {
    let root = browser_project(&state, &expected_project_root, true)?;
    let result = tauri::async_runtime::spawn_blocking(move || {
        hexagon_core::api::browser_preview(&root, &session_id)
    })
    .await
    .map_err(|e| CmdError::internal(e.to_string()))?
    .map_err(CmdError::internal)?;
    browser_project(&state, &expected_project_root, true)?;
    Ok(result)
}
#[tauri::command]
async fn browser_focus(
    state: ProjectState<'_>,
    expected_project_root: String,
    session_id: String,
) -> Result<(), CmdError> {
    let root = browser_project(&state, &expected_project_root, true)?;
    tauri::async_runtime::spawn_blocking(move || {
        hexagon_core::api::browser_focus(&root, &session_id)
    })
    .await
    .map_err(|e| CmdError::internal(e.to_string()))?
    .map_err(CmdError::internal)
}

#[cfg(test)]
mod project_identity_tests {
    use super::*;
    use hexagon_core::provider::{ChatResponse, ContentBlock, ScriptedProvider, StopReason, Usage};
    use hexagon_core::tools::{RiskClass, Tool, ToolContext, ToolError};
    use std::sync::{
        atomic::{AtomicUsize, Ordering},
        Arc,
    };

    struct OwnerEffect(Arc<AtomicUsize>);
    impl Tool for OwnerEffect {
        fn name(&self) -> &str {
            "remote_publish"
        }
        fn description(&self) -> &str {
            "Explicitly approved isolated counter"
        }
        fn input_schema(&self) -> Value {
            serde_json::json!({"type":"object","properties":{},"additionalProperties":false})
        }
        fn risk(&self) -> RiskClass {
            // Same existing safety-net fixture as the core permission tests:
            // no network, and no shadow-review model call consumes the script.
            RiskClass::Exec
        }
        fn exec(
            &self,
            _: &hexagon_core::db::Db,
            _: &Value,
            _: &ToolContext,
        ) -> Result<Value, ToolError> {
            self.0.fetch_add(1, Ordering::SeqCst);
            Ok(serde_json::json!({"confirmed":true}))
        }
    }

    fn waiting_project(root: &std::path::Path, effects: Arc<AtomicUsize>) -> (Workbench, String) {
        let mut wb = Workbench::open_evaluation_host(root).unwrap();
        wb.db
            .conn()
            .execute(
                "INSERT INTO agents(id,project_id,role,status) VALUES('a0','p1','worker','active')",
                [],
            )
            .unwrap();
        wb.registry.register(OwnerEffect(effects));
        wb.register_provider(
            "default",
            Arc::new(ScriptedProvider::new(vec![
                ChatResponse {
                    content: vec![ContentBlock::ToolUse {
                        id: "effect".into(),
                        name: "remote_publish".into(),
                        input: serde_json::json!({}),
                    }],
                    stop: StopReason::ToolUse,
                    usage: Usage::default(),
                },
                ChatResponse {
                    content: vec![ContentBlock::Text {
                        text: "original task complete".into(),
                    }],
                    stop: StopReason::EndTurn,
                    usage: Usage::default(),
                },
            ])),
        );
        let hexagon_core::turn::TurnOutcome::AwaitingPermission(card) = wb
            .run_instance("a0", "Perform the isolated approved counter once")
            .unwrap()
        else {
            panic!("real permission required")
        };
        (wb, card)
    }

    fn identity(root: &std::path::Path, generation: u64) -> ProjectIdentity {
        ProjectIdentity {
            project_root: root.canonicalize().unwrap().to_string_lossy().into_owned(),
            generation,
        }
    }

    #[test]
    fn delayed_permission_cannot_approve_another_projects_same_local_question() {
        let a = tempfile::tempdir().unwrap();
        let b = tempfile::tempdir().unwrap();
        let effects_a = Arc::new(AtomicUsize::new(0));
        let effects_b = Arc::new(AtomicUsize::new(0));
        let (wa, qa) = waiting_project(a.path(), effects_a.clone());
        let (wb, qb) = waiting_project(b.path(), effects_b.clone());
        assert_eq!(qa, qb, "database-local ids collide in real projects");
        let state = AppState {
            wb: Mutex::new(Some(wb)),
            conn: Mutex::new(Some(ControlConn {
                db: hexagon_core::db::Db::open(b.path().join(".hexagon/state.db")).unwrap(),
                root: b.path().canonicalize().unwrap(),
                generation: 42,
            })),
        };
        // A's previously captured invocation is released only after B is current.
        let delayed = ProjectState {
            state: &state,
            identity: Some(identity(a.path(), 41)),
        };
        let result = with_wb(&delayed, |wb| {
            wb.answer_permission_and_continue(&qa, true, None, "activation")
        });
        assert_eq!(effects_a.load(Ordering::SeqCst), 0);
        assert_eq!(effects_b.load(Ordering::SeqCst), 0);
        assert_eq!(result.unwrap_err().code, "project_changed");
        let current = ProjectState {
            state: &state,
            identity: Some(identity(b.path(), 42)),
        };
        with_wb(&current, |wb| {
            assert_eq!(
                hexagon_core::cards::get(&wb.db, &qb).unwrap().state,
                hexagon_core::cards::CardState::Queued
            );
            wb.answer_permission_and_continue(&qb, true, None, "activation")
        })
        .unwrap();
        assert_eq!(effects_b.load(Ordering::SeqCst), 1);
        assert!(with_wb(&current, |wb| wb.answer_permission_and_continue(
            &qb,
            true,
            None,
            "activation"
        ))
        .is_err());
        assert_eq!(effects_b.load(Ordering::SeqCst), 1);
        drop(wa);
    }

    proptest::proptest! {
        #![proptest_config(proptest::test_runner::Config::with_cases(8))]
        #[test]
        fn missing_wrong_root_or_reopened_generation_cannot_pause_the_current_project(case in 0u8..3, generation in 1u64..1000) {
            let a=tempfile::tempdir().unwrap();
            let b=tempfile::tempdir().unwrap();
            let wb=Workbench::open_evaluation_host(b.path()).unwrap();
            let state=AppState { wb:Mutex::new(Some(wb)),conn:Mutex::new(Some(ControlConn { db:hexagon_core::db::Db::open(b.path().join(".hexagon/state.db")).unwrap(),root:b.path().canonicalize().unwrap(),generation })) };
            let scope=ProjectState { state:&state,identity:match case {0=>None,1=>Some(identity(a.path(),generation)),_=>Some(identity(b.path(),generation+1))} };
            let result=with_project_conn(&scope,|db,_|hexagon_core::orchestra::pause(db,PROJECT_ID));
            proptest::prop_assert!(matches!(result,Err(CmdError{ref code,..}) if code=="project_changed"), "invalid scope must reject before pause");
            proptest::prop_assert!(!hexagon_core::orchestra::is_paused(&state.conn.lock().unwrap().as_ref().unwrap().db,PROJECT_ID).unwrap());
        }
    }

    #[test]
    fn absent_or_malformed_identity_is_not_a_lifecycle_null_token() {
        for value in [
            serde_json::json!({}),
            serde_json::json!({"_project":{"project_root":"x","generation":"1"}}),
            serde_json::json!({"_project":{"project_root":"x"}}),
        ] {
            assert!(project_identity_argument(&tauri::ipc::InvokeBody::Json(value)).is_err());
        }
        assert!(project_identity_argument(&tauri::ipc::InvokeBody::Raw(vec![])).is_err());
        assert!(project_identity_argument(&tauri::ipc::InvokeBody::Json(
            serde_json::json!({"_project":null})
        ))
        .unwrap()
        .is_none());
    }

    #[test]
    fn delayed_lifecycle_close_cannot_close_a_new_project_or_a_reopened_root() {
        let a = tempfile::tempdir().unwrap();
        let b = tempfile::tempdir().unwrap();
        for current_root in [b.path(), a.path()] {
            let wb = Workbench::open_evaluation_host(current_root).unwrap();
            let state = AppState {
                wb: Mutex::new(Some(wb)),
                conn: Mutex::new(Some(ControlConn {
                    db: hexagon_core::db::Db::open(current_root.join(".hexagon/state.db")).unwrap(),
                    root: current_root.canonicalize().unwrap(),
                    generation: 22,
                })),
            };
            let stale = Some(identity(a.path(), 21));
            assert_eq!(
                replace_project(&state, None, None, &stale)
                    .unwrap_err()
                    .code,
                "project_changed"
            );
            assert_eq!(state.conn.lock().unwrap().as_ref().unwrap().generation, 22);
            assert!(
                !hexagon_core::orchestra::is_paused(
                    &state.conn.lock().unwrap().as_ref().unwrap().db,
                    PROJECT_ID
                )
                .unwrap(),
                "refusal must precede pausing the new project"
            );
            assert_eq!(
                replace_project(&state, None, None, &None).unwrap_err().code,
                "project_changed"
            );
            replace_project(&state, None, None, &Some(identity(current_root, 22))).unwrap();
            assert!(state.conn.lock().unwrap().is_none());
        }
    }
}

#[cfg(test)]
mod preview_project_lifecycle_tests {
    use super::*;

    fn project_connection(root: &std::path::Path, generation: u64) -> ControlConn {
        let root = root.canonicalize().unwrap();
        let db = hexagon_core::db::Db::open(root.join("state.db")).unwrap();
        db.conn().execute(
            "INSERT INTO projects(id,dir,name,mode,autonomy) VALUES(?1,?2,'lifecycle','pack','L0')",
            [PROJECT_ID, root.to_str().unwrap()],
        ).unwrap();
        ControlConn {
            db,
            root,
            generation,
        }
    }

    #[test]
    fn project_replacement_pauses_before_waiting_and_installs_both_handles_together() {
        // Benchmark I1: the former conn-first swap exposed the new project while
        // its predecessor still held wb. A real second connection observes pause.
        for initially_paused in [false, true] {
            let directory = tempfile::tempdir().unwrap();
            let connection = project_connection(directory.path(), 17);
            let observer = hexagon_core::db::Db::open(connection.root.join("state.db")).unwrap();
            if initially_paused {
                hexagon_core::orchestra::pause(&observer, PROJECT_ID).unwrap();
            }
            let state = std::sync::Arc::new(AppState {
                wb: Mutex::new(None),
                conn: Mutex::new(Some(connection)),
            });
            let turn_lock = state.wb.lock().unwrap(); // D01-exempt: simulate the old parent turn
            let (send, receive) = std::sync::mpsc::channel();
            let worker_state = state.clone();
            let expected = Some(ProjectIdentity {
                project_root: directory
                    .path()
                    .canonicalize()
                    .unwrap()
                    .to_string_lossy()
                    .into_owned(),
                generation: 17,
            });
            let worker = std::thread::spawn(move || {
                send.send(replace_project(&worker_state, None, None, &expected))
                    .unwrap();
            });
            let deadline = std::time::Instant::now() + std::time::Duration::from_secs(2);
            while !hexagon_core::orchestra::is_paused(&observer, PROJECT_ID).unwrap()
                && std::time::Instant::now() < deadline
            {
                std::thread::sleep(std::time::Duration::from_millis(1));
            }
            assert!(hexagon_core::orchestra::is_paused(&observer, PROJECT_ID).unwrap());
            assert_eq!(state.conn.lock().unwrap().as_ref().unwrap().generation, 17);
            assert!(matches!(
                receive.try_recv(),
                Err(std::sync::mpsc::TryRecvError::Empty)
            ));
            drop(turn_lock);
            receive
                .recv_timeout(std::time::Duration::from_secs(2))
                .unwrap()
                .unwrap();
            worker.join().unwrap();
            assert!(state.conn.lock().unwrap().is_none());
            assert!(state.wb.lock().unwrap().is_none()); // D01-exempt: inspect final replacement
            assert_eq!(
                hexagon_core::orchestra::is_paused(&observer, PROJECT_ID).unwrap(),
                initially_paused
            );
        }
    }

    #[test]
    fn a_project_changed_while_waiting_cannot_be_closed_by_the_previous_request() {
        let old = tempfile::tempdir().unwrap();
        let new = tempfile::tempdir().unwrap();
        let connection = project_connection(old.path(), 31);
        let observer = hexagon_core::db::Db::open(connection.root.join("state.db")).unwrap();
        let state = std::sync::Arc::new(AppState {
            wb: Mutex::new(None),
            conn: Mutex::new(Some(connection)),
        });
        let turn_lock = state.wb.lock().unwrap(); // D01-exempt: simulate the old parent turn
        let worker_state = state.clone();
        let expected = Some(ProjectIdentity {
            project_root: old
                .path()
                .canonicalize()
                .unwrap()
                .to_string_lossy()
                .into_owned(),
            generation: 31,
        });
        let worker =
            std::thread::spawn(move || replace_project(&worker_state, None, None, &expected));
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(2);
        while !hexagon_core::orchestra::is_paused(&observer, PROJECT_ID).unwrap()
            && std::time::Instant::now() < deadline
        {
            std::thread::sleep(std::time::Duration::from_millis(1));
        }
        assert!(hexagon_core::orchestra::is_paused(&observer, PROJECT_ID).unwrap());
        *state.conn.lock().unwrap() = Some(project_connection(new.path(), 32));
        drop(turn_lock);
        assert_eq!(worker.join().unwrap().unwrap_err().code, "project_changed");
        assert_eq!(state.conn.lock().unwrap().as_ref().unwrap().generation, 32);
    }

    #[test]
    fn mcp_catalog_remains_available_while_an_agent_holds_the_workbench() {
        // fullstack QA #03：在角色思考时打开授权选择器曾把NS主线程卡在wb锁上。
        let state = std::sync::Arc::new(AppState {
            wb: Mutex::new(None),
            conn: Mutex::new(None),
        });
        let turn_lock = state.wb.lock().unwrap(); // D01-exempt: 模拟回合持有长锁
        let (send, receive) = std::sync::mpsc::channel();
        let worker_state = state.clone();
        let worker = std::thread::spawn(move || {
            send.send(mcp_entries_for_state(&worker_state).map(|rows| rows.len()))
                .unwrap();
        });
        let result = receive.recv_timeout(std::time::Duration::from_secs(1));
        drop(turn_lock);
        worker.join().unwrap();
        assert!(
            result.is_ok(),
            "MCP catalog waited for the active model turn"
        );
        result.unwrap().unwrap();
    }

    #[test]
    fn editor_ipc_rejects_old_project_and_same_root_reopening() {
        let a = tempfile::tempdir().unwrap();
        let b = tempfile::tempdir().unwrap();
        std::fs::write(a.path().join("README.md"), "A saved").unwrap();
        std::fs::write(b.path().join("README.md"), "B saved").unwrap();
        let source_root = a.path().canonicalize().unwrap();
        let state = AppState {
            wb: Mutex::new(None),
            conn: Mutex::new(Some(ControlConn {
                db: hexagon_core::db::Db::open_in_memory().unwrap(),
                root: source_root.clone(),
                generation: 7,
            })),
        };
        let snapshot = read_repo_file_snapshot_for_state(
            &state,
            "README.md",
            &source_root.display().to_string(),
        )
        .unwrap();
        write_repo_file_for_state(&state, &snapshot, "A draft").unwrap();
        for root in [b.path().canonicalize().unwrap(), source_root.clone()] {
            replace_control(
                &state,
                Some(ControlConn {
                    db: hexagon_core::db::Db::open_in_memory().unwrap(),
                    root,
                    generation: 8,
                }),
            )
            .unwrap();
            assert_eq!(
                write_repo_file_for_state(&state, &snapshot, "late old draft")
                    .unwrap_err()
                    .code,
                "project_changed"
            );
        }
        assert_eq!(
            std::fs::read_to_string(a.path().join("README.md")).unwrap(),
            "A draft"
        );
        assert_eq!(
            std::fs::read_to_string(b.path().join("README.md")).unwrap(),
            "B saved"
        );
    }

    #[test]
    fn close_and_swap_cancel_validated_native_focus_before_removing_project() {
        for close in [true, false] {
            let root = std::env::current_dir().unwrap().canonicalize().unwrap();
            let db = hexagon_core::db::Db::open_in_memory().unwrap();
            db.conn().execute("INSERT INTO projects(id,dir,name,mode,autonomy) VALUES(?1,?2,'preview','pack','L0')",
                [PROJECT_ID, root.to_str().unwrap()]).unwrap();
            let state = AppState {
                wb: Mutex::new(None),
                conn: Mutex::new(Some(ControlConn {
                    db,
                    root: root.clone(),
                    generation: 1,
                })),
            };
            // I1 scoped host migration: this existing native fixture obtains
            // its preview through the current project token before swapping.
            let scope = ProjectState {
                state: &state,
                identity: Some(ProjectIdentity {
                    project_root: root.display().to_string(),
                    generation: 1,
                }),
            };
            let ticket = with_project_conn(&scope, |db, path| {
                use hexagon_core::trace::EventKind;
                db.append_event(PROJECT_ID, EventKind::System, serde_json::json!({"kind":"computer_control","action":"enable","enabled":true,"actor":"owner","screen_to_selected_model":true}),None,None).unwrap();
                db.append_event(PROJECT_ID, EventKind::ToolCalled, serde_json::json!({"tool":"computer_observe","action_id":7}),None,None).unwrap();
                db.append_event(PROJECT_ID, EventKind::ToolResult, serde_json::json!({"action_id":7,"ok":true,"result":{"output":{"ok":true,"result":{"snapshot_id":"observed","window_id":12,"process_id":42}}}}),None,None).unwrap();
                hexagon_core::api::desktop_preview_target(db,path,&root.display().to_string()).map_err(CmdError::internal)
            }).unwrap().unwrap();
            let replacement = if close {
                None
            } else {
                Some(ControlConn {
                    db: hexagon_core::db::Db::open_in_memory().unwrap(),
                    root,
                    generation: 2,
                })
            };
            replace_control(&state, replacement).unwrap();
            assert_eq!(state.conn.lock().unwrap().is_none(), close);
            // No native library is loaded: this must fail on the cancelled
            // generation before any delayed focus can reach the native ABI.
            assert_eq!(
                hexagon_core::api::desktop_preview_focus(&ticket).unwrap_err(),
                "preview stopped"
            );
        }
    }
}

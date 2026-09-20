//! Tauri 壳：command 只做转发，业务全在 hexagon-core 的 Workbench。
//! UI 的唯一通道是这些 command + 后续的事件推送 channel，没有旁路。
//!
//! 连接模型（ADR 0052 三通道）：`wb` 锁只守 turn/mutation 组命令；
//! 读组+控制组走 `conn` 里的第二条 Db 连接——回合占着 wb 时
//! UI 读取与干预指令（pause/resume/send_message/裁决）照常落地，
//! WAL + busy_timeout(5s) 兜底单写者争用（db.rs::init）。

use hexagon_core::api::Workbench;
use hexagon_core::PROJECT_ID;
use serde_json::Value;
use std::sync::{Arc, Mutex};
use tauri::{Emitter, Manager};

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
fn open_control(dir: &str) -> Result<ControlConn, String> {
    Ok(ControlConn {
        db: hexagon_core::db::Db::open(std::path::Path::new(dir).join(".hexagon/state.db"))
            .map_err(|e| e.to_string())?,
        root: std::path::PathBuf::from(dir),
    })
}

/// 读/控通道（ADR 0052）：不摸 wb——回合进行中照样读写。
fn with_conn<R>(
    state: &AppState,
    f: impl FnOnce(&hexagon_core::db::Db, &std::path::Path) -> Result<R, String>,
) -> Result<R, String> {
    let g = state.conn.lock().map_err(|e| e.to_string())?;
    let c = g.as_ref().ok_or_else(|| "no project open".to_string())?;
    f(&c.db, &c.root)
}

fn with_wb<R>(
    state: &AppState,
    f: impl FnOnce(&Workbench) -> Result<R, hexagon_core::api::ApiError>,
) -> Result<R, String> {
    let g = state.wb.lock().map_err(|e| e.to_string())?;
    let wb = g.as_ref().ok_or_else(|| "no project open".to_string())?;
    f(wb).map_err(|e| e.to_string())
}

fn with_wb_mut<R>(
    state: &AppState,
    f: impl FnOnce(&mut Workbench) -> Result<R, hexagon_core::api::ApiError>,
) -> Result<R, String> {
    let mut g = state.wb.lock().map_err(|e| e.to_string())?;
    let wb = g.as_mut().ok_or_else(|| "no project open".to_string())?;
    f(wb).map_err(|e| e.to_string())
}

#[tauri::command]
fn core_ping() -> String {
    hexagon_core::ping()
}

#[tauri::command]
fn open_project(
    app: tauri::AppHandle,
    state: tauri::State<AppState>,
    dir: String,
    name: String,
    roles: Vec<(String, String)>,
    pack_json: Option<String>,
) -> Result<(), String> {
    let pack = pack_json
        .map(|s| serde_json::from_str(&s))
        .transpose()
        .map_err(|e: serde_json::Error| e.to_string())?;
    // open_with（票 05）：open → 凭据库 → 供应商注册，三处 open 序列同一入口
    let wb = Workbench::open_with(
        &dir,
        &name,
        &roles,
        pack,
        Arc::new(hexagon_core::credentials::OsKeychain),
    )
    .map_err(|e| e.to_string())?;
    attach_delta_hook(&app, &wb);
    *state.conn.lock().map_err(|e| e.to_string())? = Some(open_control(&dir)?);
    *state.wb.lock().map_err(|e| e.to_string())? = Some(wb);
    remember_recent(&app, &dir, &name, "pack");
    Ok(())
}

#[tauri::command]
fn timeline(
    state: tauri::State<AppState>,
    after: Option<i64>,
    limit: usize,
) -> Result<Value, String> {
    with_conn(&state, |db, _| {
        db.timeline(PROJECT_ID, after, limit, None)
            .map(|v| serde_json::to_value(v).unwrap())
            .map_err(|e| e.to_string())
    })
}

#[tauri::command]
fn send_message(state: tauri::State<AppState>, body: String) -> Result<i64, String> {
    // 控制通道（ADR 0052）：路由表归 commands::send_via_control——消息落库 +
    // 暂停/恢复 就地生效；其余指令（rewind/stamp/skip/override/install）
    // 返回上来排 wb 队列。两段分开拿锁：conn→wb 不嵌套。
    let (id, cmd) = with_conn(&state, |db, _| {
        hexagon_core::commands::send_via_control(db, PROJECT_ID, &body).map_err(|e| e.to_string())
    })?;
    if let Some(other) = cmd {
        with_wb(&state, |wb| wb.dispatch_command(&other))?;
    }
    Ok(id)
}

#[tauri::command]
fn answer_permission(
    state: tauri::State<AppState>,
    question_id: String,
    allow: bool,
    remember_shape: Option<String>,
    scope: String,
) -> Result<(), String> {
    with_wb(&state, |wb| {
        wb.answer_permission(&question_id, allow, remember_shape.as_deref(), &scope)
    })
}

#[tauri::command]
fn advance(state: tauri::State<AppState>) -> Result<Value, String> {
    with_wb(&state, |wb| wb.advance())
}
#[tauri::command]
fn run_checks(state: tauri::State<AppState>) -> Result<Value, String> {
    with_wb(&state, |wb| wb.run_checks())
}
#[tauri::command]
fn stamp(state: tauri::State<AppState>) -> Result<Value, String> {
    with_wb(&state, |wb| wb.stamp())
}
#[tauri::command]
fn rewind(state: tauri::State<AppState>, to_seq: usize) -> Result<Value, String> {
    with_wb(&state, |wb| wb.rewind(to_seq))
}
#[tauri::command]
fn skip(state: tauri::State<AppState>) -> Result<Value, String> {
    with_wb(&state, |wb| wb.skip())
}
#[tauri::command]
fn skip_review(state: tauri::State<AppState>, artifact_kind: String) -> Result<(), String> {
    with_wb(&state, |wb| wb.skip_review(&artifact_kind))
}
#[tauri::command]
fn pause(state: tauri::State<AppState>) -> Result<(), String> {
    // 暂停按钮是回合中的叫停通道——必须走控制连接，wb 锁正被回合占着
    with_conn(&state, |db, _| {
        hexagon_core::orchestra::pause(db, PROJECT_ID).map_err(|e| e.to_string())
    })
}
#[tauri::command]
fn resume(state: tauri::State<AppState>) -> Result<(), String> {
    with_conn(&state, |db, _| {
        hexagon_core::orchestra::resume(db, PROJECT_ID).map_err(|e| e.to_string())
    })
}
#[tauri::command]
fn sleep_all(state: tauri::State<AppState>) -> Result<(), String> {
    // 全员休眠是干预指令（ADR 0052 控制组）：回合中途也要能落
    with_conn(&state, |db, _| {
        hexagon_core::orchestra::sleep_all(db, PROJECT_ID).map_err(|e| e.to_string())
    })
}

#[tauri::command]
fn artifacts(state: tauri::State<AppState>) -> Result<Vec<Value>, String> {
    with_conn(&state, |db, _| {
        hexagon_core::artifacts::query(db, PROJECT_ID, None, None, None, None)
            .map_err(|e| e.to_string())
    })
}
#[tauri::command]
fn artifact_content(state: tauri::State<AppState>, path: String) -> Result<String, String> {
    with_conn(&state, |db, root| {
        hexagon_core::artifacts::content(db, root, PROJECT_ID, &path).map_err(|e| e.to_string())
    })
}
#[tauri::command]
fn team(state: tauri::State<AppState>) -> Result<Vec<Value>, String> {
    with_conn(&state, |db, _| {
        hexagon_core::orchestra::team(db, PROJECT_ID).map_err(|e| e.to_string())
    })
}
#[tauri::command]
fn stage_status(state: tauri::State<AppState>) -> Result<Vec<Value>, String> {
    with_conn(&state, |db, _| {
        hexagon_core::orchestra::stage_status(db, PROJECT_ID).map_err(|e| e.to_string())
    })
}
#[tauri::command]
fn pending_questions(state: tauri::State<AppState>) -> Result<Vec<Value>, String> {
    with_conn(&state, |db, _| {
        db.queued_questions(PROJECT_ID).map_err(|e| e.to_string())
    })
}
#[tauri::command]
fn usage(state: tauri::State<AppState>) -> Result<Vec<Value>, String> {
    with_conn(&state, |db, _| {
        hexagon_core::usage::project_summary(db, PROJECT_ID).map_err(|e| e.to_string())
    })
}

#[tauri::command]
fn open_stage(state: tauri::State<AppState>, seq: usize) -> Result<Value, String> {
    with_wb(&state, |wb| wb.open_stage(seq))
}

#[tauri::command]
fn recover_run(state: tauri::State<AppState>, run_id: String) -> Result<(), String> {
    with_wb(&state, |wb| wb.recover_run(&run_id))
}

#[tauri::command]
fn override_checks(state: tauri::State<AppState>, reason: String) -> Result<Value, String> {
    with_wb(&state, |wb| wb.override_checks(&reason))
}

#[tauri::command]
fn request_install(state: tauri::State<AppState>, desc: String) -> Result<String, String> {
    // 安装请求只是入队一张待决卡（控制组）；执行在 resolve_install（wb 组）
    with_conn(&state, |db, root| {
        hexagon_core::install::request_install(db, PROJECT_ID, root, &desc)
            .map_err(|e| e.to_string())
    })
}

#[tauri::command]
fn agent_detail(state: tauri::State<AppState>, agent_id: String) -> Result<Value, String> {
    with_conn(&state, |db, _| {
        hexagon_core::roles::agent_detail(db, PROJECT_ID, &agent_id).map_err(|e| e.to_string())
    })
}

#[tauri::command]
fn update_agent(
    state: tauri::State<AppState>,
    agent_id: String,
    patch: Value,
) -> Result<(), String> {
    let patch: hexagon_core::roles::AgentPatch =
        serde_json::from_value(patch).map_err(|e| e.to_string())?;
    // 纯 DB 变更——控制通道直落（票 05：不占 wb 锁）
    with_conn(&state, |db, _| {
        hexagon_core::roles::update_agent_def(db, PROJECT_ID, &agent_id, &patch)
            .map_err(|e| e.to_string())
    })
}

#[tauri::command]
fn create_role(state: tauri::State<AppState>, def: Value) -> Result<String, String> {
    let def: hexagon_core::presets::RoleDef =
        serde_json::from_value(def).map_err(|e| e.to_string())?;
    with_conn(&state, |db, _| {
        hexagon_core::roles::create_role(db, PROJECT_ID, &def).map_err(|e| e.to_string())
    })
}

#[tauri::command]
fn set_agent_grants(
    state: tauri::State<AppState>,
    agent_id: String,
    kind: String,
    names: Vec<String>,
) -> Result<(), String> {
    with_conn(&state, |db, _| {
        hexagon_core::roles::set_grants(db, &agent_id, &kind, &names).map_err(|e| e.to_string())
    })
}

#[tauri::command]
fn draft_role_def(
    state: tauri::State<AppState>,
    agent_id: String,
    hint: String,
) -> Result<String, String> {
    with_wb(&state, |wb| wb.draft_role_def(&agent_id, &hint))
}

#[tauri::command]
fn pack_draft(state: tauri::State<AppState>) -> Result<Value, String> {
    with_conn(&state, |_, root| {
        hexagon_core::packedit::load_draft(root)
            .and_then(|p| serde_json::to_value(&p).map_err(|e| e.into()))
            .map_err(|e| e.to_string())
    })
}

#[tauri::command]
fn save_pack_draft(state: tauri::State<AppState>, pack_json: String) -> Result<(), String> {
    // 草稿写 .hexagon/pack.json（非 active 副本）——控制通道可落
    with_conn(&state, |db, root| {
        let pack = hexagon_core::packedit::parse_draft(&pack_json).map_err(|e| e.to_string())?;
        hexagon_core::packedit::save_draft(db, root, PROJECT_ID, &pack).map_err(|e| e.to_string())
    })
}

/// 个人模板存 ~/.config/hexagon/templates/——项目无关，不需要任何项目态。
#[tauri::command]
fn save_pack_template(pack_json: String) -> Result<String, String> {
    let pack = hexagon_core::packedit::parse_draft(&pack_json).map_err(|e| e.to_string())?;
    hexagon_core::packedit::save_template(&pack)
        .map(|p| p.to_string_lossy().to_string())
        .map_err(|e| e.to_string())
}

/// 个人模板列表：同上，项目无关。
#[tauri::command]
fn pack_templates() -> Result<Vec<String>, String> {
    hexagon_core::packedit::list_templates().map_err(|e| e.to_string())
}

#[tauri::command]
fn export_pack_yaml(state: tauri::State<AppState>, dest: String) -> Result<(), String> {
    with_conn(&state, |_, root| {
        hexagon_core::packedit::export_yaml(root, std::path::Path::new(&dest))
            .map_err(|e| e.to_string())
    })
}

#[tauri::command]
fn resolve_install(
    state: tauri::State<AppState>,
    qid: String,
    allow: bool,
) -> Result<Value, String> {
    // 放行/驳回 + 落盘执行全在 Db+repo_root——控制通道直落（票 05）
    with_conn(&state, |db, root| {
        hexagon_core::install::resolve_install(db, PROJECT_ID, root, &qid, allow)
            .map_err(|e| e.to_string())
    })
}

#[tauri::command]
fn export_events(
    state: tauri::State<AppState>,
    path: String,
    stage_run_id: Option<String>,
    agent_id: Option<String>,
    kinds: Option<Vec<String>>,
) -> Result<usize, String> {
    let kinds = kinds
        .map(|ks| {
            ks.iter()
                .map(|k| {
                    serde_json::from_value::<hexagon_core::trace::EventKind>(serde_json::json!(k))
                        .map_err(|e| e.to_string())
                })
                .collect::<Result<Vec<_>, String>>()
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
        .map_err(|e| e.to_string())
    })
}

#[tauri::command]
fn autonomy(state: tauri::State<AppState>) -> Result<String, String> {
    with_conn(&state, |db, _| {
        hexagon_core::autonomy::level(db, PROJECT_ID).map_err(|e| e.to_string())
    })
}

#[tauri::command]
fn set_autonomy(state: tauri::State<AppState>, level: String) -> Result<(), String> {
    // 档位是 projects 行旋钮（控制组）：回合途中改档即时落库，下回合生效
    with_conn(&state, |db, _| {
        hexagon_core::autonomy::set_level(db, PROJECT_ID, &level).map_err(|e| e.to_string())
    })
}

#[tauri::command]
fn owner_away(state: tauri::State<AppState>) -> Result<(), String> {
    with_conn(&state, |db, _| {
        hexagon_core::autonomy::leave(db, PROJECT_ID)
            .map(|_| ())
            .map_err(|e| e.to_string())
    })
}

#[tauri::command]
fn owner_back(state: tauri::State<AppState>) -> Result<Value, String> {
    with_conn(&state, |db, _| {
        hexagon_core::autonomy::back(db, PROJECT_ID).map_err(|e| e.to_string())
    })
}

#[tauri::command]
fn reject_stamp(state: tauri::State<AppState>) -> Result<Value, String> {
    with_wb(&state, |wb| wb.reject_stamp())
}

/// ADR 0052 wb 组：context_overflow 放行时内联续跑一回合——必须持 wb 锁。
/// 异步化与否见 report.md 待验证假设，本票不碰语义。
#[tauri::command]
fn adjudicate_flag(
    state: tauri::State<AppState>,
    qid: String,
    agree: bool,
) -> Result<Value, String> {
    with_wb(&state, |wb| wb.adjudicate_flag(&qid, agree))
}

#[tauri::command]
fn proposals(state: tauri::State<AppState>) -> Result<Vec<Value>, String> {
    with_conn(&state, |db, _| {
        hexagon_core::proposals::list(db, PROJECT_ID).map_err(|e| e.to_string())
    })
}

#[tauri::command]
fn review_proposal(
    state: tauri::State<AppState>,
    proposal_id: String,
    pass: bool,
    reason: String,
    reviewer_agent: String,
) -> Result<(), String> {
    // 裁决类写（ADR 0052 控制组）：ctx 与 Workbench::ctx_for(reviewer_agent) 同配方
    with_conn(&state, |db, root| {
        let ctx = hexagon_core::tools::ToolContext::for_agent(db, root, &reviewer_agent);
        hexagon_core::proposals::review(db, &ctx, &proposal_id, pass, &reason)
            .map_err(|e| e.to_string())
    })
    .map(|_| ())
}

#[tauri::command]
fn confirm_proposal(state: tauri::State<AppState>, qid: String) -> Result<String, String> {
    with_conn(&state, |db, root| {
        let ctx = hexagon_core::tools::ToolContext::owner(db, root);
        hexagon_core::proposals::activate(db, &ctx, &qid).map_err(|e| e.to_string())
    })
}

#[tauri::command]
fn reject_proposal(
    state: tauri::State<AppState>,
    qid: String,
    reason: String,
) -> Result<(), String> {
    with_conn(&state, |db, root| {
        let ctx = hexagon_core::tools::ToolContext::owner(db, root);
        hexagon_core::proposals::reject_at_stamp(db, &ctx, &qid, &reason).map_err(|e| e.to_string())
    })
}

/// 票 07：不变量伴随件手动体检入口——返回违规数（0=trace 自洽），
/// 违规明细落 invariant_violation 事件。
/// ADR 0052 wb 组：回合中段跑会把在途回合误报成悬挂回合——要静止态。
#[tauri::command]
fn invariant_check(state: tauri::State<AppState>) -> Result<usize, String> {
    // 独立路径重校验 trace——纯 Db 读+违规事件写，走控制通道（票 05 移出 wb 组）
    with_conn(&state, |db, _| {
        hexagon_core::invariant::check_and_log(db, PROJECT_ID).map_err(|e| e.to_string())
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
) -> Result<String, String> {
    let sc: hexagon_core::scenario::Scenario =
        serde_json::from_value(scenario).map_err(|e| e.to_string())?;
    with_wb(&state, |wb| wb.policydev_propose(&edits, &sc, &motive))
}

#[tauri::command]
fn rollback_proposal(state: tauri::State<AppState>, proposal_id: String) -> Result<(), String> {
    with_conn(&state, |db, root| {
        let ctx = hexagon_core::tools::ToolContext::owner(db, root);
        hexagon_core::proposals::rollback(db, &ctx, &proposal_id).map_err(|e| e.to_string())
    })
}

#[tauri::command]
fn request_publish(state: tauri::State<AppState>, remote: String) -> Result<String, String> {
    // 发布请求只是入队待决卡（控制组）；push 在 confirm_publish（wb 组，要 creds）
    with_conn(&state, |db, _| {
        hexagon_core::publish::request(db, PROJECT_ID, &remote).map_err(|e| e.to_string())
    })
}

#[tauri::command]
fn confirm_publish(state: tauri::State<AppState>, qid: String) -> Result<Value, String> {
    with_wb(&state, |wb| wb.confirm_publish(&qid))
}

#[tauri::command]
fn reject_publish(state: tauri::State<AppState>, qid: String) -> Result<(), String> {
    with_conn(&state, |db, _| {
        hexagon_core::publish::reject(db, PROJECT_ID, &qid).map_err(|e| e.to_string())
    })
}

#[tauri::command]
fn set_agent_avatar(
    state: tauri::State<AppState>,
    agent_id: String,
    data_url: String,
) -> Result<(), String> {
    // 纯文件写（avatars/ 目录）——控制通道取 root 即可
    with_conn(&state, |_, root| {
        hexagon_core::roles::set_agent_avatar(root, &agent_id, &data_url).map_err(|e| e.to_string())
    })
}

#[tauri::command]
fn agent_avatar(state: tauri::State<AppState>, agent_id: String) -> Result<Option<String>, String> {
    with_conn(&state, |_, root| {
        hexagon_core::roles::agent_avatar(root, &agent_id).map_err(|e| e.to_string())
    })
}

#[tauri::command]
fn artifact_content_at(
    state: tauri::State<AppState>,
    path: String,
    version: i64,
) -> Result<Option<String>, String> {
    with_conn(&state, |db, root| {
        hexagon_core::artifacts::content_at(db, root, PROJECT_ID, &path, version)
            .map_err(|e| e.to_string())
    })
}

#[tauri::command]
fn set_agent_sleeping(
    state: tauri::State<AppState>,
    agent_id: String,
    sleeping: bool,
) -> Result<(), String> {
    // 休眠/唤醒是干预指令（ADR 0052 控制组）
    with_conn(&state, |db, _| {
        hexagon_core::orchestra::set_agent_sleeping(db, PROJECT_ID, &agent_id, sleeping)
            .map_err(|e| e.to_string())
    })
}

#[tauri::command]
fn set_usage_limit(state: tauri::State<AppState>, limit_cents: Option<i64>) -> Result<(), String> {
    with_conn(&state, |db, _| {
        hexagon_core::usage::set_limit(db, PROJECT_ID, limit_cents).map_err(|e| e.to_string())
    })
}

#[tauri::command]
fn usage_series(
    state: tauri::State<AppState>,
    granularity: String,
    from: Option<String>,
    to: Option<String>,
) -> Result<Value, String> {
    with_conn(&state, |db, _| {
        hexagon_core::usage::series(db, PROJECT_ID, &granularity, from.as_deref(), to.as_deref())
            .map(|v| serde_json::to_value(v).unwrap())
            .map_err(|e| e.to_string())
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

fn read_recents(app: &tauri::AppHandle) -> Vec<Value> {
    recents_path(app)
        .and_then(|p| std::fs::read_to_string(p).ok())
        .and_then(|t| serde_json::from_str::<Vec<Value>>(&t).ok())
        .unwrap_or_default()
}

fn remember_recent(app: &tauri::AppHandle, dir: &str, name: &str, mode: &str) {
    let mut rs = read_recents(app);
    rs.retain(|r| r["dir"] != dir);
    rs.insert(
        0,
        serde_json::json!({
            "dir": dir, "name": name, "mode": mode,
            "opened_at": std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_secs())
                .unwrap_or(0),
        }),
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
fn recent_projects(app: tauri::AppHandle) -> Vec<Value> {
    read_recents(&app)
        .into_iter()
        .filter(|r| r["dir"].as_str().map(|d| std::path::Path::new(d).is_dir()) == Some(true))
        .collect()
}

/// 重新打开已有项目：钉住的包副本恢复 pack，fastpath 项目无副本即 None。
#[tauri::command]
fn open_recent(
    app: tauri::AppHandle,
    state: tauri::State<AppState>,
    dir: String,
) -> Result<(), String> {
    let d = std::path::Path::new(&dir);
    if !d.join(".hexagon/state.db").exists() {
        return Err(format!("不是 Hexagon 项目目录: {dir}"));
    }
    let pack = hexagon_core::orchestra::PackDef::pinned(d).ok();
    let name = d
        .file_name()
        .map(|s| s.to_string_lossy().to_string())
        .unwrap_or_else(|| dir.clone());
    // OR IGNORE 保住库里的真实 name/mode；project_info 读库得真值
    let wb = Workbench::open_with(
        &dir,
        &name,
        &[],
        pack,
        Arc::new(hexagon_core::credentials::OsKeychain),
    )
    .map_err(|e| e.to_string())?;
    attach_delta_hook(&app, &wb);
    *state.conn.lock().map_err(|e| e.to_string())? = Some(open_control(&dir)?);
    let info =
        hexagon_core::orchestra::project_info(&wb.db, PROJECT_ID).map_err(|e| e.to_string())?;
    *state.wb.lock().map_err(|e| e.to_string())? = Some(wb);
    remember_recent(
        &app,
        &dir,
        info["name"].as_str().unwrap_or(&name),
        info["mode"].as_str().unwrap_or("pack"),
    );
    Ok(())
}

/// 关闭当前项目回启动页（不删任何数据）。
#[tauri::command]
fn close_project(state: tauri::State<AppState>) -> Result<(), String> {
    *state.wb.lock().map_err(|e| e.to_string())? = None;
    *state.conn.lock().map_err(|e| e.to_string())? = None;
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
fn set_log_enabled(app: tauri::AppHandle, enabled: bool) -> Result<(), String> {
    let level = if enabled {
        log::LevelFilter::Debug
    } else {
        log::LevelFilter::Warn
    };
    log::set_max_level(level);
    let p = log_enabled_path(&app).ok_or("no config dir")?;
    if let Some(parent) = p.parent() {
        std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
    }
    std::fs::write(&p, serde_json::json!({"log_enabled": enabled}).to_string())
        .map_err(|e| e.to_string())?;
    log::info!("log level set to {level}");
    Ok(())
}

#[tauri::command]
fn log_enabled(app: tauri::AppHandle) -> bool {
    load_log_enabled(&app)
}

// ---------- 项目向导（票 24）：开项目前的检查/选择/建项目，不需要 wb ----------

#[tauri::command]
fn inspect_dir(dir: String) -> Value {
    serde_json::to_value(hexagon_core::setup::inspect_dir(&dir)).unwrap()
}

#[tauri::command]
fn preset_roles() -> Result<Value, String> {
    hexagon_core::presets::preset_roles()
        .map(|v| serde_json::to_value(v).unwrap())
        .map_err(|e| e.to_string())
}

#[tauri::command]
fn preset_packs() -> Result<Value, String> {
    hexagon_core::presets::preset_packs()
        .map(|v| serde_json::to_value(v).unwrap())
        .map_err(|e| e.to_string())
}

#[tauri::command]
fn check_model_keys(slots: Vec<String>) -> Result<Vec<String>, String> {
    use hexagon_core::credentials::{model_key_name, CredentialStore, OsKeychain};
    let mut missing = Vec::new();
    for s in slots {
        let name = model_key_name(&s);
        if OsKeychain.get(&name).map_err(|e| e.to_string())?.is_none() {
            missing.push(name);
        }
    }
    Ok(missing)
}

#[tauri::command]
fn set_model_key(slot: String, secret: String) -> Result<(), String> {
    use hexagon_core::credentials::{model_key_name, CredentialStore, OsKeychain};
    OsKeychain
        .set(&model_key_name(&slot), &secret)
        .map_err(|e| e.to_string())
}

// ---------- 供应商配置（设置页模型区 / 启动页共用）----------

/// 供应商文档：非密字段 + 各供应商 key 是否已存（key 明文永不回传）+ 槽位绑定表。
/// 实现归 provider_admin（票 05）——壳层只转发。
#[tauri::command]
fn list_providers() -> Result<Value, String> {
    hexagon_core::provider_admin::list(&hexagon_core::credentials::OsKeychain)
        .map_err(|e| e.to_string())
}

/// 刷新运行中 Workbench 的供应商注册（保存/删除/绑定变更后热生效）。
fn refresh_providers(state: &AppState) {
    let mut g = match state.wb.lock() {
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
    provider: Value,
    secret: Option<String>,
) -> Result<(), String> {
    hexagon_core::provider_admin::save(provider, secret, &hexagon_core::credentials::OsKeychain)
        .map_err(|e| e.to_string())?;
    refresh_providers(&state);
    Ok(())
}

/// 删供应商（级联解绑槽位）+ 热刷新；keychain 不动。
#[tauri::command]
fn delete_provider(state: tauri::State<AppState>, id: String) -> Result<(), String> {
    hexagon_core::provider_admin::delete(&id).map_err(|e| e.to_string())?;
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
) -> Result<(), String> {
    hexagon_core::provider_admin::set_binding(&slot, &provider_id, &model)
        .map_err(|e| e.to_string())?;
    refresh_providers(&state);
    Ok(())
}

/// 解绑槽位 + 热刷新。
#[tauri::command]
fn remove_slot_binding(state: tauri::State<AppState>, slot: String) -> Result<(), String> {
    hexagon_core::provider_admin::remove_binding(&slot).map_err(|e| e.to_string())?;
    refresh_providers(&state);
    Ok(())
}

/// 拉取/检测供应商模型目录：GET /models；key 从 keychain 现取，缺 key 直报。
/// 返回 ModelEntry 表（id + 分组 + 推断能力），UI 合并进供应商配置。
#[tauri::command]
fn fetch_provider_models(id: String) -> Result<Value, String> {
    hexagon_core::provider_admin::fetch_models(&id, &hexagon_core::credentials::OsKeychain)
        .map_err(|e| e.to_string())
}

#[tauri::command]
fn agents_md_draft(name: String) -> String {
    hexagon_core::setup::agents_md_draft(&name)
}

#[derive(serde::Deserialize)]
#[serde(rename_all = "camelCase")]
struct CreateProjectOpts {
    dir: String,
    name: String,
    roles: Vec<String>,
    pack_name: Option<String>,
    fastpath_role: Option<String>,
    init_git: bool,
    agents_md: Option<String>,
}

#[tauri::command]
fn create_project(
    app: tauri::AppHandle,
    state: tauri::State<AppState>,
    opts: CreateProjectOpts,
) -> Result<(), String> {
    use hexagon_core::credentials::OsKeychain;
    use hexagon_core::setup;
    // 负责人已确认的说明文件先落盘（已存在会被 write_agents_md 拒绝，不覆盖）
    if let Some(md) = &opts.agents_md {
        setup::write_agents_md(&opts.dir, md).map_err(|e| e.to_string())?;
    }
    let pack = opts
        .pack_name
        .as_deref()
        .map(|n| {
            hexagon_core::presets::preset_packs()
                .map_err(|e| e.to_string())?
                .into_iter()
                .find(|p| p.name == n)
                .ok_or_else(|| format!("未知流程包: {n}"))
        })
        .transpose()?;
    // 壳层唯一保留的 providers:: 直调：create_project 建档校验要文档现状
    let pdoc = hexagon_core::providers::load().unwrap_or_default();
    let mut wb = setup::create_project(
        &opts.dir,
        &opts.name,
        &opts.roles,
        pack.as_ref(),
        opts.fastpath_role.as_deref(),
        opts.init_git,
        &OsKeychain,
        &pdoc,
    )
    .map_err(|e| e.to_string())?;
    // 接线尾步归 Workbench：凭据库 + 按文档注册运行槽位（票 05）
    wb.attach_providers(Arc::new(OsKeychain));
    attach_delta_hook(&app, &wb);
    *state.conn.lock().map_err(|e| e.to_string())? = Some(open_control(&opts.dir)?);
    *state.wb.lock().map_err(|e| e.to_string())? = Some(wb);
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
    state.wb.lock().map(|g| g.is_some()).unwrap_or(false)
}

// ---------- 快速通道（票 26）----------

#[tauri::command]
fn project_info(state: tauri::State<AppState>) -> Result<Value, String> {
    with_conn(&state, |db, _| {
        hexagon_core::orchestra::project_info(db, PROJECT_ID).map_err(|e| e.to_string())
    })
}

#[tauri::command]
fn dispatch(state: tauri::State<AppState>, role: String, input: String) -> Result<Value, String> {
    with_wb(&state, |wb| {
        wb.dispatch(&role, &input)
            .map(|o| serde_json::to_value(o).unwrap())
    })
}

#[tauri::command]
fn upgrade_to_pack(state: tauri::State<AppState>, pack_name: String) -> Result<(), String> {
    let pack = hexagon_core::presets::preset_packs()
        .map_err(|e| e.to_string())?
        .into_iter()
        .find(|p| p.name == pack_name)
        .ok_or_else(|| format!("未知流程包: {pack_name}"))?;
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
            open_project,
            timeline,
            send_message,
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
            export_pack_yaml,
            request_install,
            resolve_install,
            export_events,
            autonomy,
            set_autonomy,
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
            preset_packs,
            check_model_keys,
            set_model_key,
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

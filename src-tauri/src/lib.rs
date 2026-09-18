//! Tauri 壳：command 只做转发，业务全在 hexagon-core 的 Workbench。
//! UI 的唯一通道是这些 command + 后续的事件推送 channel，没有旁路。

use hexagon_core::api::Workbench;
use serde_json::Value;
use std::sync::Mutex;
use tauri::Manager;

struct AppState {
    wb: Mutex<Option<Workbench>>,
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
    let wb = Workbench::open(&dir, &name, &roles, pack).map_err(|e| e.to_string())?;
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
    with_wb(&state, |wb| {
        wb.timeline(after, limit)
            .map(|v| serde_json::to_value(v).unwrap())
    })
}

#[tauri::command]
fn send_message(state: tauri::State<AppState>, body: String) -> Result<i64, String> {
    with_wb(&state, |wb| wb.send_message(&body))
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
    with_wb(&state, |wb| wb.pause())
}
#[tauri::command]
fn resume(state: tauri::State<AppState>) -> Result<(), String> {
    with_wb(&state, |wb| wb.resume())
}
#[tauri::command]
fn sleep_all(state: tauri::State<AppState>) -> Result<(), String> {
    with_wb(&state, |wb| wb.sleep_all())
}

#[tauri::command]
fn artifacts(state: tauri::State<AppState>) -> Result<Vec<Value>, String> {
    with_wb(&state, |wb| wb.artifacts())
}
#[tauri::command]
fn artifact_content(state: tauri::State<AppState>, path: String) -> Result<String, String> {
    with_wb(&state, |wb| wb.artifact_content(&path))
}
#[tauri::command]
fn team(state: tauri::State<AppState>) -> Result<Vec<Value>, String> {
    with_wb(&state, |wb| wb.team())
}
#[tauri::command]
fn stage_status(state: tauri::State<AppState>) -> Result<Vec<Value>, String> {
    with_wb(&state, |wb| wb.stage_status())
}
#[tauri::command]
fn pending_questions(state: tauri::State<AppState>) -> Result<Vec<Value>, String> {
    with_wb(&state, |wb| wb.pending_questions())
}
#[tauri::command]
fn usage(state: tauri::State<AppState>) -> Result<Vec<Value>, String> {
    with_wb(&state, |wb| wb.usage())
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
fn autonomy(state: tauri::State<AppState>) -> Result<String, String> {
    with_wb(&state, |wb| wb.autonomy())
}

#[tauri::command]
fn set_autonomy(state: tauri::State<AppState>, level: String) -> Result<(), String> {
    with_wb(&state, |wb| wb.set_autonomy(&level))
}

#[tauri::command]
fn owner_away(state: tauri::State<AppState>) -> Result<(), String> {
    with_wb(&state, |wb| wb.owner_away())
}

#[tauri::command]
fn owner_back(state: tauri::State<AppState>) -> Result<Value, String> {
    with_wb(&state, |wb| wb.owner_back())
}

#[tauri::command]
fn reject_stamp(state: tauri::State<AppState>) -> Result<Value, String> {
    with_wb(&state, |wb| wb.reject_stamp())
}

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
    with_wb(&state, |wb| wb.proposals())
}

#[tauri::command]
fn review_proposal(
    state: tauri::State<AppState>,
    proposal_id: String,
    pass: bool,
    reason: String,
    reviewer_agent: String,
) -> Result<(), String> {
    with_wb(&state, |wb| {
        wb.review_proposal(&proposal_id, pass, &reason, &reviewer_agent)
    })
}

#[tauri::command]
fn confirm_proposal(state: tauri::State<AppState>, qid: String) -> Result<String, String> {
    with_wb(&state, |wb| wb.confirm_proposal(&qid))
}

#[tauri::command]
fn reject_proposal(
    state: tauri::State<AppState>,
    qid: String,
    reason: String,
) -> Result<(), String> {
    with_wb(&state, |wb| wb.reject_proposal(&qid, &reason))
}

#[tauri::command]
fn rollback_proposal(state: tauri::State<AppState>, proposal_id: String) -> Result<(), String> {
    with_wb(&state, |wb| wb.rollback_proposal(&proposal_id))
}

#[tauri::command]
fn request_publish(state: tauri::State<AppState>, remote: String) -> Result<String, String> {
    with_wb(&state, |wb| wb.request_publish(&remote))
}

#[tauri::command]
fn confirm_publish(state: tauri::State<AppState>, qid: String) -> Result<Value, String> {
    with_wb(&state, |wb| wb.confirm_publish(&qid))
}

#[tauri::command]
fn reject_publish(state: tauri::State<AppState>, qid: String) -> Result<(), String> {
    with_wb(&state, |wb| wb.reject_publish(&qid))
}

#[tauri::command]
fn set_agent_avatar(
    state: tauri::State<AppState>,
    agent_id: String,
    data_url: String,
) -> Result<(), String> {
    with_wb(&state, |wb| wb.set_agent_avatar(&agent_id, &data_url))
}

#[tauri::command]
fn agent_avatar(state: tauri::State<AppState>, agent_id: String) -> Result<Option<String>, String> {
    with_wb(&state, |wb| wb.agent_avatar(&agent_id))
}

#[tauri::command]
fn artifact_content_at(
    state: tauri::State<AppState>,
    path: String,
    version: i64,
) -> Result<Option<String>, String> {
    with_wb(&state, |wb| wb.artifact_content_at(&path, version))
}

#[tauri::command]
fn set_agent_sleeping(
    state: tauri::State<AppState>,
    agent_id: String,
    sleeping: bool,
) -> Result<(), String> {
    with_wb(&state, |wb| wb.set_agent_sleeping(&agent_id, sleeping))
}

#[tauri::command]
fn set_usage_limit(state: tauri::State<AppState>, limit_cents: Option<i64>) -> Result<(), String> {
    with_wb(&state, |wb| wb.set_usage_limit(limit_cents))
}

#[tauri::command]
fn usage_series(
    state: tauri::State<AppState>,
    granularity: String,
    from: Option<String>,
    to: Option<String>,
) -> Result<Value, String> {
    with_wb(&state, |wb| {
        wb.usage_series(&granularity, from.as_deref(), to.as_deref())
            .map(|v| serde_json::to_value(v).unwrap())
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
    let wb = Workbench::open(&dir, &name, &[], pack).map_err(|e| e.to_string())?;
    let info = wb.project_info().map_err(|e| e.to_string())?;
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
    let wb = setup::create_project(
        &opts.dir,
        &opts.name,
        &opts.roles,
        pack.as_ref(),
        opts.fastpath_role.as_deref(),
        opts.init_git,
        &OsKeychain,
    )
    .map_err(|e| e.to_string())?;
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
    with_wb(&state, |wb| wb.project_info())
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

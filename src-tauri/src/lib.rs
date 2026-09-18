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

#[tauri::command]
fn core_ping() -> String {
    hexagon_core::ping()
}

#[tauri::command]
fn open_project(
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

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
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
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}

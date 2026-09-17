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
            set_log_enabled,
            log_enabled,
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}

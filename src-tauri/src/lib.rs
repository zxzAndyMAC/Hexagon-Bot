//! Tauri 壳：command 只做转发，业务全在 hexagon-core 的 Workbench。
//! UI 的唯一通道是这些 command + 后续的事件推送 channel，没有旁路。

use hexagon_core::api::Workbench;
use serde_json::Value;
use std::sync::Mutex;

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

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
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
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}

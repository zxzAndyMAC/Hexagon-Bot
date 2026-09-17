//! Tauri 壳：command 只做转发，业务全在 hexagon-core。

#[tauri::command]
fn core_ping() -> String {
    hexagon_core::ping()
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .invoke_handler(tauri::generate_handler![core_ping])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}

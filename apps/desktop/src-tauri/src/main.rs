#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

#[tauri::command]
fn audio_shim_version() -> String {
    pg_capture::shim_version()
}

fn main() {
    tauri::Builder::default()
        .invoke_handler(tauri::generate_handler![audio_shim_version])
        .run(tauri::generate_context!())
        .expect("failed to run the postgame app");
}

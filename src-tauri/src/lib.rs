mod commands;

use commands::*;

pub fn run() {
    tracing_subscriber::fmt::init();

    tauri::Builder::default()
        .plugin(tauri_plugin_shell::init())
        .plugin(tauri_plugin_notification::init())
        .invoke_handler(tauri::generate_handler![
            list_projects,
            create_project,
            detect_runtime,
            delete_project,
        ])
        .run(tauri::generate_context!())
        .expect("error while running Runway");
}

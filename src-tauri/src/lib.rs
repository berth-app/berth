mod commands;

use commands::*;
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::Mutex;

pub fn run() {
    tracing_subscriber::fmt::init();

    let process_registry: ProcessRegistry = Arc::new(Mutex::new(HashMap::new()));

    tauri::Builder::default()
        .plugin(tauri_plugin_shell::init())
        .plugin(tauri_plugin_notification::init())
        .manage(process_registry)
        .invoke_handler(tauri::generate_handler![
            list_projects,
            create_project,
            detect_runtime,
            delete_project,
            save_paste_code,
            run_project,
            stop_project,
        ])
        .run(tauri::generate_context!())
        .expect("error while running Runway");
}

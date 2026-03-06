mod commands;

use commands::*;
use std::collections::HashMap;
use std::sync::Arc;
use tauri::image::Image;
use tauri::tray::TrayIconBuilder;
use tauri::menu::{MenuBuilder, MenuItemBuilder};
use tauri::Manager;
use tokio::sync::Mutex;

pub fn run() {
    tracing_subscriber::fmt::init();

    let process_registry: ProcessRegistry = Arc::new(Mutex::new(HashMap::new()));

    tauri::Builder::default()
        .plugin(tauri_plugin_shell::init())
        .plugin(tauri_plugin_notification::init())
        .manage(process_registry)
        .setup(|app| {
            let show = MenuItemBuilder::with_id("show", "Show Runway").build(app)?;
            let quit = MenuItemBuilder::with_id("quit", "Quit Runway").build(app)?;
            let menu = MenuBuilder::new(app)
                .item(&show)
                .separator()
                .item(&quit)
                .build()?;

            let icon = Image::from_bytes(include_bytes!("../icons/tray-icon.png"))?;

            let tray = TrayIconBuilder::new()
                .icon(icon)
                .icon_as_template(true)
                .tooltip("Runway")
                .menu(&menu)
                .on_menu_event(|app, event| {
                    match event.id().as_ref() {
                        "show" => {
                            if let Some(window) = app.get_webview_window("main") {
                                let _ = window.show();
                                let _ = window.set_focus();
                            }
                        }
                        "quit" => {
                            app.exit(0);
                        }
                        _ => {}
                    }
                })
                .build(app)?;

            // Keep tray alive for the app's lifetime — it is removed when dropped
            app.manage(tray);

            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            list_projects,
            create_project,
            detect_runtime,
            delete_project,
            save_paste_code,
            run_project,
            stop_project,
            list_targets,
            add_target,
            remove_target,
            ping_target,
            run_project_remote,
            stop_project_remote,
        ])
        .run(tauri::generate_context!())
        .expect("error while running Runway");
}

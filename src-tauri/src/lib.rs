mod commands;

use commands::*;
use tauri::image::Image;
use tauri::tray::TrayIconBuilder;
use tauri::menu::{MenuBuilder, MenuItemBuilder};
use tauri::{Emitter, Manager};

fn rebuild_tray_menu(app: &tauri::AppHandle) {
    let projects = match commands::get_store().and_then(|s| s.list().map_err(|e| e.to_string())) {
        Ok(p) => p,
        Err(_) => vec![],
    };

    let build = || -> Result<(), Box<dyn std::error::Error>> {
        let mut builder = MenuBuilder::new(app);

        builder = builder.item(&MenuItemBuilder::with_id("new_project", "New Project").build(app)?);
        builder = builder.separator();

        // Show up to 5 projects, running ones first
        let mut sorted = projects;
        sorted.sort_by(|a, b| {
            let a_run = a.status == runway_core::project::ProjectStatus::Running;
            let b_run = b.status == runway_core::project::ProjectStatus::Running;
            b_run.cmp(&a_run).then(b.updated_at.cmp(&a.updated_at))
        });

        for project in sorted.iter().take(5) {
            let label = if project.status == runway_core::project::ProjectStatus::Running {
                format!("\u{25CF} {} (Running)", project.name)
            } else {
                project.name.clone()
            };
            let id = format!("project:{}", project.id);
            builder = builder.item(&MenuItemBuilder::with_id(&id, &label).build(app)?);
        }

        builder = builder.separator();
        builder = builder.item(&MenuItemBuilder::with_id("settings", "Settings\u{2026}").build(app)?);
        builder = builder.item(&MenuItemBuilder::with_id("show", "Show Runway").build(app)?);
        builder = builder.separator();
        builder = builder.item(&MenuItemBuilder::with_id("quit", "Quit Runway").build(app)?);

        let menu = builder.build()?;
        let tray = app.state::<tauri::tray::TrayIcon>();
        tray.set_menu(Some(menu))?;

        Ok(())
    };

    if let Err(e) = build() {
        tracing::error!("Failed to rebuild tray menu: {e}");
    }
}

/// Background scheduler loop — checks for due schedules every 30 seconds.
/// Runs on a dedicated thread since ProjectStore (rusqlite) isn't Send.
fn start_scheduler(app_handle: tauri::AppHandle) {
    std::thread::spawn(move || {
        let rt = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("scheduler runtime");

        rt.block_on(async move {
            // Wait for the local agent to be ready
            tokio::time::sleep(tokio::time::Duration::from_secs(5)).await;

            loop {
                if let Ok(store) = commands::get_store() {
                    let results = runway_core::scheduler::tick(&store).await;
                    for (project_id, result) in &results {
                        let (status, exit_code) = match result {
                            Ok(code) => {
                                let s = if *code == 0 { "idle" } else { "failed" };
                                (s, Some(*code))
                            }
                            Err(_) => ("failed", Some(-1)),
                        };

                        let _ = app_handle.emit(
                            "project-status-change",
                            commands::StatusEvent {
                                project_id: project_id.to_string(),
                                status: status.into(),
                                exit_code,
                            },
                        );

                        let _ = app_handle.emit(
                            "schedule-executed",
                            serde_json::json!({
                                "project_id": project_id.to_string(),
                                "success": result.is_ok() && result.as_ref().unwrap() == &0,
                                "exit_code": exit_code,
                            }),
                        );
                    }

                    if !results.is_empty() {
                        rebuild_tray_menu(&app_handle);
                    }
                }

                tokio::time::sleep(tokio::time::Duration::from_secs(30)).await;
            }
        });
    });
}

fn show_window_and_navigate(app: &tauri::AppHandle, payload: &str) {
    if let Some(window) = app.get_webview_window("main") {
        let _ = window.show();
        let _ = window.set_focus();
    }
    let _ = app.emit("navigate", payload);
}

pub fn run() {
    tracing_subscriber::fmt::init();

    tauri::Builder::default()
        .plugin(tauri_plugin_shell::init())
        .plugin(tauri_plugin_notification::init())
        .setup(|app| {
            // Start the embedded local agent on a background task
            tauri::async_runtime::spawn(async {
                if let Err(e) = runway_core::local_agent::get_or_start_local_agent().await {
                    tracing::error!("Failed to start local agent: {}", e);
                }
            });

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
                    let id = event.id().as_ref();
                    match id {
                        "show" => {
                            if let Some(window) = app.get_webview_window("main") {
                                let _ = window.show();
                                let _ = window.set_focus();
                            }
                        }
                        "quit" => {
                            runway_core::local_agent::cleanup_lockfile();
                            app.exit(0);
                        }
                        "new_project" => show_window_and_navigate(app, "paste"),
                        "settings" => show_window_and_navigate(app, "settings"),
                        _ if id.starts_with("project:") => {
                            let project_id = &id["project:".len()..];
                            if let Some(window) = app.get_webview_window("main") {
                                let _ = window.show();
                                let _ = window.set_focus();
                            }
                            let _ = app.emit("navigate-project", project_id);
                        }
                        _ => {}
                    }
                })
                .build(app)?;

            // Keep tray alive for the app's lifetime — it is removed when dropped
            app.manage(tray);

            // Build initial tray menu with projects
            rebuild_tray_menu(&app.handle());

            // Start background scheduler loop
            start_scheduler(app.handle().clone());

            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            list_projects,
            create_project,
            detect_runtime,
            update_project,
            delete_project,
            save_paste_code,
            run_project,
            stop_project,
            list_targets,
            add_target,
            remove_target,
            ping_target,
            get_agent_stats,
            list_schedules,
            add_schedule,
            remove_schedule,
            toggle_schedule,
            get_settings,
            update_setting,
            import_file,
            list_execution_logs,
            set_project_notify,
            set_project_target,
            read_project_file,
            write_project_file,
        ])
        .run(tauri::generate_context!())
        .expect("error while running Runway");
}

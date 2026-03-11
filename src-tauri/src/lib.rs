mod commands;

use commands::*;
use tauri::image::Image;
use tauri::tray::TrayIconBuilder;
use tauri::menu::{MenuBuilder, MenuItemBuilder};
use tauri::{Emitter, Manager};

use berth_core::nats_relay::NatsConfig;
use berth_core::nats_subscriber::NatsSubscriber;

/// Send a macOS notification via osascript.
/// Works in both dev and release mode without bundle ID restrictions.
pub(crate) fn notify_macos(title: &str, body: &str) {
    let title = title.replace('\\', "\\\\").replace('"', "\\\"");
    let body = body.replace('\\', "\\\\").replace('"', "\\\"");
    let script = format!(r#"display notification "{body}" with title "{title}""#);
    std::thread::spawn(move || {
        let _ = std::process::Command::new("osascript")
            .arg("-e")
            .arg(&script)
            .output();
    });
}

/// Send a macOS notification for a completed run if the project has notify_on_complete enabled.
fn notify_if_enabled(project_id: &str, exit_code: Option<i32>) {
    if let Ok(store) = commands::get_store() {
        if let Ok(projects) = store.list() {
            if let Some(project) = projects.iter().find(|p| p.id.to_string() == project_id) {
                if project.notify_on_complete {
                    let (title, body) = if exit_code == Some(0) {
                        ("Berth — Run Complete", format!("{} finished successfully", project.name))
                    } else {
                        ("Berth — Run Failed", format!("{} failed (exit code {:?})", project.name, exit_code))
                    };
                    notify_macos(title, &body);
                }
            }
        }
    }
}

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
            let a_run = a.status == berth_core::project::ProjectStatus::Running;
            let b_run = b.status == berth_core::project::ProjectStatus::Running;
            b_run.cmp(&a_run).then(b.updated_at.cmp(&a.updated_at))
        });

        for project in sorted.iter().take(5) {
            let label = if project.status == berth_core::project::ProjectStatus::Running {
                format!("\u{25CF} {} (Running)", project.name)
            } else {
                project.name.clone()
            };
            let id = format!("project:{}", project.id);
            builder = builder.item(&MenuItemBuilder::with_id(&id, &label).build(app)?);
        }

        builder = builder.separator();
        builder = builder.item(&MenuItemBuilder::with_id("settings", "Settings\u{2026}").build(app)?);
        builder = builder.item(&MenuItemBuilder::with_id("show", "Show Berth").build(app)?);
        builder = builder.separator();
        builder = builder.item(&MenuItemBuilder::with_id("quit", "Quit Berth").build(app)?);

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
                    let results = berth_core::scheduler::tick(&store).await;
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

                        notify_if_enabled(&project_id.to_string(), exit_code);
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

/// Start NATS subscriber if configured. Bridges NATS messages to Tauri events.
fn start_nats_subscriber(app_handle: tauri::AppHandle) {
    tauri::async_runtime::spawn(async move {
        // Read NATS config from settings
        let (nats_url, nats_creds) = match commands::get_store() {
            Ok(store) => {
                let settings = store.get_all_settings().unwrap_or_default();
                let url = settings.get("nats_url").cloned();
                let creds = settings.get("nats_creds").cloned();
                (url, creds)
            }
            Err(_) => (None, None),
        };

        let nats_url = match nats_url {
            Some(url) if !url.is_empty() => url,
            _ => return, // NATS not configured
        };

        // Get install_id (generate if first launch)
        let install_id = match commands::get_store() {
            Ok(store) => {
                let settings = store.get_all_settings().unwrap_or_default();
                match settings.get("install_id") {
                    Some(id) => id.clone(),
                    None => {
                        let id = uuid::Uuid::new_v4().to_string();
                        let _ = store.set_setting("install_id", &id);
                        id
                    }
                }
            }
            Err(_) => return,
        };

        let config = NatsConfig {
            url: nats_url,
            creds_path: nats_creds,
            agent_id: String::new(), // not used for subscriber
            owner_id: install_id.clone(),
        };

        let subscriber = match NatsSubscriber::connect(&config, &install_id).await {
            Ok(sub) => sub,
            Err(e) => {
                tracing::warn!("Failed to connect to NATS (running without relay): {e}");
                return;
            }
        };

        tracing::info!("NATS subscriber connected, bridging events to UI");

        // Store the NATS client for command channel use
        commands::set_nats_client(subscriber.client().clone()).await;

        // Get agent IDs from remote targets
        let agent_ids: Vec<String> = match commands::get_store() {
            Ok(store) => store
                .list_targets()
                .unwrap_or_default()
                .into_iter()
                .filter(|t| t.kind == berth_core::target::TargetKind::Remote)
                .filter_map(|t| t.nats_agent_id)
                .collect(),
            Err(_) => vec![],
        };

        if agent_ids.is_empty() {
            tracing::info!("No remote targets with NATS agent IDs configured");
            return;
        }

        // Subscribe to heartbeats (returns a channel, doesn't need subscriber after)
        let hb_rx = subscriber.subscribe_heartbeats(&agent_ids).await;
        if let Ok(mut rx) = hb_rx {
            let hb_app = app_handle.clone();
            tokio::spawn(async move {
                while let Some(hb) = rx.recv().await {
                    let _ = hb_app.emit("agent-heartbeat", serde_json::json!({
                        "agent_id": hb.agent_id,
                        "status": "online",
                        "cpu_usage": hb.cpu_usage,
                        "memory_bytes": hb.memory_bytes,
                        "uptime_seconds": hb.uptime_seconds,
                        "version": hb.version,
                    }));
                }
            });
        }

        // Subscribe to events
        let evt_app = app_handle.clone();
        match subscriber.subscribe_events(&agent_ids).await {
            Ok(mut stream) => {
                use futures::StreamExt;
                tokio::spawn(async move {
                    while let Some(event) = stream.next().await {
                        match event.event_type.as_str() {
                            "execution_completed" | "execution_stopped" => {
                                let data: serde_json::Value = serde_json::from_str(&event.data).unwrap_or_default();
                                let status = data.get("status").and_then(|v| v.as_str()).unwrap_or("idle");
                                let exit_code = data.get("exit_code").and_then(|v| v.as_i64()).map(|v| v as i32);
                                let project_id = event.project_id.unwrap_or_default();
                                let _ = evt_app.emit(
                                    "project-status-change",
                                    commands::StatusEvent {
                                        project_id: project_id.clone(),
                                        status: status.into(),
                                        exit_code,
                                    },
                                );
                                notify_if_enabled(&project_id, exit_code);
                            }
                            "schedule_triggered" => {
                                let data: serde_json::Value = serde_json::from_str(&event.data).unwrap_or_default();
                                let exit_code = data.get("exit_code").and_then(|v| v.as_i64()).map(|v| v as i32);
                                let status = if exit_code == Some(0) { "idle" } else { "failed" };
                                let project_id = event.project_id.clone().unwrap_or_default();

                                let _ = evt_app.emit(
                                    "project-status-change",
                                    commands::StatusEvent {
                                        project_id: project_id.clone(),
                                        status: status.into(),
                                        exit_code,
                                    },
                                );

                                let _ = evt_app.emit("schedule-executed", serde_json::json!({
                                    "project_id": &project_id,
                                    "via": "nats",
                                }));

                                notify_if_enabled(&project_id, exit_code);
                            }
                            "service_restarting" => {
                                let data: serde_json::Value = serde_json::from_str(&event.data).unwrap_or_default();
                                let exit_code = data.get("exit_code").and_then(|v| v.as_i64()).map(|v| v as i32);
                                let restart_count = data.get("restart_count").and_then(|v| v.as_u64()).unwrap_or(0);
                                let delay_ms = data.get("delay_ms").and_then(|v| v.as_u64()).unwrap_or(0);
                                let project_id = event.project_id.unwrap_or_default();

                                let _ = evt_app.emit(
                                    "project-status-change",
                                    commands::StatusEvent {
                                        project_id: project_id.clone(),
                                        status: "restarting".into(),
                                        exit_code,
                                    },
                                );

                                // Notify on crash (service died unexpectedly)
                                if let Ok(store) = commands::get_store() {
                                    if let Ok(projects) = store.list() {
                                        if let Some(project) = projects.iter().find(|p| p.id.to_string() == project_id) {
                                            if project.notify_on_complete {
                                                let body = format!(
                                                    "{} crashed (exit {}). Restarting #{} in {:.1}s",
                                                    project.name,
                                                    exit_code.map(|c| c.to_string()).unwrap_or("?".into()),
                                                    restart_count,
                                                    delay_ms as f64 / 1000.0
                                                );
                                                notify_macos("Berth — Service Restarting", &body);
                                            }
                                        }
                                    }
                                }
                            }
                            "service_restarted" => {
                                let project_id = event.project_id.unwrap_or_default();
                                let _ = evt_app.emit(
                                    "project-status-change",
                                    commands::StatusEvent {
                                        project_id: project_id.clone(),
                                        status: "running".into(),
                                        exit_code: None,
                                    },
                                );
                            }
                            _ => {}
                        }
                    }
                });
            }
            Err(e) => tracing::warn!("Failed to subscribe to NATS events: {e}"),
        }
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
        .plugin(tauri_plugin_deep_link::init())
        .setup(|app| {
            // Start the embedded local agent on a background task
            tauri::async_runtime::spawn(async {
                if let Err(e) = berth_core::local_agent::get_or_start_local_agent().await {
                    tracing::error!("Failed to start local agent: {}", e);
                }
            });

            let show = MenuItemBuilder::with_id("show", "Show Berth").build(app)?;
            let quit = MenuItemBuilder::with_id("quit", "Quit Berth").build(app)?;
            let menu = MenuBuilder::new(app)
                .item(&show)
                .separator()
                .item(&quit)
                .build()?;

            let icon = Image::from_bytes(include_bytes!("../icons/tray-icon.png"))?;

            let tray = TrayIconBuilder::new()
                .icon(icon)
                .icon_as_template(true)
                .tooltip("Berth")
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

            // Ensure install_id exists (used as owner_id for pairing + NATS)
            if let Ok(store) = commands::get_store() {
                let settings = store.get_all_settings().unwrap_or_default();
                if settings.get("install_id").is_none() {
                    let id = uuid::Uuid::new_v4().to_string();
                    let _ = store.set_setting("install_id", &id);
                }
            }

            // Start NATS subscriber (bridges remote agent events to UI)
            start_nats_subscriber(app.handle().clone());

            // Initialize telemetry and track app launch
            commands::init_telemetry();
            commands::track_app_launch();

            // Flush telemetry every 4 hours
            tauri::async_runtime::spawn(async {
                let mut interval = tokio::time::interval(tokio::time::Duration::from_secs(4 * 60 * 60));
                interval.tick().await; // skip first immediate tick
                loop {
                    interval.tick().await;
                    commands::flush_telemetry().await;
                }
            });

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
            pair_agent,
            update_target_nats,
            ping_target,
            get_agent_stats,
            list_schedules,
            add_schedule,
            remove_schedule,
            toggle_schedule,
            get_settings,
            update_setting,
            save_nats_credentials,
            clear_nats_credentials,
            import_file,
            list_execution_logs,
            set_project_notify,
            set_project_target,
            read_project_file,
            write_project_file,
            check_agent_upgrade,
            upgrade_agent,
            rollback_agent,
            upgrade_all_agents,
            set_project_run_mode,
            publish_project,
            unpublish_project,
            get_env_vars,
            set_env_var,
            delete_env_var,
            import_env_file,
            store_list_templates,
            store_search_templates,
            store_install_template,
            get_telemetry_status,
            set_telemetry_enabled,
            get_telemetry_events,
            purge_telemetry,
        ])
        .build(tauri::generate_context!())
        .expect("error while building Berth")
        .run(|_app, event| {
            if let tauri::RunEvent::Exit = event {
                tauri::async_runtime::block_on(commands::flush_telemetry());
                berth_core::local_agent::cleanup_lockfile();
            }
        });
}

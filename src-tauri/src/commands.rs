use std::collections::HashMap;
use std::sync::Arc;

use runway_core::executor::{self, LogStream};
use tauri::Emitter;
use runway_core::project::{Project, ProjectStatus};
use runway_core::runtime::{self, RuntimeInfo};
use runway_core::store::ProjectStore;
use serde::Serialize;
use std::path::Path;
use tokio::process::Child;
use tokio::sync::Mutex;
use uuid::Uuid;

pub struct RunningProcess {
    pub child: Child,
    pub log_task_abort: tokio::task::AbortHandle,
}

pub type ProcessRegistry = Arc<Mutex<HashMap<Uuid, RunningProcess>>>;

#[derive(Clone, Serialize)]
pub struct LogEvent {
    pub project_id: String,
    pub stream: String,
    pub text: String,
    pub timestamp: String,
}

#[derive(Clone, Serialize)]
pub struct StatusEvent {
    pub project_id: String,
    pub status: String,
}

#[derive(Serialize)]
pub struct ProjectResponse {
    pub projects: Vec<Project>,
}

fn get_store() -> Result<ProjectStore, String> {
    let data_dir = dirs_next::data_dir()
        .unwrap_or_else(|| std::path::PathBuf::from("."))
        .join("com.runway.app");
    std::fs::create_dir_all(&data_dir).map_err(|e| e.to_string())?;
    let db_path = data_dir.join("runway.db");
    ProjectStore::open(db_path.to_str().unwrap_or("runway.db")).map_err(|e| e.to_string())
}

#[tauri::command]
pub fn list_projects() -> Result<ProjectResponse, String> {
    let store = get_store()?;
    let projects = store.list().map_err(|e| e.to_string())?;
    Ok(ProjectResponse { projects })
}

#[tauri::command]
pub fn create_project(name: String, path: String) -> Result<Project, String> {
    let store = get_store()?;
    let info = runtime::detect_runtime(Path::new(&path));
    let mut project = Project::new(name, path, info.runtime);
    project.entrypoint = info.entrypoint;
    store.insert(&project).map_err(|e| e.to_string())?;
    Ok(project)
}

#[tauri::command]
pub fn save_paste_code(name: String, code: String) -> Result<String, String> {
    // Normalize smart/curly quotes to straight quotes (macOS auto-substitution)
    let code = code
        .replace('\u{201C}', "\"") // left double curly quote
        .replace('\u{201D}', "\"") // right double curly quote
        .replace('\u{2018}', "'")  // left single curly quote
        .replace('\u{2019}', "'"); // right single curly quote

    let base = dirs_next::data_dir()
        .unwrap_or_else(|| std::path::PathBuf::from("/tmp"))
        .join("com.runway.app")
        .join("projects")
        .join(&name);
    std::fs::create_dir_all(&base).map_err(|e| e.to_string())?;

    // Detect language from code content to pick filename
    let filename = if code.starts_with("#!/usr/bin/env python")
        || code.starts_with("#!/usr/bin/python")
        || code.contains("import ")
        || code.contains("def ")
        || code.contains("print(")
    {
        "main.py"
    } else if code.contains("console.log")
        || code.contains("require(")
        || code.contains("import {")
        || code.contains("export ")
        || code.contains("const ")
        || code.contains("async function")
    {
        "index.js"
    } else if code.contains("package main")
        || code.contains("func main()")
        || code.contains("fmt.Print")
    {
        "main.go"
    } else if code.starts_with("#!/") || code.starts_with("set -") {
        "run.sh"
    } else {
        "main.py"
    };

    let file_path = base.join(filename);
    std::fs::write(&file_path, &code).map_err(|e| e.to_string())?;

    Ok(base.to_string_lossy().to_string())
}

#[tauri::command]
pub fn detect_runtime(path: String) -> RuntimeInfo {
    runtime::detect_runtime(Path::new(&path))
}

#[tauri::command]
pub fn delete_project(id: String) -> Result<(), String> {
    let store = get_store()?;
    let uuid: Uuid = id.parse().map_err(|e: uuid::Error| e.to_string())?;
    store.delete(uuid).map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn run_project(
    id: String,
    registry: tauri::State<'_, ProcessRegistry>,
    app_handle: tauri::AppHandle,
) -> Result<(), String> {
    let uuid: Uuid = id.parse().map_err(|e: uuid::Error| e.to_string())?;
    let store = get_store()?;
    let project = store
        .get(uuid)
        .map_err(|e| e.to_string())?
        .ok_or_else(|| format!("Project {id} not found"))?;

    let entrypoint = project
        .entrypoint
        .as_deref()
        .ok_or("Project has no entrypoint. Use 'Detect' to identify the runtime and entry file.")?
        .to_string();

    // Prevent double-run
    {
        let reg = registry.lock().await;
        if reg.contains_key(&uuid) {
            return Err("Project is already running.".to_string());
        }
    }

    let (child, mut rx) = executor::spawn_and_stream(project.runtime, &entrypoint, &project.path)
        .await
        .map_err(|e| format!("Failed to start process: {e}"))?;

    store
        .update_status(uuid, ProjectStatus::Running)
        .map_err(|e| e.to_string())?;

    let _ = app_handle.emit(
        "project-status-change",
        StatusEvent {
            project_id: id.clone(),
            status: "running".into(),
        },
    );

    let app_for_logs = app_handle.clone();
    let app_for_exit = app_handle;
    let project_id_for_logs = id.clone();
    let registry_clone = registry.inner().clone();

    let log_task = tokio::spawn(async move {
        while let Some(line) = rx.recv().await {
            let _ = app_for_logs.emit(
                "project-log",
                LogEvent {
                    project_id: project_id_for_logs.clone(),
                    stream: match line.stream {
                        LogStream::Stdout => "stdout".into(),
                        LogStream::Stderr => "stderr".into(),
                    },
                    text: line.text,
                    timestamp: line.timestamp.to_rfc3339(),
                },
            );
        }

        // Channel closed — process exited. Clean up.
        let mut reg = registry_clone.lock().await;
        if let Some(mut proc) = reg.remove(&uuid) {
            let exit_status = proc.child.wait().await.ok();
            let status = match exit_status.and_then(|s| s.code()) {
                Some(0) => ProjectStatus::Idle,
                _ => ProjectStatus::Failed,
            };
            if let Ok(store) = get_store() {
                let _ = store.update_status(uuid, status);
            }
            let status_str = match status {
                ProjectStatus::Idle => "idle",
                ProjectStatus::Failed => "failed",
                _ => "idle",
            };
            let _ = app_for_exit.emit(
                "project-status-change",
                StatusEvent {
                    project_id: uuid.to_string(),
                    status: status_str.into(),
                },
            );
        }
    });

    // Store child and abort handle in registry
    {
        let mut reg = registry.lock().await;
        reg.insert(
            uuid,
            RunningProcess {
                child,
                log_task_abort: log_task.abort_handle(),
            },
        );
    }

    Ok(())
}

#[tauri::command]
pub async fn stop_project(
    id: String,
    registry: tauri::State<'_, ProcessRegistry>,
    app_handle: tauri::AppHandle,
) -> Result<(), String> {
    let uuid: Uuid = id.parse().map_err(|e: uuid::Error| e.to_string())?;

    let mut proc = {
        let mut reg = registry.lock().await;
        reg.remove(&uuid).ok_or("Project is not running.")?
    };

    proc.child
        .kill()
        .await
        .map_err(|e| format!("Failed to stop process: {e}"))?;

    proc.log_task_abort.abort();

    let store = get_store()?;
    store
        .update_status(uuid, ProjectStatus::Stopped)
        .map_err(|e| e.to_string())?;

    let _ = app_handle.emit(
        "project-status-change",
        StatusEvent {
            project_id: id,
            status: "stopped".into(),
        },
    );

    Ok(())
}

use std::collections::HashMap;
use std::sync::Arc;

use runway_core::agent_client::AgentClient;
use runway_core::executor::{self, LogStream};
use tauri::Emitter;
use runway_core::project::{Project, ProjectStatus};
use runway_core::runtime::{self, RuntimeInfo};
use runway_core::store::ProjectStore;
use runway_core::target::Target;
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
    pub exit_code: Option<i32>,
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

    // Record run start (updates status, last_run_at, run_count)
    store
        .record_run_start(uuid)
        .map_err(|e| e.to_string())?;

    let _ = app_handle.emit(
        "project-status-change",
        StatusEvent {
            project_id: id.clone(),
            status: "running".into(),
            exit_code: None,
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
            let exit_code = exit_status.and_then(|s| s.code());

            if let Ok(store) = get_store() {
                let _ = store.record_run_end(uuid, exit_code);
            }

            let status_str = match exit_code {
                Some(0) => "idle",
                _ => "failed",
            };
            let _ = app_for_exit.emit(
                "project-status-change",
                StatusEvent {
                    project_id: uuid.to_string(),
                    status: status_str.into(),
                    exit_code,
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
            exit_code: None,
        },
    );

    Ok(())
}

// --- Target commands ---

#[derive(Clone, Serialize)]
pub struct TargetInfo {
    pub id: String,
    pub name: String,
    pub kind: String,
    pub host: Option<String>,
    pub port: u16,
    pub status: String,
    pub agent_version: Option<String>,
    pub last_seen_at: Option<String>,
}

impl From<&Target> for TargetInfo {
    fn from(t: &Target) -> Self {
        Self {
            id: t.id.to_string(),
            name: t.name.clone(),
            kind: format!("{:?}", t.kind).to_lowercase(),
            host: t.host.clone(),
            port: t.port,
            status: format!("{:?}", t.status).to_lowercase(),
            agent_version: t.agent_version.clone(),
            last_seen_at: t.last_seen_at.map(|ts| ts.to_rfc3339()),
        }
    }
}

#[tauri::command]
pub fn list_targets() -> Result<Vec<TargetInfo>, String> {
    let store = get_store()?;
    let targets = store.list_targets().map_err(|e| e.to_string())?;
    Ok(targets.iter().map(TargetInfo::from).collect())
}

#[tauri::command]
pub fn add_target(name: String, host: String, port: u16) -> Result<TargetInfo, String> {
    let store = get_store()?;
    let target = Target::new_remote(name, host, port);
    store.insert_target(&target).map_err(|e| e.to_string())?;
    Ok(TargetInfo::from(&target))
}

#[tauri::command]
pub fn remove_target(id: String) -> Result<(), String> {
    let store = get_store()?;
    let uuid: Uuid = id.parse().map_err(|e: uuid::Error| e.to_string())?;
    store.delete_target(uuid).map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn ping_target(id: String) -> Result<TargetInfo, String> {
    let store = get_store()?;
    let uuid: Uuid = id.parse().map_err(|e: uuid::Error| e.to_string())?;

    let targets = store.list_targets().map_err(|e| e.to_string())?;
    let target = targets
        .iter()
        .find(|t| t.id == uuid)
        .ok_or_else(|| format!("Target {} not found", id))?;

    let endpoint = target.grpc_endpoint();
    match AgentClient::connect(&endpoint).await {
        Ok(mut client) => match client.health().await {
            Ok(health) => {
                let _ = store.update_target_status(
                    target.id,
                    runway_core::target::TargetStatus::Online,
                    Some(&health.version),
                );
                let mut info = TargetInfo::from(target);
                info.status = "online".into();
                info.agent_version = Some(health.version);
                Ok(info)
            }
            Err(e) => {
                let _ = store.update_target_status(
                    target.id,
                    runway_core::target::TargetStatus::Offline,
                    None,
                );
                Err(format!("Agent unhealthy: {}", e))
            }
        },
        Err(e) => {
            let _ = store.update_target_status(
                target.id,
                runway_core::target::TargetStatus::Offline,
                None,
            );
            Err(format!("Connection failed: {}", e))
        }
    }
}

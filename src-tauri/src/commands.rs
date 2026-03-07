use runway_core::agent_client::AgentClient;
use runway_core::agent_transport::AgentTransport;
use runway_core::nats_cmd_client::NatsAgentClient;
use tauri::Emitter;
use tauri_plugin_notification::NotificationExt;
use runway_core::project::{Project, ProjectStatus};
use runway_core::runtime::{self, RuntimeInfo};
use runway_core::scheduler::Schedule;
use runway_core::store::{ExecutionLog, ProjectStore};
use runway_core::target::Target;
use serde::Serialize;
use std::collections::HashMap;
use std::path::Path;
use tokio::sync::Mutex;
use uuid::Uuid;

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

pub(crate) fn get_store() -> Result<ProjectStore, String> {
    let data_dir = dirs_next::data_dir()
        .unwrap_or_else(|| std::path::PathBuf::from("."))
        .join("com.runway.app");
    std::fs::create_dir_all(&data_dir).map_err(|e| e.to_string())?;
    let db_path = data_dir.join("runway.db");
    ProjectStore::open(db_path.to_str().unwrap_or("runway.db")).map_err(|e| e.to_string())
}

/// Get a transport for the given target. None or "local" uses the embedded local agent via gRPC.
/// Remote targets with `nats_enabled` use NATS command channel; otherwise gRPC.
async fn get_agent_client(target_id: Option<&str>) -> Result<Box<dyn AgentTransport>, String> {
    match target_id {
        None | Some("local") | Some("") => {
            let client = runway_core::local_agent::get_or_start_local_agent()
                .await
                .map_err(|e| format!("Failed to start local agent: {e}"))?;
            Ok(Box::new(client))
        }
        Some(tid) => {
            let store = get_store()?;
            let uuid: Uuid = tid.parse().map_err(|e: uuid::Error| e.to_string())?;
            let targets = store.list_targets().map_err(|e| e.to_string())?;
            let target = targets
                .into_iter()
                .find(|t| t.id == uuid)
                .ok_or_else(|| format!("Target {tid} not found"))?;

            // Use NATS transport if enabled and agent_id is set
            if target.nats_enabled {
                if let Some(ref agent_id) = target.nats_agent_id {
                    let nats_client = get_nats_client().await?;
                    return Ok(Box::new(NatsAgentClient::new(nats_client, agent_id.clone())));
                }
            }

            // Fall back to gRPC
            let endpoint = target.grpc_endpoint();
            let client = AgentClient::connect(&endpoint)
                .await
                .map_err(|e| format!("Failed to connect to agent: {e}"))?;
            Ok(Box::new(client))
        }
    }
}

/// Get the shared NATS client, or connect on demand from settings.
async fn get_nats_client() -> Result<async_nats::Client, String> {
    // Try reading from global state (set by start_nats_subscriber in lib.rs)
    if let Some(client) = nats_client_lock().lock().await.clone() {
        return Ok(client);
    }

    // If no client yet, connect from settings
    let store = get_store()?;
    let settings = store.get_all_settings().unwrap_or_default();
    let nats_url = settings
        .get("nats_url")
        .cloned()
        .ok_or("NATS not configured. Set nats_url in Settings.")?;
    let nats_creds = settings.get("nats_creds").cloned();

    let mut opts = async_nats::ConnectOptions::new();
    if let Some(ref creds_path) = nats_creds {
        opts = opts
            .credentials_file(creds_path)
            .await
            .map_err(|e| format!("Failed to load NATS credentials: {e}"))?;
    }
    let client = opts
        .connect(&nats_url)
        .await
        .map_err(|e| format!("Failed to connect to NATS: {e}"))?;

    // Cache for next time
    *nats_client_lock().lock().await = Some(client.clone());

    Ok(client)
}

static NATS_CLIENT: std::sync::OnceLock<Mutex<Option<async_nats::Client>>> =
    std::sync::OnceLock::new();

fn nats_client_lock() -> &'static Mutex<Option<async_nats::Client>> {
    NATS_CLIENT.get_or_init(|| Mutex::new(None))
}

/// Set the NATS client (called from lib.rs when subscriber connects).
pub async fn set_nats_client(client: async_nats::Client) {
    *nats_client_lock().lock().await = Some(client);
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
pub fn update_project(id: String, name: String, entrypoint: Option<String>) -> Result<(), String> {
    let store = get_store()?;
    let uuid: Uuid = id.parse().map_err(|e: uuid::Error| e.to_string())?;
    store
        .update_project(uuid, &name, entrypoint.as_deref())
        .map_err(|e| e.to_string())
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
    target: Option<String>,
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

    let runtime_str = format!("{:?}", project.runtime).to_lowercase();
    let is_remote = matches!(target.as_deref(), Some(t) if !t.is_empty() && t != "local");

    // For remote targets, read the code from disk to send over gRPC
    let code = if is_remote {
        let code_path = std::path::Path::new(&project.path).join(&entrypoint);
        Some(
            std::fs::read(&code_path)
                .map_err(|e| format!("Failed to read {}: {e}", code_path.display()))?,
        )
    } else {
        None
    };

    let working_dir = if is_remote {
        "/tmp".to_string()
    } else {
        project.path.clone()
    };

    let client = get_agent_client(target.as_deref()).await?;

    // Create execution log entry
    let exec_log = ExecutionLog::new(uuid, "manual");
    let exec_log_id = exec_log.id;
    store
        .insert_execution_log(&exec_log)
        .map_err(|e| e.to_string())?;

    // Record run start
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

    let project_id_str = id.clone();
    let project_name = project.name.clone();
    let notify_on_complete = project.notify_on_complete;

    // Spawn background task for streaming logs
    tokio::spawn(async move {
        use runway_core::agent_transport::ExecuteParams;

        let params = ExecuteParams {
            project_id: project_id_str.clone(),
            runtime: runtime_str,
            entrypoint,
            working_dir,
            code,
            image_tag: None,
            env_vars: std::collections::HashMap::new(),
        };

        let stream_result = client.execute_streaming(&params).await;

        match stream_result {
            Ok(mut rx) => {
                let mut exit_code = 0i32;
                let mut collected_output = String::new();

                while let Some(msg) = rx.recv().await {
                    if msg.is_final {
                        exit_code = msg.exit_code;
                        continue;
                    }

                    // Collect output for execution log (cap at 64KB)
                    if collected_output.len() < 65536 {
                        collected_output.push_str(&msg.text);
                    }

                    let _ = app_handle.emit(
                        "project-log",
                        LogEvent {
                            project_id: project_id_str.clone(),
                            stream: msg.stream,
                            text: msg.text,
                            timestamp: msg.timestamp,
                        },
                    );
                }

                if let Ok(store) = get_store() {
                    let _ = store.record_run_end(uuid, Some(exit_code));
                    let _ = store.finish_execution_log(exec_log_id, exit_code, &collected_output);
                }

                let status = if exit_code == 0 { "idle" } else { "failed" };
                let _ = app_handle.emit(
                    "project-status-change",
                    StatusEvent {
                        project_id: project_id_str,
                        status: status.into(),
                        exit_code: Some(exit_code),
                    },
                );

                if notify_on_complete {
                    let (title, body) = if exit_code == 0 {
                        ("Runway — Run Complete".to_string(), format!("{project_name} finished successfully"))
                    } else {
                        ("Runway — Run Failed".to_string(), format!("{project_name} exited with code {exit_code}"))
                    };
                    let _ = app_handle.notification().builder().title(&title).body(&body).show();
                }
            }
            Err(e) => {
                let error_msg = format!("Execution failed: {e}");

                let _ = app_handle.emit(
                    "project-log",
                    LogEvent {
                        project_id: project_id_str.clone(),
                        stream: "stderr".into(),
                        text: error_msg.clone(),
                        timestamp: chrono::Utc::now().to_rfc3339(),
                    },
                );

                if let Ok(store) = get_store() {
                    let _ = store.record_run_end(uuid, Some(1));
                    let _ = store.finish_execution_log(exec_log_id, 1, &error_msg);
                }

                let _ = app_handle.emit(
                    "project-status-change",
                    StatusEvent {
                        project_id: project_id_str,
                        status: "failed".into(),
                        exit_code: Some(1),
                    },
                );

                if notify_on_complete {
                    let _ = app_handle.notification()
                        .builder()
                        .title("Runway — Run Failed")
                        .body(&format!("{project_name}: {error_msg}"))
                        .show();
                }
            }
        }
    });

    Ok(())
}

#[tauri::command]
pub async fn stop_project(
    id: String,
    target: Option<String>,
    app_handle: tauri::AppHandle,
) -> Result<(), String> {
    let uuid: Uuid = id.parse().map_err(|e: uuid::Error| e.to_string())?;

    let client = get_agent_client(target.as_deref()).await?;

    let stopped = client
        .stop(&id)
        .await
        .map_err(|e| format!("Failed to stop project: {e}"))?;

    if stopped {
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
    } else {
        Err("Project is not running.".into())
    }
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
    pub nats_agent_id: Option<String>,
    pub nats_enabled: bool,
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
            nats_agent_id: t.nats_agent_id.clone(),
            nats_enabled: t.nats_enabled,
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
pub fn add_target(name: String, host: String, port: u16, nats_agent_id: Option<String>) -> Result<TargetInfo, String> {
    let store = get_store()?;
    let mut target = Target::new_remote(name, host, port);
    if let Some(ref agent_id) = nats_agent_id {
        if !agent_id.is_empty() {
            target.nats_agent_id = Some(agent_id.clone());
            target.nats_enabled = true;
        }
    }
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
pub fn update_target_nats(id: String, nats_agent_id: String, nats_enabled: bool) -> Result<(), String> {
    let store = get_store()?;
    let uuid: Uuid = id.parse().map_err(|e: uuid::Error| e.to_string())?;
    let agent_id = if nats_agent_id.is_empty() { None } else { Some(nats_agent_id.as_str()) };
    store.update_target_nats(uuid, agent_id, nats_enabled).map_err(|e| e.to_string())
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

    let transport = get_agent_client(Some(&id)).await;
    match transport {
        Ok(client) => match client.health().await {
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

// --- Agent stats command ---

#[derive(Clone, Serialize)]
pub struct AgentStats {
    pub agent_id: String,
    pub version: String,
    pub status: String,
    pub uptime_seconds: u64,
    pub cpu_usage: f64,
    pub memory_mb: u64,
    pub podman_version: Option<String>,
    pub container_ready: bool,
    pub running_projects: Vec<AgentRunningProject>,
    pub os: Option<String>,
    pub arch: Option<String>,
}

#[derive(Clone, Serialize)]
pub struct AgentRunningProject {
    pub project_id: String,
    pub status: String,
    pub started_at: String,
}

#[tauri::command]
pub async fn get_agent_stats(id: String) -> Result<AgentStats, String> {
    let store = get_store()?;
    let uuid: Uuid = id.parse().map_err(|e: uuid::Error| e.to_string())?;

    let targets = store.list_targets().map_err(|e| e.to_string())?;
    let target = targets
        .iter()
        .find(|t| t.id == uuid)
        .ok_or_else(|| format!("Target {} not found", id))?;

    let client = get_agent_client(Some(&id)).await?;

    let health = client.health().await.map_err(|e| format!("Health RPC failed: {e}"))?;
    let status = client.status().await.map_err(|e| format!("Status RPC failed: {e}"))?;

    // Update target status while we're at it
    let _ = store.update_target_status(
        target.id,
        runway_core::target::TargetStatus::Online,
        Some(&health.version),
    );

    Ok(AgentStats {
        agent_id: status.agent_id,
        version: health.version,
        status: status.status,
        uptime_seconds: health.uptime_seconds,
        cpu_usage: status.cpu_usage,
        memory_mb: status.memory_bytes / 1024 / 1024,
        podman_version: health.podman_version,
        container_ready: health.container_ready,
        running_projects: status
            .running_projects
            .into_iter()
            .map(|p| AgentRunningProject {
                project_id: p.project_id,
                status: p.status,
                started_at: p.started_at,
            })
            .collect(),
        os: health.os,
        arch: health.arch,
    })
}

// --- Schedule commands ---

#[derive(Clone, Serialize)]
pub struct ScheduleInfo {
    pub id: String,
    pub project_id: String,
    pub cron_expr: String,
    pub enabled: bool,
    pub created_at: String,
    pub last_triggered_at: Option<String>,
    pub next_run_at: Option<String>,
}

impl From<&Schedule> for ScheduleInfo {
    fn from(s: &Schedule) -> Self {
        Self {
            id: s.id.to_string(),
            project_id: s.project_id.to_string(),
            cron_expr: s.cron_expr.clone(),
            enabled: s.enabled,
            created_at: s.created_at.to_rfc3339(),
            last_triggered_at: s.last_triggered_at.map(|t| t.to_rfc3339()),
            next_run_at: s.next_run_at.map(|t| t.to_rfc3339()),
        }
    }
}

#[tauri::command]
pub fn list_schedules(project_id: String) -> Result<Vec<ScheduleInfo>, String> {
    let store = get_store()?;
    let uuid: Uuid = project_id.parse().map_err(|e: uuid::Error| e.to_string())?;
    let schedules = store
        .get_schedules_for_project(uuid)
        .map_err(|e| e.to_string())?;
    Ok(schedules.iter().map(ScheduleInfo::from).collect())
}

#[tauri::command]
pub fn add_schedule(project_id: String, cron_expr: String) -> Result<ScheduleInfo, String> {
    let store = get_store()?;
    let uuid: Uuid = project_id.parse().map_err(|e: uuid::Error| e.to_string())?;
    let schedule = Schedule::new(uuid, cron_expr);
    store
        .insert_schedule(&schedule)
        .map_err(|e| e.to_string())?;
    Ok(ScheduleInfo::from(&schedule))
}

#[tauri::command]
pub fn remove_schedule(id: String) -> Result<(), String> {
    let store = get_store()?;
    let uuid: Uuid = id.parse().map_err(|e: uuid::Error| e.to_string())?;
    store.delete_schedule(uuid).map_err(|e| e.to_string())
}

#[tauri::command]
pub fn toggle_schedule(id: String, enabled: bool) -> Result<(), String> {
    let store = get_store()?;
    let uuid: Uuid = id.parse().map_err(|e: uuid::Error| e.to_string())?;
    store
        .set_schedule_enabled(uuid, enabled)
        .map_err(|e| e.to_string())
}

// --- Execution log commands ---

#[derive(Clone, Serialize)]
pub struct ExecutionLogInfo {
    pub id: String,
    pub project_id: String,
    pub started_at: String,
    pub finished_at: Option<String>,
    pub exit_code: Option<i32>,
    pub output: String,
    pub trigger: String,
}

impl From<&ExecutionLog> for ExecutionLogInfo {
    fn from(l: &ExecutionLog) -> Self {
        Self {
            id: l.id.to_string(),
            project_id: l.project_id.to_string(),
            started_at: l.started_at.to_rfc3339(),
            finished_at: l.finished_at.map(|t| t.to_rfc3339()),
            exit_code: l.exit_code,
            output: l.output.clone(),
            trigger: l.trigger.clone(),
        }
    }
}

#[tauri::command]
pub fn list_execution_logs(
    project_id: String,
    limit: Option<u32>,
) -> Result<Vec<ExecutionLogInfo>, String> {
    let store = get_store()?;
    let uuid: Uuid = project_id.parse().map_err(|e: uuid::Error| e.to_string())?;
    let logs = store
        .list_execution_logs(uuid, limit.unwrap_or(20))
        .map_err(|e| e.to_string())?;
    Ok(logs.iter().map(ExecutionLogInfo::from).collect())
}

// --- Settings commands ---

#[tauri::command]
pub fn get_settings() -> Result<HashMap<String, String>, String> {
    let store = get_store()?;
    store.get_all_settings().map_err(|e| e.to_string())
}

#[tauri::command]
pub fn update_setting(key: String, value: String) -> Result<(), String> {
    let store = get_store()?;
    store.set_setting(&key, &value).map_err(|e| e.to_string())
}

// --- Project notification setting ---

#[tauri::command]
pub fn set_project_notify(id: String, enabled: bool) -> Result<(), String> {
    let store = get_store()?;
    let uuid: Uuid = id.parse().map_err(|e: uuid::Error| e.to_string())?;
    store
        .set_project_notify(uuid, enabled)
        .map_err(|e| e.to_string())
}

// --- Project target assignment ---

#[tauri::command]
pub fn set_project_target(id: String, target_id: Option<String>) -> Result<(), String> {
    let store = get_store()?;
    let uuid: Uuid = id.parse().map_err(|e: uuid::Error| e.to_string())?;
    store
        .set_project_target(uuid, target_id.as_deref())
        .map_err(|e| e.to_string())
}

// --- Project file commands ---

#[tauri::command]
pub fn read_project_file(id: String) -> Result<String, String> {
    let store = get_store()?;
    let uuid: Uuid = id.parse().map_err(|e: uuid::Error| e.to_string())?;
    let project = store
        .get(uuid)
        .map_err(|e| e.to_string())?
        .ok_or_else(|| format!("Project {id} not found"))?;

    let entrypoint = project
        .entrypoint
        .as_deref()
        .ok_or("Project has no entrypoint")?;

    let file_path = Path::new(&project.path).join(entrypoint);
    std::fs::read_to_string(&file_path)
        .map_err(|e| format!("Failed to read {}: {e}", file_path.display()))
}

#[tauri::command]
pub fn write_project_file(id: String, content: String) -> Result<(), String> {
    let store = get_store()?;
    let uuid: Uuid = id.parse().map_err(|e: uuid::Error| e.to_string())?;
    let project = store
        .get(uuid)
        .map_err(|e| e.to_string())?
        .ok_or_else(|| format!("Project {id} not found"))?;

    let entrypoint = project
        .entrypoint
        .as_deref()
        .ok_or("Project has no entrypoint")?;

    let file_path = Path::new(&project.path).join(entrypoint);
    std::fs::write(&file_path, content)
        .map_err(|e| format!("Failed to write {}: {e}", file_path.display()))
}

// --- File import command ---

#[tauri::command]
pub fn import_file(file_path: String) -> Result<Project, String> {
    let src = Path::new(&file_path);

    let ext = src
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or("");

    let allowed = ["py", "js", "ts", "go", "sh", "rs"];
    if !allowed.contains(&ext) {
        return Err(format!(
            "Unsupported file type '.{ext}'. Supported: {}",
            allowed.join(", ")
        ));
    }

    let filename = src
        .file_name()
        .and_then(|n| n.to_str())
        .ok_or("Invalid filename")?;

    let project_name = src
        .file_stem()
        .and_then(|n| n.to_str())
        .unwrap_or("imported")
        .to_string();

    let base = dirs_next::data_dir()
        .unwrap_or_else(|| std::path::PathBuf::from("/tmp"))
        .join("com.runway.app")
        .join("projects")
        .join(&project_name);
    std::fs::create_dir_all(&base).map_err(|e| e.to_string())?;

    let dest = base.join(filename);
    std::fs::copy(src, &dest).map_err(|e| format!("Failed to copy file: {e}"))?;

    let info = runtime::detect_runtime(&base);
    let store = get_store()?;
    let mut project = Project::new(project_name, base.to_string_lossy().to_string(), info.runtime);
    project.entrypoint = info.entrypoint;
    store.insert(&project).map_err(|e| e.to_string())?;

    Ok(project)
}

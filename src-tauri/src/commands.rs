use berth_core::agent_transport::AgentTransport;
use berth_core::nats_cmd_client::NatsAgentClient;
use berth_core::path_safety;
use tauri::Emitter;
use berth_core::project::{Project, ProjectStatus};
use berth_core::runtime::{self, RuntimeInfo};
use berth_core::scheduler::Schedule;
use berth_core::store::{ExecutionLog, ProjectStore};
use berth_core::target::Target;
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
        .join("com.berth.app");
    std::fs::create_dir_all(&data_dir).map_err(|e| e.to_string())?;
    let db_path = data_dir.join("berth.db");
    ProjectStore::open(db_path.to_str().unwrap_or("berth.db")).map_err(|e| e.to_string())
}

/// Get a transport for the given target. None or "local" uses the embedded local agent via UDS.
/// Remote targets use NATS (Synadia Cloud) or direct gRPC with mTLS.
async fn get_agent_client(target_id: Option<&str>) -> Result<Box<dyn AgentTransport>, String> {
    match target_id {
        None | Some("local") | Some("") => {
            let client = berth_core::local_agent::get_or_start_local_agent()
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

            let has_nats_id = target.nats_agent_id.as_deref().map_or(false, |id| !id.is_empty());
            let has_host = target.host.as_deref().map_or(false, |h| !h.is_empty()) && target.port > 0;

            // Prefer NATS if agent has a NATS ID and NATS client is available
            if has_nats_id {
                if let Ok(nats_client) = get_nats_client().await {
                    let agent_id = target.nats_agent_id.as_deref().unwrap().to_string();
                    let owner_id = target.owner_id.clone().ok_or_else(|| {
                        "Target has no owner_id — re-pair this agent in target settings".to_string()
                    })?;
                    let mut client = NatsAgentClient::new(nats_client, agent_id, owner_id);

                    // Load shared secret from Keychain for HMAC signing
                    #[cfg(target_os = "macos")]
                    {
                        let key = format!("agent-secret:{}", uuid);
                        if let Ok(Some(hex_secret)) = berth_core::credentials::get_credential(&key) {
                            if let Ok(secret_bytes) = hex::decode(&hex_secret) {
                                client = client.with_secret(secret_bytes);
                            }
                        }
                    }

                    return Ok(Box::new(client));
                }
            }

            // Fall back to direct gRPC with mTLS
            if has_host {
                let settings = store.get_all_settings().unwrap_or_default();
                let host = target.host.as_deref().unwrap();
                let endpoint = format!("https://{}:{}", host, target.port);

                // Check for TLS certificates
                if let (Some(ca_path), Some(cert_path), Some(key_path)) = (
                    settings.get("tls_ca"),
                    settings.get("tls_client_cert"),
                    settings.get("tls_client_key"),
                ) {
                    let ca_pem = std::fs::read_to_string(ca_path)
                        .map_err(|e| format!("Failed to read CA cert: {e}"))?;
                    let cert_pem = std::fs::read_to_string(cert_path)
                        .map_err(|e| format!("Failed to read client cert: {e}"))?;
                    let key_pem = std::fs::read_to_string(key_path)
                        .map_err(|e| format!("Failed to read client key: {e}"))?;

                    let client = berth_core::agent_client::AgentClient::connect_tls(
                        &endpoint, &ca_pem, &cert_pem, &key_pem,
                    )
                    .await
                    .map_err(|e| format!("Direct mTLS connection failed: {e}"))?;

                    return Ok(Box::new(client));
                }

                return Err(format!(
                    "Target has host {}:{} but no TLS certificates configured. \
                     Import the agent's CA and client certificates in Settings, \
                     or use Synadia Cloud for relay-based connections.",
                    host, target.port
                ));
            }

            Err(
                "Target has no connection method configured. Either:\n\
                 - Set up Synadia Cloud (NATS relay) and pair the agent, or\n\
                 - Add a host:port and import mTLS certificates for direct connection."
                    .into(),
            )
        }
    }
}

/// Get the shared NATS client, or connect on demand from settings.
/// NATS URL is user-provided (BYON — Synadia Cloud recommended). No default URL.
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
        .ok_or("NATS not configured. Set your Synadia Cloud nats_url in Settings, or use direct gRPC connection.")?;
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
    // Validate project name to prevent directory traversal
    let name = path_safety::sanitize_project_name(&name)?;

    // Normalize smart/curly quotes to straight quotes (macOS auto-substitution)
    let code = code
        .replace('\u{201C}', "\"") // left double curly quote
        .replace('\u{201D}', "\"") // right double curly quote
        .replace('\u{2018}', "'")  // left single curly quote
        .replace('\u{2019}', "'"); // right single curly quote

    let base = dirs_next::data_dir()
        .unwrap_or_else(|| std::path::PathBuf::from("/tmp"))
        .join("com.berth.app")
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

    // Load env vars from store before entering async block
    let env_vars = store.get_env_vars(uuid).unwrap_or_default();

    // Spawn background task for streaming logs
    tokio::spawn(async move {
        use berth_core::agent_transport::ExecuteParams;

        let run_mode = match project.run_mode {
            berth_core::RunMode::Service => "service".to_string(),
            berth_core::RunMode::Oneshot => "oneshot".to_string(),
        };
        let params = ExecuteParams {
            project_id: project_id_str.clone(),
            runtime: runtime_str,
            entrypoint,
            working_dir,
            code,
            image_tag: None,
            env_vars: env_vars.clone(),
            run_mode,
            service_port: project.service_port.unwrap_or(0),
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

                    // Mask env var values in log output
                    let masked_text = berth_core::env::mask_env_values(&msg.text, &env_vars);

                    // Collect output for execution log (cap at 64KB)
                    if collected_output.len() < 65536 {
                        collected_output.push_str(&masked_text);
                    }

                    let _ = app_handle.emit(
                        "project-log",
                        LogEvent {
                            project_id: project_id_str.clone(),
                            stream: msg.stream,
                            text: masked_text,
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
                        ("Berth — Run Complete".to_string(), format!("{project_name} finished successfully"))
                    } else {
                        ("Berth — Run Failed".to_string(), format!("{project_name} exited with code {exit_code}"))
                    };
                    super::notify_macos(&title, &body);
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
                    super::notify_macos("Berth — Run Failed", &format!("{project_name}: {error_msg}"));
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
        // Agent says process isn't running — reset DB status anyway so UI gets unstuck
        let store = get_store()?;
        store
            .update_status(uuid, ProjectStatus::Idle)
            .map_err(|e| e.to_string())?;

        let _ = app_handle.emit(
            "project-status-change",
            StatusEvent {
                project_id: id,
                status: "idle".into(),
                exit_code: None,
            },
        );

        Ok(())
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
    pub tunnel_providers: Vec<String>,
    pub owner_id: Option<String>,
    pub docker_version: Option<String>,
    pub compose_version: Option<String>,
    pub container_runtime: Option<String>,
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
            tunnel_providers: vec![],
            owner_id: t.owner_id.clone(),
            docker_version: t.docker_version.clone(),
            compose_version: t.compose_version.clone(),
            container_runtime: t.container_runtime.clone(),
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

#[derive(Serialize)]
pub struct PairingResult {
    pub success: bool,
    pub agent_id: String,
    pub agent_hostname: String,
    pub agent_os: String,
    pub agent_version: String,
    pub owner_id: String,
    pub target_id: String,
}

#[tauri::command]
pub async fn pair_agent(code: String) -> Result<PairingResult, String> {
    use berth_core::nats_relay::{
        compute_challenge_response, pairing_advertise_subject, pairing_ack_subject,
        pairing_claim_subject, PairingAdvertisement, PairingAck, PairingClaim,
    };
    use futures::StreamExt;

    let code = code.trim().to_uppercase();
    if code.len() != 8 {
        return Err("Pairing code must be 8 characters".into());
    }

    let nats_client = get_nats_client().await?;
    let store = get_store()?;

    // Get install_id as owner_id
    let settings = store.get_all_settings().unwrap_or_default();
    let owner_id = settings
        .get("install_id")
        .cloned()
        .ok_or("install_id not found — restart the app")?;

    // Subscribe to ack first, then discover agent
    let ack_subject = pairing_ack_subject(&code);
    let mut ack_sub = nats_client
        .subscribe(ack_subject)
        .await
        .map_err(|e| format!("Failed to subscribe to pairing ack: {e}"))?;

    // Subscribe to advertisement to discover agent info
    let adv_subject = pairing_advertise_subject(&code);
    let mut adv_sub = nats_client
        .subscribe(adv_subject)
        .await
        .map_err(|e| format!("Failed to subscribe to pairing advertisements: {e}"))?;

    // Wait up to 15s for an advertisement
    let advertisement: PairingAdvertisement = tokio::time::timeout(
        std::time::Duration::from_secs(15),
        async {
            while let Some(msg) = adv_sub.next().await {
                if let Ok(ad) = serde_json::from_slice::<PairingAdvertisement>(&msg.payload) {
                    return ad;
                }
            }
            unreachable!()
        },
    )
    .await
    .map_err(|_| "No agent found with that code. Check the code and try again.")?;

    // Compute challenge-response to prove we know the pairing code
    let challenge_response = if !advertisement.challenge.is_empty() {
        compute_challenge_response(&advertisement.challenge, &code)
    } else {
        String::new()
    };

    // Send claim with challenge-response
    let claim = PairingClaim {
        owner_id: owner_id.clone(),
        challenge_response,
    };
    let claim_subject = pairing_claim_subject(&code);
    let payload = serde_json::to_vec(&claim).map_err(|e| e.to_string())?;
    nats_client
        .publish(claim_subject, payload.into())
        .await
        .map_err(|e| format!("Failed to send pairing claim: {e}"))?;
    nats_client.flush().await.map_err(|e| e.to_string())?;

    // Wait for ack
    let ack: PairingAck = tokio::time::timeout(
        std::time::Duration::from_secs(10),
        async {
            while let Some(msg) = ack_sub.next().await {
                if let Ok(ack) = serde_json::from_slice::<PairingAck>(&msg.payload) {
                    return ack;
                }
            }
            unreachable!()
        },
    )
    .await
    .map_err(|_| "Agent did not acknowledge pairing. Try again.")?;

    if !ack.success {
        return Err(format!("Pairing rejected: {}", ack.message));
    }

    // Create target
    let mut target = Target::new_remote(
        advertisement.hostname.clone(),
        String::new(), // no direct host needed for NATS
        0,
    );
    target.nats_agent_id = Some(advertisement.agent_id.clone());
    target.nats_enabled = true;
    target.owner_id = Some(owner_id.clone());
    target.agent_version = Some(advertisement.version.clone());
    store.insert_target(&target).map_err(|e| e.to_string())?;

    // Store shared secret in Keychain for HMAC signing
    #[cfg(target_os = "macos")]
    if !ack.shared_secret.is_empty() {
        let key = format!("agent-secret:{}", target.id);
        if let Err(e) = berth_core::credentials::store_credential(&key, &ack.shared_secret) {
            tracing::warn!("Failed to store agent shared secret in Keychain: {e}");
        }
    }

    Ok(PairingResult {
        success: true,
        agent_id: advertisement.agent_id,
        agent_hostname: advertisement.hostname,
        agent_os: format!("{}/{}", advertisement.os, advertisement.arch),
        agent_version: advertisement.version,
        owner_id,
        target_id: target.id.to_string(),
    })
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
                    berth_core::target::TargetStatus::Online,
                    Some(&health.version),
                );
                let _ = store.update_target_container_caps(
                    target.id,
                    health.docker_version.as_deref(),
                    health.compose_version.as_deref(),
                    &health.container_runtime,
                );
                let mut info = TargetInfo::from(target);
                info.status = "online".into();
                info.agent_version = Some(health.version);
                info.tunnel_providers = health.tunnel_providers;
                info.docker_version = health.docker_version;
                info.compose_version = health.compose_version;
                info.container_runtime = Some(health.container_runtime);
                Ok(info)
            }
            Err(e) => {
                let _ = store.update_target_status(
                    target.id,
                    berth_core::target::TargetStatus::Offline,
                    None,
                );
                Err(format!("Agent unhealthy: {}", e))
            }
        },
        Err(e) => {
            let _ = store.update_target_status(
                target.id,
                berth_core::target::TargetStatus::Offline,
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
    pub tunnel_providers: Vec<String>,
    pub docker_version: Option<String>,
    pub compose_version: Option<String>,
    pub container_runtime: String,
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
        berth_core::target::TargetStatus::Online,
        Some(&health.version),
    );

    // Persist container capabilities on the target
    let _ = store.update_target_container_caps(
        target.id,
        health.docker_version.as_deref(),
        health.compose_version.as_deref(),
        &health.container_runtime,
    );

    Ok(AgentStats {
        agent_id: status.agent_id,
        version: health.version.clone(),
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
        tunnel_providers: health.tunnel_providers,
        docker_version: health.docker_version,
        compose_version: health.compose_version,
        container_runtime: health.container_runtime,
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

#[tauri::command]
pub fn save_nats_credentials(credentials: String) -> Result<(), String> {
    // Validate that it looks like a NATS credentials block
    if !credentials.contains("BEGIN NATS USER JWT") || !credentials.contains("BEGIN USER NKEY SEED") {
        return Err(
            "Invalid credentials format. Expected a Synadia Cloud credentials block containing \
             both a NATS USER JWT and USER NKEY SEED section."
                .into(),
        );
    }

    let data_dir = dirs_next::data_dir()
        .unwrap_or_else(|| std::path::PathBuf::from("."))
        .join("com.berth.app");
    std::fs::create_dir_all(&data_dir).map_err(|e| e.to_string())?;
    let creds_path = data_dir.join("nats.creds");

    std::fs::write(&creds_path, credentials.trim()).map_err(|e| e.to_string())?;

    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&creds_path, std::fs::Permissions::from_mode(0o600))
            .map_err(|e| e.to_string())?;
    }

    // Save the path in settings
    let store = get_store()?;
    store
        .set_setting("nats_creds", creds_path.to_str().unwrap_or(""))
        .map_err(|e| e.to_string())?;

    // Auto-set NATS URL if not already configured
    let settings = store.get_all_settings().unwrap_or_default();
    if settings.get("nats_url").map_or(true, |v| v.is_empty()) {
        store
            .set_setting("nats_url", "tls://connect.ngs.global")
            .map_err(|e| e.to_string())?;
    }

    Ok(())
}

#[tauri::command]
pub fn clear_nats_credentials() -> Result<(), String> {
    let store = get_store()?;
    let settings = store.get_all_settings().unwrap_or_default();

    // Delete the credentials file if it exists
    if let Some(creds_path) = settings.get("nats_creds") {
        let path = std::path::Path::new(creds_path);
        if path.exists() {
            std::fs::remove_file(path).map_err(|e| e.to_string())?;
        }
    }

    // Clear the setting
    store.set_setting("nats_creds", "").map_err(|e| e.to_string())?;

    Ok(())
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

// --- Project run mode ---

#[tauri::command]
pub fn set_project_run_mode(id: String, run_mode: String, service_port: Option<u16>) -> Result<(), String> {
    let store = get_store()?;
    let uuid: Uuid = id.parse().map_err(|e: uuid::Error| e.to_string())?;
    let mode = match run_mode.as_str() {
        "service" => berth_core::RunMode::Service,
        _ => berth_core::RunMode::Oneshot,
    };
    store.set_project_run_mode(uuid, mode, service_port).map_err(|e| e.to_string())
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

    // Validate entrypoint doesn't escape project directory
    path_safety::sanitize_entrypoint(entrypoint)?;
    let project_path = Path::new(&project.path);
    let file_path = project_path.join(entrypoint);
    let file_path = path_safety::validate_path_within(project_path, &file_path)?;

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

    // Validate entrypoint doesn't escape project directory
    path_safety::sanitize_entrypoint(entrypoint)?;
    let project_path = Path::new(&project.path);
    let file_path = project_path.join(entrypoint);
    let file_path = path_safety::validate_path_within(project_path, &file_path)?;

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
    // Sanitize filename to prevent path traversal
    let filename = path_safety::sanitize_filename(filename)?;

    let project_name = src
        .file_stem()
        .and_then(|n| n.to_str())
        .unwrap_or("imported")
        .to_string();
    // Sanitize project name to prevent directory traversal
    let project_name = path_safety::sanitize_project_name(&project_name)?;

    let base = dirs_next::data_dir()
        .unwrap_or_else(|| std::path::PathBuf::from("/tmp"))
        .join("com.berth.app")
        .join("projects")
        .join(&project_name);
    std::fs::create_dir_all(&base).map_err(|e| e.to_string())?;

    let dest = base.join(&filename);
    std::fs::copy(src, &dest).map_err(|e| format!("Failed to copy file: {e}"))?;

    let info = runtime::detect_runtime(&base);
    let store = get_store()?;
    let mut project = Project::new(project_name, base.to_string_lossy().to_string(), info.runtime);
    project.entrypoint = info.entrypoint;
    store.insert(&project).map_err(|e| e.to_string())?;

    Ok(project)
}

// --- Agent upgrade commands ---

const AGENT_GITHUB_REPO: &str = "berth-app/berth-agent";

/// Fetch the latest agent version from GitHub Releases (cached 5 min).
async fn get_latest_agent_version() -> Result<String, String> {
    use std::sync::OnceLock;
    use std::time::{Duration, Instant};

    static CACHE: OnceLock<std::sync::Mutex<(String, Instant)>> = OnceLock::new();
    let cache = CACHE.get_or_init(|| {
        std::sync::Mutex::new((String::new(), Instant::now() - Duration::from_secs(600)))
    });

    {
        let guard = cache.lock().unwrap();
        if guard.1.elapsed() < Duration::from_secs(300) && !guard.0.is_empty() {
            return Ok(guard.0.clone());
        }
    }

    let client = reqwest::Client::builder()
        .user_agent("berth-app")
        .build()
        .map_err(|e| format!("HTTP client error: {e}"))?;

    let url = format!("https://api.github.com/repos/{AGENT_GITHUB_REPO}/releases/latest");
    let resp = client
        .get(&url)
        .header("Accept", "application/vnd.github+json")
        .send()
        .await
        .map_err(|e| format!("Failed to check latest agent version: {e}"))?;

    if !resp.status().is_success() {
        return Err(format!("GitHub API returned {}", resp.status()));
    }

    let json: serde_json::Value = resp
        .json()
        .await
        .map_err(|e| format!("Failed to parse GitHub response: {e}"))?;

    let tag = json["tag_name"]
        .as_str()
        .ok_or_else(|| "No tag_name in release".to_string())?;

    let version = tag.trim_start_matches('v').to_string();

    *cache.lock().unwrap() = (version.clone(), Instant::now());
    Ok(version)
}

#[derive(Clone, Serialize)]
pub struct UpgradeCheck {
    pub available: bool,
    pub current_version: String,
    pub latest_version: String,
    pub arch: Option<String>,
}

#[derive(Clone, Serialize)]
pub struct UpgradeResult {
    pub target_id: String,
    pub target_name: String,
    pub success: bool,
    pub new_version: String,
    pub message: String,
}

#[derive(Clone, Serialize)]
pub struct RollbackResult {
    pub success: bool,
    pub restored_version: String,
    pub message: String,
}

#[tauri::command]
pub async fn check_agent_upgrade(id: String) -> Result<UpgradeCheck, String> {
    let store = get_store()?;
    let uuid: Uuid = id.parse().map_err(|e: uuid::Error| e.to_string())?;

    let targets = store.list_targets().map_err(|e| e.to_string())?;
    let target = targets
        .iter()
        .find(|t| t.id == uuid)
        .ok_or_else(|| format!("Target {} not found", id))?;

    let current_version = target.agent_version.clone().unwrap_or_default();
    let latest_version = get_latest_agent_version().await.unwrap_or_default();

    let available = if current_version.is_empty() || latest_version.is_empty() {
        false
    } else {
        match (
            semver::Version::parse(&current_version),
            semver::Version::parse(&latest_version),
        ) {
            (Ok(current), Ok(latest)) => current < latest,
            _ => false,
        }
    };

    Ok(UpgradeCheck {
        available,
        current_version,
        latest_version,
        arch: None,
    })
}

/// Resolve the download URL and SHA-256 checksum for an agent binary.
/// Downloads the binary on desktop to compute checksum, caches it, returns (url, checksum, token).
async fn get_agent_download_info(version: &str, arch: &str) -> Result<(String, String, Option<String>), String> {
    use sha2::{Sha256, Digest};

    let store = get_store()?;
    let binary_name = format!("berth-agent-linux-{arch}");

    // Check local cache for pre-computed checksum
    let cache_dir = dirs_next::data_dir()
        .unwrap_or_else(|| std::path::PathBuf::from("/tmp"))
        .join("com.berth.app")
        .join("agent-binaries")
        .join(format!("v{version}"));

    let cached_path = cache_dir.join(&binary_name);
    let checksum_path = cache_dir.join(format!("{binary_name}.sha256"));

    let github_token = store
        .get_all_settings()
        .ok()
        .and_then(|s| s.get("github_token").cloned())
        .filter(|t| !t.is_empty());

    let client = reqwest::Client::builder()
        .user_agent("berth-app")
        .build()
        .map_err(|e| format!("Failed to create HTTP client: {e}"))?;

    // Resolve the download URL the agent will use
    let (download_url, is_private) = if let Some(token) = &github_token {
        let api_url = format!(
            "https://api.github.com/repos/{AGENT_GITHUB_REPO}/releases/tags/v{version}"
        );
        let release: serde_json::Value = client
            .get(&api_url)
            .header("Authorization", format!("Bearer {token}"))
            .header("Accept", "application/vnd.github+json")
            .send()
            .await
            .map_err(|e| format!("Failed to query GitHub release API: {e}"))?
            .json::<serde_json::Value>()
            .await
            .map_err(|e| format!("Failed to parse release JSON: {e}"))?;

        let asset_url = release["assets"]
            .as_array()
            .and_then(|assets: &Vec<serde_json::Value>| {
                assets.iter().find(|a| a["name"].as_str() == Some(&binary_name))
            })
            .and_then(|a| a["url"].as_str())
            .ok_or_else(|| format!("Asset {binary_name} not found in release v{version}"))?
            .to_string();

        (asset_url, true)
    } else {
        let url = format!(
            "https://github.com/{AGENT_GITHUB_REPO}/releases/download/v{version}/{binary_name}"
        );
        (url, false)
    };

    // If we have a cached checksum, use it
    if checksum_path.exists() {
        let checksum = std::fs::read_to_string(&checksum_path)
            .map_err(|e| format!("Failed to read cached checksum: {e}"))?;
        return Ok((download_url, checksum.trim().to_string(), if is_private { github_token } else { None }));
    }

    // Download binary to compute checksum
    let response = if let Some(token) = &github_token {
        client
            .get(&download_url)
            .header("Authorization", format!("Bearer {token}"))
            .header("Accept", "application/octet-stream")
            .send()
            .await
            .map_err(|e| format!("Failed to download agent binary: {e}"))?
    } else {
        client
            .get(&download_url)
            .send()
            .await
            .map_err(|e| format!("Failed to download agent binary: {e}"))?
    };

    if !response.status().is_success() {
        return Err(format!(
            "Download failed: HTTP {}. {}",
            response.status(),
            if github_token.is_none() {
                "Repo may be private — set a 'github_token' in Settings."
            } else {
                "Check that the release exists and the token has repo access."
            }
        ));
    }

    let bytes = response
        .bytes()
        .await
        .map_err(|e| format!("Failed to read download response: {e}"))?;

    // Compute SHA-256
    let mut hasher = Sha256::new();
    hasher.update(&bytes);
    let checksum = format!("{:x}", hasher.finalize());

    // Cache binary and checksum
    std::fs::create_dir_all(&cache_dir)
        .map_err(|e| format!("Failed to create cache directory: {e}"))?;
    std::fs::write(&cached_path, &bytes)
        .map_err(|e| format!("Failed to cache binary: {e}"))?;
    std::fs::write(&checksum_path, &checksum)
        .map_err(|e| format!("Failed to cache checksum: {e}"))?;

    Ok((download_url, checksum, if is_private { github_token } else { None }))
}

#[tauri::command]
pub async fn upgrade_agent(id: String) -> Result<UpgradeResult, String> {
    let store = get_store()?;
    let uuid: Uuid = id.parse().map_err(|e: uuid::Error| e.to_string())?;

    let targets = store.list_targets().map_err(|e| e.to_string())?;
    let target = targets
        .iter()
        .find(|t| t.id == uuid)
        .ok_or_else(|| format!("Target {} not found", id))?;

    let target_name = target.name.clone();
    let target_id = target.id.to_string();

    // Get agent arch via health check
    let client = get_agent_client(Some(&id)).await?;
    let health = client.health().await.map_err(|e| format!("Health check failed: {e}"))?;

    let arch = health.arch.as_deref().unwrap_or("x86_64");

    // Resolve latest agent version from GitHub
    let latest_version = get_latest_agent_version().await?;

    // Resolve download URL and checksum from GitHub
    let (download_url, checksum, github_token) = get_agent_download_info(&latest_version, arch).await?;

    // Send upgrade command — agent downloads directly
    match client.upgrade(&latest_version, &download_url, github_token.as_deref(), &checksum).await {
        Ok((success, new_version, message)) => {
            if success {
                let _ = store.update_target_status(
                    uuid,
                    berth_core::target::TargetStatus::Online,
                    Some(&new_version),
                );
            }
            Ok(UpgradeResult {
                target_id,
                target_name,
                success,
                new_version,
                message,
            })
        }
        Err(e) => Ok(UpgradeResult {
            target_id,
            target_name,
            success: false,
            new_version: String::new(),
            message: format!("Upgrade failed: {e}"),
        }),
    }
}

#[tauri::command]
pub async fn rollback_agent(id: String) -> Result<RollbackResult, String> {
    let client = get_agent_client(Some(&id)).await?;

    match client.rollback().await {
        Ok((success, restored_version, message)) => {
            if success {
                let store = get_store()?;
                let uuid: Uuid = id.parse().map_err(|e: uuid::Error| e.to_string())?;
                let _ = store.update_target_status(
                    uuid,
                    berth_core::target::TargetStatus::Online,
                    Some(&restored_version),
                );
            }
            Ok(RollbackResult {
                success,
                restored_version,
                message,
            })
        }
        Err(e) => Err(format!("Rollback failed: {e}")),
    }
}

#[tauri::command]
pub async fn upgrade_all_agents() -> Result<Vec<UpgradeResult>, String> {
    let store = get_store()?;
    let targets = store.list_targets().map_err(|e| e.to_string())?;

    let latest_str = get_latest_agent_version().await?;
    let latest = semver::Version::parse(&latest_str)
        .map_err(|e| format!("Invalid latest agent version: {e}"))?;

    let mut results = Vec::new();

    for target in &targets {
        let _current = match &target.agent_version {
            Some(v) => match semver::Version::parse(v) {
                Ok(ver) if ver < latest => ver,
                _ => continue,
            },
            None => continue,
        };

        let id = target.id.to_string();
        match upgrade_agent(id).await {
            Ok(result) => results.push(result),
            Err(e) => results.push(UpgradeResult {
                target_id: target.id.to_string(),
                target_name: target.name.clone(),
                success: false,
                new_version: String::new(),
                message: e,
            }),
        }
    }

    Ok(results)
}

// --- Tunnel / Publish commands ---

#[derive(Serialize)]
pub struct PublishResult {
    pub success: bool,
    pub url: String,
    pub provider: String,
    pub message: String,
}

#[derive(Serialize)]
pub struct UnpublishResult {
    pub success: bool,
    pub message: String,
}

#[tauri::command]
pub async fn publish_project(
    id: String,
    port: u16,
    provider: Option<String>,
    target: Option<String>,
) -> Result<PublishResult, String> {
    let client = get_agent_client(target.as_deref()).await?;
    let provider = provider.as_deref().unwrap_or("cloudflared");

    let (success, url, used_provider, message) = client
        .publish(&id, port, provider, "")
        .await
        .map_err(|e| e.to_string())?;

    if success {
        let store = get_store()?;
        let uuid: Uuid = id.parse().map_err(|e: uuid::Error| e.to_string())?;
        store
            .set_tunnel_url(uuid, &url, &used_provider)
            .map_err(|e| e.to_string())?;
    }

    Ok(PublishResult {
        success,
        url,
        provider: used_provider,
        message,
    })
}

#[tauri::command]
pub async fn unpublish_project(
    id: String,
    target: Option<String>,
) -> Result<UnpublishResult, String> {
    let client = get_agent_client(target.as_deref()).await?;

    let (success, message) = client
        .unpublish(&id)
        .await
        .map_err(|e| e.to_string())?;

    if success {
        let store = get_store()?;
        let uuid: Uuid = id.parse().map_err(|e: uuid::Error| e.to_string())?;
        store.clear_tunnel_url(uuid).map_err(|e| e.to_string())?;
    }

    Ok(UnpublishResult { success, message })
}

// --- Environment variable commands ---

#[tauri::command]
pub fn get_env_vars(project_id: String) -> Result<HashMap<String, String>, String> {
    let store = get_store()?;
    let uuid: Uuid = project_id.parse().map_err(|e: uuid::Error| e.to_string())?;
    store.get_env_vars(uuid).map_err(|e| e.to_string())
}

#[tauri::command]
pub fn set_env_var(project_id: String, key: String, value: String) -> Result<(), String> {
    let store = get_store()?;
    let uuid: Uuid = project_id.parse().map_err(|e: uuid::Error| e.to_string())?;
    store.set_env_var(uuid, &key, &value).map_err(|e| e.to_string())
}

#[tauri::command]
pub fn delete_env_var(project_id: String, key: String) -> Result<(), String> {
    let store = get_store()?;
    let uuid: Uuid = project_id.parse().map_err(|e: uuid::Error| e.to_string())?;
    store.delete_env_var(uuid, &key).map_err(|e| e.to_string())
}

#[tauri::command]
pub fn import_env_file(project_id: String, content: String) -> Result<u32, String> {
    let store = get_store()?;
    let uuid: Uuid = project_id.parse().map_err(|e: uuid::Error| e.to_string())?;
    let vars = berth_core::env::parse_dotenv(&content);
    let count = vars.len() as u32;
    for (key, value) in vars {
        store.set_env_var(uuid, &key, &value).map_err(|e| e.to_string())?;
    }
    Ok(count)
}

// --- Template Store ---

#[derive(Serialize)]
pub struct StoreListResponse {
    pub categories: Vec<berth_core::template_store::TemplateCategory>,
    pub templates: Vec<StoreTemplateItem>,
}

#[derive(Serialize)]
pub struct StoreTemplateItem {
    pub id: String,
    pub name: String,
    pub description: String,
    pub category: String,
    pub runtime: String,
    pub entrypoint: String,
    pub version: String,
    pub author: String,
    pub pro_only: bool,
    pub featured: bool,
    pub env_vars: Vec<berth_core::template_store::TemplateEnvHint>,
    pub tags: Vec<String>,
    pub install_count: u32,
}

impl StoreTemplateItem {
    fn from_meta(
        meta: &berth_core::template_store::TemplateMeta,
        install_counts: &std::collections::HashMap<String, u32>,
    ) -> Self {
        Self {
            id: meta.id.clone(),
            name: meta.name.clone(),
            description: meta.description.clone(),
            category: meta.category.clone(),
            runtime: meta.runtime.clone(),
            entrypoint: meta.entrypoint.clone(),
            version: meta.version.clone(),
            author: meta.author.clone(),
            pro_only: meta.pro_only,
            featured: meta.featured,
            env_vars: meta.env_vars.clone(),
            tags: meta.tags.clone(),
            install_count: install_counts.get(&meta.id).copied().unwrap_or(0),
        }
    }
}

#[tauri::command]
pub async fn store_list_templates(
    category: Option<String>,
    force_refresh: bool,
) -> Result<StoreListResponse, String> {
    let catalog = berth_core::template_store::get_catalog(force_refresh)
        .await
        .map_err(|e| e.to_string())?;

    let store = get_store()?;
    let install_counts = store
        .get_template_install_counts()
        .map_err(|e| e.to_string())?;

    let templates: Vec<StoreTemplateItem> = match &category {
        Some(cat) => berth_core::template_store::filter_by_category(&catalog, cat)
            .into_iter()
            .map(|t| StoreTemplateItem::from_meta(t, &install_counts))
            .collect(),
        None => catalog
            .templates
            .iter()
            .map(|t| StoreTemplateItem::from_meta(t, &install_counts))
            .collect(),
    };

    Ok(StoreListResponse {
        categories: catalog.categories,
        templates,
    })
}

#[tauri::command]
pub async fn store_search_templates(
    query: String,
) -> Result<StoreListResponse, String> {
    let catalog = berth_core::template_store::get_catalog(false)
        .await
        .map_err(|e| e.to_string())?;

    let store = get_store()?;
    let install_counts = store
        .get_template_install_counts()
        .map_err(|e| e.to_string())?;

    let templates: Vec<StoreTemplateItem> =
        berth_core::template_store::search_templates(&catalog, &query)
            .into_iter()
            .map(|t| StoreTemplateItem::from_meta(t, &install_counts))
            .collect();

    Ok(StoreListResponse {
        categories: catalog.categories,
        templates,
    })
}

#[tauri::command]
pub async fn store_install_template(
    template_id: String,
) -> Result<berth_core::project::Project, String> {
    let catalog = berth_core::template_store::get_catalog(false)
        .await
        .map_err(|e| e.to_string())?;

    let template = catalog
        .templates
        .iter()
        .find(|t| t.id == template_id)
        .ok_or_else(|| format!("Template '{}' not found in store", template_id))?
        .clone();

    // Async download phase (no store needed)
    let project_dir = berth_core::template_store::download_template_files(&template)
        .await
        .map_err(|e| e.to_string())?;

    // Sync store phase (no .await after this)
    let store = get_store()?;
    berth_core::template_store::finalize_template_install(&store, &template, &project_dir)
        .map_err(|e| e.to_string())
}

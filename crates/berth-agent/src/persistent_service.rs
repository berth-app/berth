use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;

use sysinfo::System;
use tokio::sync::Mutex;
use tokio_stream::wrappers::ReceiverStream;
use tonic::{Request, Response, Status, Streaming};
use uuid::Uuid;

use berth_proto::proto::agent_service_server::AgentService;
use berth_proto::proto::*;
use berth_proto::transport::{RemoteExecution, RemoteSchedule, ExecuteResponseLine, DeployResponseLine};
use berth_proto::executor::{LogLine, LogStream};
use berth_proto::runtime::Runtime;
use crate::executor;
use crate::tunnel::{TunnelManager, TunnelProvider};
use crate::{archive, container, setup};

use crate::agent_store::{AgentStore, Deployment, Execution};
use crate::nats_publisher::{self, NatsPublisher};

fn gethostname() -> String {
    System::host_name().unwrap_or_else(|| "unknown".into())
}

use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};

enum ProcessKind {
    BareProcess {
        abort_handle: tokio::task::AbortHandle,
    },
    Container {
        container_name: String,
        abort_handle: tokio::task::AbortHandle,
    },
}

/// Tracks a managed process — oneshot or service mode.
struct ManagedProcess {
    kind: ProcessKind,
    started_at: chrono::DateTime<chrono::Utc>,
    /// Set to true to signal the supervisor to stop restarting.
    cancelled: Arc<AtomicBool>,
    /// Run mode: "oneshot" or "service".
    run_mode: String,
    /// Number of restarts in this session.
    restart_count: Arc<AtomicU32>,
    /// Current supervisor state: "running", "backoff", "stopped".
    supervisor_state: Arc<std::sync::Mutex<String>>,
}

pub struct PersistentAgentService {
    store: Arc<Mutex<AgentStore>>,
    processes: Arc<Mutex<HashMap<String, ManagedProcess>>>,
    tunnel_mgr: Arc<TunnelManager>,
    start_time: std::time::Instant,
    deploys_dir: PathBuf,
    podman_version: Option<String>,
    nats: Option<Arc<NatsPublisher>>,
}

impl PersistentAgentService {
    fn berth_dir() -> PathBuf {
        dirs_next::home_dir()
            .unwrap_or_else(|| PathBuf::from("/tmp"))
            .join(".berth")
    }

    pub fn new(store: Arc<Mutex<AgentStore>>, nats: Option<Arc<NatsPublisher>>) -> Self {
        let deploys_dir = dirs_next::home_dir()
            .unwrap_or_else(|| PathBuf::from("/tmp"))
            .join(".berth/deploys");

        let podman_version = std::process::Command::new("podman")
            .args(["version", "--format", "{{.Client.Version}}"])
            .output()
            .ok()
            .filter(|o| o.status.success())
            .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string());

        Self {
            store,
            processes: Arc::new(Mutex::new(HashMap::new())),
            tunnel_mgr: Arc::new(TunnelManager::new()),
            start_time: std::time::Instant::now(),
            deploys_dir,
            podman_version,
            nats,
        }
    }

    fn deploy_dir(&self, project_id: &str, version: u32) -> Result<PathBuf, String> {
        // Validate project_id against path traversal
        if project_id.contains("..") || project_id.contains('/') || project_id.contains('\\')
            || project_id.contains('\0')
        {
            return Err(format!("Invalid project_id: contains illegal characters"));
        }
        let path = self.deploys_dir
            .join(project_id)
            .join(format!("v{version}"));
        Ok(path)
    }

    // --- Core logic methods (shared between gRPC and NATS handlers) ---

    pub fn do_health(&self) -> HealthInfo {
        let berth_dir = dirs_next::home_dir()
            .unwrap_or_else(|| PathBuf::from("/tmp"))
            .join(".berth");

        let probation_status = if berth_dir.join(".probation").exists() {
            "in_probation".into()
        } else if berth_dir.join(".rollback-count").exists() {
            "rolled_back".into()
        } else {
            String::new()
        };

        HealthInfo {
            version: env!("CARGO_PKG_VERSION").into(),
            status: "healthy".into(),
            uptime_seconds: self.start_time.elapsed().as_secs(),
            podman_version: self.podman_version.clone().unwrap_or_default(),
            container_ready: self.podman_version.is_some(),
            os: std::env::consts::OS.into(),
            arch: std::env::consts::ARCH.into(),
            probation_status,
            tunnel_providers: TunnelManager::available_providers(),
        }
    }

    pub async fn do_status(&self) -> StatusInfo {
        let procs = self.processes.lock().await;
        let projects: Vec<ProjectStatusInfo> = procs
            .iter()
            .map(|(id, managed)| {
                let supervisor_state = managed.supervisor_state.lock()
                    .map(|s| s.clone())
                    .unwrap_or_else(|_| "running".into());
                let uptime_secs = (chrono::Utc::now() - managed.started_at).num_seconds().max(0) as u64;
                ProjectStatusInfo {
                    project_id: id.clone(),
                    status: "running".into(),
                    started_at: managed.started_at.to_rfc3339(),
                    run_mode: managed.run_mode.clone(),
                    restart_count: managed.restart_count.load(Ordering::Relaxed),
                    supervisor_state,
                    uptime_secs,
                }
            })
            .collect();

        let mut sys = sysinfo::System::new();
        sys.refresh_cpu_usage();
        sys.refresh_memory();

        StatusInfo {
            agent_id: gethostname(),
            status: "running".into(),
            cpu_usage: sys.global_cpu_usage() as f64,
            memory_bytes: sys.used_memory(),
            projects,
        }
    }

    pub async fn do_stop(&self, project_id: &str) -> (bool, String) {
        let mut procs = self.processes.lock().await;

        if let Some(managed) = procs.remove(project_id) {
            // Signal supervisor to stop restarting
            managed.cancelled.store(true, Ordering::SeqCst);

            match managed.kind {
                ProcessKind::BareProcess { abort_handle } => {
                    abort_handle.abort();
                }
                ProcessKind::Container {
                    container_name,
                    abort_handle,
                } => {
                    let _ = container::stop_container(&container_name).await;
                    abort_handle.abort();
                }
            }

            let data = serde_json::json!({"project_id": project_id}).to_string();
            {
                let store = self.store.lock().await;
                let _ = store.insert_event("execution_stopped", Some(project_id), None, &data);
            }
            nats_publisher::maybe_publish_event(&self.nats, "execution_stopped", Some(project_id), None, &data).await;

            // Also stop tunnel if one exists
            let _ = self.tunnel_mgr.stop(project_id).await;

            (true, format!("Stopped project {project_id}"))
        } else {
            (false, format!("Project {project_id} is not running"))
        }
    }

    pub async fn do_publish(
        &self,
        project_id: &str,
        port: u16,
        provider: &str,
        _provider_config: &str,
    ) -> Result<(bool, String, String, String), String> {
        let tunnel_provider = match provider {
            "cloudflared" | "" => TunnelProvider::Cloudflared,
            other => return Err(format!("Unknown tunnel provider: {other}. Available: cloudflared")),
        };

        let info = self
            .tunnel_mgr
            .start(project_id, port, &tunnel_provider)
            .await
            .map_err(|e| e.to_string())?;

        let data = serde_json::json!({
            "project_id": project_id,
            "url": &info.public_url,
            "provider": &info.provider,
            "port": port,
        })
        .to_string();

        nats_publisher::maybe_publish_event(
            &self.nats,
            "tunnel_started",
            Some(project_id),
            None,
            &data,
        )
        .await;

        Ok((
            true,
            info.public_url,
            info.provider,
            "Tunnel started".to_string(),
        ))
    }

    pub async fn do_unpublish(&self, project_id: &str) -> Result<(bool, String), String> {
        self.tunnel_mgr
            .stop(project_id)
            .await
            .map_err(|e| e.to_string())?;

        let data = serde_json::json!({"project_id": project_id}).to_string();
        nats_publisher::maybe_publish_event(
            &self.nats,
            "tunnel_stopped",
            Some(project_id),
            None,
            &data,
        )
        .await;

        Ok((true, "Tunnel stopped".to_string()))
    }

    pub async fn do_execute(
        &self,
        project_id: &str,
        runtime_str: &str,
        entrypoint: &str,
        working_dir: &str,
        code: Option<&[u8]>,
        image_tag: Option<&str>,
        env_vars: HashMap<String, String>,
        container_name: Option<&str>,
        run_mode: &str,
        _service_port: u16,
    ) -> Result<tokio::sync::mpsc::Receiver<ExecuteResponseLine>, String> {
        // Validate inputs against path traversal
        if entrypoint.contains("..") || entrypoint.starts_with('/') || entrypoint.contains('\0') {
            return Err("Invalid entrypoint: must be a relative path without '..'".into());
        }
        if working_dir.contains('\0') {
            return Err("Invalid working_dir: contains null bytes".into());
        }
        // Verify working_dir is within deploys or tmp directory
        if !working_dir.is_empty() && code.is_none() {
            let wd = std::path::Path::new(working_dir);
            if wd.is_absolute() {
                let deploys = &self.deploys_dir;
                let berth_tmp = Self::berth_dir().join("tmp");
                if !wd.starts_with(deploys) && !wd.starts_with(&berth_tmp) {
                    return Err("Invalid working_dir: must be within agent deploy directory".into());
                }
            }
        }

        let is_service = run_mode == "service";
        let runtime = parse_runtime(runtime_str);
        let use_container = image_tag.map_or(false, |t| !t.is_empty());
        let project_id = project_id.to_string();

        let execution_id = Uuid::new_v4().to_string();
        let now = chrono::Utc::now();
        {
            let store = self.store.lock().await;
            let _ = store.insert_execution(&Execution {
                id: execution_id.clone(),
                project_id: project_id.clone(),
                deployment_id: None,
                started_at: now,
                finished_at: None,
                exit_code: None,
                trigger: "manual".into(),
                status: "running".into(),
            });
        }

        let (tx, rx) = tokio::sync::mpsc::channel(256);
        let processes = self.processes.clone();
        let store = self.store.clone();
        let nats = self.nats.clone();

        if use_container {
            let image_tag = image_tag.unwrap().to_string();
            let container_name = container_name
                .map(|s| s.to_string())
                .unwrap_or_else(|| format!("berth-{project_id}"));

            let (mut child, mut child_rx) =
                container::run_container(&image_tag, &container_name, &env_vars)
                    .await
                    .map_err(|e| format!("Failed to run container: {e}"))?;

            let container_name_clone = container_name.clone();
            let exec_id = execution_id.clone();
            let pid = project_id.clone();
            let nats_clone = nats.clone();
            let cancelled = Arc::new(AtomicBool::new(false));
            let run_mode_str = run_mode.to_string();
            let task = tokio::spawn(async move {
                let mut seq: i64 = 0;
                while let Some(line) = child_rx.recv().await {
                    let stream_str = match line.stream {
                        LogStream::Stdout => "stdout",
                        LogStream::Stderr => "stderr",
                    };
                    {
                        let s = store.lock().await;
                        let _ = s.append_log_line(&exec_id, seq, stream_str, &line.text, &line.timestamp);
                    }
                    nats_publisher::maybe_publish_log_line(&nats_clone, &pid, &exec_id, stream_str, &line.text, seq).await;

                    let resp = ExecuteResponseLine {
                        stream: stream_str.into(),
                        text: line.text,
                        timestamp: line.timestamp.to_rfc3339(),
                        exit_code: 0,
                        is_final: false,
                    };
                    if tx.send(resp).await.is_err() { break; }
                    seq += 1;
                }

                let exit_code = match child.wait().await {
                    Ok(status) => status.code().unwrap_or(-1),
                    Err(_) => -1,
                };

                let status_str = if exit_code == 0 { "completed" } else { "failed" };
                let data = serde_json::json!({"exit_code": exit_code, "status": status_str}).to_string();
                {
                    let s = store.lock().await;
                    let _ = s.finish_execution(&exec_id, exit_code, status_str);
                    let _ = s.insert_event("execution_completed", Some(&pid), Some(&exec_id), &data);
                }
                nats_publisher::maybe_publish_event(&nats_clone, "execution_completed", Some(&pid), Some(&exec_id), &data).await;

                let _ = tx.send(ExecuteResponseLine {
                    stream: String::new(),
                    text: String::new(),
                    timestamp: chrono::Utc::now().to_rfc3339(),
                    exit_code,
                    is_final: true,
                }).await;

                let mut procs = processes.lock().await;
                procs.remove(&pid);
            });

            {
                let mut procs = self.processes.lock().await;
                procs.insert(
                    project_id,
                    ManagedProcess {
                        kind: ProcessKind::Container {
                            container_name: container_name_clone,
                            abort_handle: task.abort_handle(),
                        },
                        started_at: chrono::Utc::now(),
                        cancelled,
                        run_mode: run_mode_str,
                        restart_count: Arc::new(AtomicU32::new(0)),
                        supervisor_state: Arc::new(std::sync::Mutex::new("running".to_string())),
                    },
                );
            }
        } else {
            let (actual_working_dir, actual_entrypoint) = if let Some(code_bytes) = code {
                let tmp = Self::berth_dir().join("tmp").join(format!("berth-agent-{}", Uuid::new_v4()));
                std::fs::create_dir_all(&tmp).map_err(|e| format!("Failed to create temp dir: {e}"))?;
                let ep = if entrypoint.is_empty() { "main.py" } else { entrypoint };
                std::fs::write(tmp.join(ep), code_bytes)
                    .map_err(|e| format!("Failed to write code: {e}"))?;
                (tmp.to_string_lossy().to_string(), ep.to_string())
            } else {
                (working_dir.to_string(), entrypoint.to_string())
            };

            let env_ref = if env_vars.is_empty() { None } else { Some(&env_vars) };

            let (child, child_rx) =
                executor::spawn_and_stream(runtime, &actual_entrypoint, &actual_working_dir, env_ref)
                    .await
                    .map_err(|e| format!("Failed to spawn: {e}"))?;

            let exec_id = execution_id.clone();
            let pid = project_id.clone();
            let nats_clone = nats.clone();
            let cancelled = Arc::new(AtomicBool::new(false));
            let cancelled_clone = cancelled.clone();
            let restart_count = Arc::new(AtomicU32::new(0));
            let restart_count_clone = restart_count.clone();
            let supervisor_state = Arc::new(std::sync::Mutex::new("running".to_string()));
            let supervisor_state_clone = supervisor_state.clone();
            let is_service_clone = is_service;
            let run_mode_str = run_mode.to_string();
            let runtime_clone = runtime;
            let entrypoint_clone = actual_entrypoint.clone();
            let working_dir_clone = actual_working_dir.clone();
            let env_vars_clone = env_vars.clone();

            let task = tokio::spawn(async move {
                let mut current_exec_id = exec_id;
                let mut current_child = child;
                let mut current_rx = child_rx;
                let mut consecutive_failures: u32 = 0;

                loop {
                    // Stream logs for current child
                    let mut seq: i64 = 0;
                    while let Some(line) = current_rx.recv().await {
                        let stream_str = match line.stream {
                            LogStream::Stdout => "stdout",
                            LogStream::Stderr => "stderr",
                        };
                        {
                            let s = store.lock().await;
                            let _ = s.append_log_line(&current_exec_id, seq, stream_str, &line.text, &line.timestamp);
                        }
                        nats_publisher::maybe_publish_log_line(&nats_clone, &pid, &current_exec_id, stream_str, &line.text, seq).await;

                        let resp = ExecuteResponseLine {
                            stream: stream_str.into(),
                            text: line.text,
                            timestamp: line.timestamp.to_rfc3339(),
                            exit_code: 0,
                            is_final: false,
                        };
                        if tx.send(resp).await.is_err() { break; }
                        seq += 1;
                    }

                    let start_time = std::time::Instant::now();
                    let exit_code = match current_child.wait().await {
                        Ok(status) => status.code().unwrap_or(-1),
                        Err(_) => -1,
                    };

                    let status_str = if exit_code == 0 { "completed" } else { "failed" };
                    let data = serde_json::json!({"exit_code": exit_code, "status": status_str}).to_string();
                    {
                        let s = store.lock().await;
                        let _ = s.finish_execution(&current_exec_id, exit_code, status_str);
                        let _ = s.insert_event("execution_completed", Some(&pid), Some(&current_exec_id), &data);
                    }
                    nats_publisher::maybe_publish_event(&nats_clone, "execution_completed", Some(&pid), Some(&current_exec_id), &data).await;

                    // Check if we should restart (service mode only)
                    if !is_service_clone || cancelled_clone.load(Ordering::SeqCst) {
                        // Oneshot or cancelled — send final and exit
                        let _ = tx.send(ExecuteResponseLine {
                            stream: String::new(),
                            text: String::new(),
                            timestamp: chrono::Utc::now().to_rfc3339(),
                            exit_code,
                            is_final: true,
                        }).await;
                        break;
                    }

                    // Service mode: compute backoff and restart
                    let ran_for = start_time.elapsed();
                    if ran_for.as_secs() > 60 {
                        consecutive_failures = 0;
                    } else {
                        consecutive_failures += 1;
                    }

                    let count = restart_count_clone.fetch_add(1, Ordering::Relaxed) + 1;

                    // Exponential backoff: 1s * 2^failures, capped at 60s
                    let base_delay_ms = 1000u64 * 2u64.saturating_pow(consecutive_failures.min(6));
                    let delay_ms = base_delay_ms.min(60_000);
                    // Add ±10% jitter
                    let jitter = (delay_ms as f64 * 0.1 * (0.5 - rand_simple())) as i64;
                    let actual_delay_ms = (delay_ms as i64 + jitter).max(100) as u64;

                    // Notify about restart
                    let restart_msg = format!(
                        "\n--- Service restarting (attempt {}, exit code {}, backoff {}ms) ---\n",
                        count, exit_code, actual_delay_ms
                    );
                    let _ = tx.send(ExecuteResponseLine {
                        stream: "stderr".into(),
                        text: restart_msg,
                        timestamp: chrono::Utc::now().to_rfc3339(),
                        exit_code: 0,
                        is_final: false,
                    }).await;

                    // Publish restart event
                    let restart_data = serde_json::json!({
                        "restart_count": count,
                        "exit_code": exit_code,
                        "delay_ms": actual_delay_ms,
                    }).to_string();
                    nats_publisher::maybe_publish_event(&nats_clone, "service_restarting", Some(&pid), None, &restart_data).await;

                    // Set state to backoff
                    if let Ok(mut s) = supervisor_state_clone.lock() {
                        *s = "backoff".to_string();
                    }

                    // Interruptible sleep — check cancelled every 100ms
                    let sleep_until = std::time::Instant::now() + std::time::Duration::from_millis(actual_delay_ms);
                    loop {
                        if cancelled_clone.load(Ordering::SeqCst) {
                            let _ = tx.send(ExecuteResponseLine {
                                stream: String::new(),
                                text: String::new(),
                                timestamp: chrono::Utc::now().to_rfc3339(),
                                exit_code,
                                is_final: true,
                            }).await;
                            // Break out of both loops
                            let mut procs = processes.lock().await;
                            procs.remove(&pid);
                            return;
                        }
                        if std::time::Instant::now() >= sleep_until {
                            break;
                        }
                        tokio::time::sleep(std::time::Duration::from_millis(100)).await;
                    }

                    // Set state back to running
                    if let Ok(mut s) = supervisor_state_clone.lock() {
                        *s = "running".to_string();
                    }

                    // Spawn new child process
                    let env_ref = if env_vars_clone.is_empty() { None } else { Some(&env_vars_clone) };
                    match executor::spawn_and_stream(runtime_clone, &entrypoint_clone, &working_dir_clone, env_ref).await {
                        Ok((new_child, new_rx)) => {
                            // Create new execution record
                            let new_exec_id = Uuid::new_v4().to_string();
                            {
                                let s = store.lock().await;
                                let _ = s.insert_execution(&Execution {
                                    id: new_exec_id.clone(),
                                    project_id: pid.clone(),
                                    deployment_id: None,
                                    started_at: chrono::Utc::now(),
                                    finished_at: None,
                                    exit_code: None,
                                    trigger: "service_restart".into(),
                                    status: "running".into(),
                                });
                            }

                            let restart_msg = format!("--- Service restarted (attempt {}) ---\n", count);
                            let _ = tx.send(ExecuteResponseLine {
                                stream: "stderr".into(),
                                text: restart_msg,
                                timestamp: chrono::Utc::now().to_rfc3339(),
                                exit_code: 0,
                                is_final: false,
                            }).await;

                            nats_publisher::maybe_publish_event(&nats_clone, "service_restarted", Some(&pid), None, &serde_json::json!({"restart_count": count}).to_string()).await;

                            // Update the managed process's started_at
                            {
                                let mut procs = processes.lock().await;
                                if let Some(managed) = procs.get_mut(&pid) {
                                    managed.started_at = chrono::Utc::now();
                                }
                            }

                            current_exec_id = new_exec_id;
                            current_child = new_child;
                            current_rx = new_rx;
                            // Continue loop
                        }
                        Err(e) => {
                            let error_msg = format!("--- Service restart failed: {} ---\n", e);
                            let _ = tx.send(ExecuteResponseLine {
                                stream: "stderr".into(),
                                text: error_msg,
                                timestamp: chrono::Utc::now().to_rfc3339(),
                                exit_code: 0,
                                is_final: false,
                            }).await;
                            // Don't give up — try again after backoff
                            continue;
                        }
                    }
                }

                let mut procs = processes.lock().await;
                procs.remove(&pid);
            });

            {
                let mut procs = self.processes.lock().await;
                procs.insert(
                    project_id,
                    ManagedProcess {
                        kind: ProcessKind::BareProcess {
                            abort_handle: task.abort_handle(),
                        },
                        started_at: chrono::Utc::now(),
                        cancelled,
                        run_mode: run_mode_str,
                        restart_count,
                        supervisor_state,
                    },
                );
            }
        }

        Ok(rx)
    }

    pub async fn do_deploy(
        &self,
        project_id: &str,
        runtime: &str,
        entrypoint: &str,
        source_archive: &[u8],
        containerfile: &str,
        version: u32,
        setup_commands: Vec<String>,
    ) -> Result<tokio::sync::mpsc::Receiver<DeployResponseLine>, String> {
        let deploy_dir = self.deploy_dir(project_id, version)?;
        std::fs::create_dir_all(&deploy_dir)
            .map_err(|e| format!("Failed to create deploy dir: {e}"))?;
        archive::extract(source_archive, &deploy_dir)
            .map_err(|e| format!("Failed to extract archive: {e}"))?;

        let (tx, rx) = tokio::sync::mpsc::channel(256);
        let _ = tx.send(DeployResponseLine {
            phase: "extracting".into(),
            text: format!("Extracting source to {}", deploy_dir.display()),
            timestamp: chrono::Utc::now().to_rfc3339(),
            image_tag: String::new(),
            version,
            is_final: false,
            success: false,
        }).await;

        let use_container = self.podman_version.is_some() && !containerfile.is_empty();
        let store = self.store.clone();
        let deploy_dir_str = deploy_dir.to_string_lossy().to_string();
        let nats = self.nats.clone();
        let project_id = project_id.to_string();
        let runtime = runtime.to_string();
        let entrypoint = entrypoint.to_string();
        let containerfile = containerfile.to_string();

        if use_container {
            tokio::spawn(async move {
                let _ = tx.send(DeployResponseLine { phase: "building".into(), text: "Building container image...".into(), timestamp: chrono::Utc::now().to_rfc3339(), image_tag: String::new(), version, is_final: false, success: false }).await;

                let (build_tx, mut build_rx) = tokio::sync::mpsc::channel(256);
                let build_task = tokio::spawn({
                    let pid = project_id.clone();
                    let dd = deploy_dir.clone();
                    let cf = containerfile.clone();
                    async move { container::build_image(&pid, version, &cf, &dd, build_tx).await }
                });

                while let Some(line) = build_rx.recv().await {
                    let _ = tx.send(DeployResponseLine { phase: "building".into(), text: line.text, timestamp: line.timestamp.to_rfc3339(), image_tag: String::new(), version, is_final: false, success: false }).await;
                }

                match build_task.await {
                    Ok(Ok(image_tag)) => {
                        {
                            let s = store.lock().await;
                            let _ = s.insert_deployment(&Deployment { id: Uuid::new_v4().to_string(), project_id: project_id.clone(), version, runtime, entrypoint, working_dir: deploy_dir_str, image_tag: Some(image_tag.clone()), status: "deployed".into(), deployed_at: chrono::Utc::now() });
                            let data = serde_json::json!({"version": version, "image_tag": &image_tag, "success": true}).to_string();
                            let _ = s.insert_event("deploy_completed", Some(&project_id), None, &data);
                            let _ = s.prune_old_deployments(&project_id, 5);
                        }
                        nats_publisher::maybe_publish_event(&nats, "deploy_completed", Some(&project_id), None, &serde_json::json!({"version": version, "image_tag": &image_tag, "success": true}).to_string()).await;
                        let _ = tx.send(DeployResponseLine { phase: "ready".into(), text: format!("Image built: {image_tag}"), timestamp: chrono::Utc::now().to_rfc3339(), image_tag, version, is_final: true, success: true }).await;
                    }
                    Ok(Err(e)) => {
                        let _ = tx.send(DeployResponseLine { phase: "error".into(), text: format!("Build failed: {e}"), timestamp: chrono::Utc::now().to_rfc3339(), image_tag: String::new(), version, is_final: true, success: false }).await;
                    }
                    Err(e) => {
                        let _ = tx.send(DeployResponseLine { phase: "error".into(), text: format!("Build task panicked: {e}"), timestamp: chrono::Utc::now().to_rfc3339(), image_tag: String::new(), version, is_final: true, success: false }).await;
                    }
                }
            });
        } else {
            tokio::spawn(async move {
                if setup_commands.is_empty() {
                    {
                        let s = store.lock().await;
                        let _ = s.insert_deployment(&Deployment { id: Uuid::new_v4().to_string(), project_id: project_id.clone(), version, runtime, entrypoint, working_dir: deploy_dir_str, image_tag: None, status: "deployed".into(), deployed_at: chrono::Utc::now() });
                        let data = serde_json::json!({"version": version, "success": true}).to_string();
                        let _ = s.insert_event("deploy_completed", Some(&project_id), None, &data);
                        let _ = s.prune_old_deployments(&project_id, 5);
                    }
                    nats_publisher::maybe_publish_event(&nats, "deploy_completed", Some(&project_id), None, &serde_json::json!({"version": version, "success": true}).to_string()).await;
                    let _ = tx.send(DeployResponseLine { phase: "ready".into(), text: "No setup needed".into(), timestamp: chrono::Utc::now().to_rfc3339(), image_tag: String::new(), version, is_final: true, success: true }).await;
                    return;
                }

                let _ = tx.send(DeployResponseLine { phase: "setup".into(), text: "Running setup commands...".into(), timestamp: chrono::Utc::now().to_rfc3339(), image_tag: String::new(), version, is_final: false, success: false }).await;

                let (setup_tx, mut setup_rx) = tokio::sync::mpsc::channel(256);
                let setup_task = tokio::spawn({
                    let dir = deploy_dir.clone();
                    let cmds = setup_commands.clone();
                    async move { setup::run_setup_commands(&dir, &cmds, setup_tx).await }
                });

                while let Some(line) = setup_rx.recv().await {
                    let _ = tx.send(DeployResponseLine { phase: "setup".into(), text: line.text, timestamp: line.timestamp.to_rfc3339(), image_tag: String::new(), version, is_final: false, success: false }).await;
                }

                match setup_task.await {
                    Ok(Ok(())) => {
                        {
                            let s = store.lock().await;
                            let _ = s.insert_deployment(&Deployment { id: Uuid::new_v4().to_string(), project_id: project_id.clone(), version, runtime, entrypoint, working_dir: deploy_dir_str, image_tag: None, status: "deployed".into(), deployed_at: chrono::Utc::now() });
                            let data = serde_json::json!({"version": version, "success": true}).to_string();
                            let _ = s.insert_event("deploy_completed", Some(&project_id), None, &data);
                            let _ = s.prune_old_deployments(&project_id, 5);
                        }
                        nats_publisher::maybe_publish_event(&nats, "deploy_completed", Some(&project_id), None, &serde_json::json!({"version": version, "success": true}).to_string()).await;
                        let _ = tx.send(DeployResponseLine { phase: "ready".into(), text: "Setup complete".into(), timestamp: chrono::Utc::now().to_rfc3339(), image_tag: String::new(), version, is_final: true, success: true }).await;
                    }
                    Ok(Err(e)) => {
                        let _ = tx.send(DeployResponseLine { phase: "error".into(), text: format!("Setup failed: {e}"), timestamp: chrono::Utc::now().to_rfc3339(), image_tag: String::new(), version, is_final: true, success: false }).await;
                    }
                    Err(e) => {
                        let _ = tx.send(DeployResponseLine { phase: "error".into(), text: format!("Setup task panicked: {e}"), timestamp: chrono::Utc::now().to_rfc3339(), image_tag: String::new(), version, is_final: true, success: false }).await;
                    }
                }
            });
        }

        Ok(rx)
    }

    pub async fn do_get_executions(&self, project_id: &str, limit: u32) -> Result<Vec<RemoteExecution>, String> {
        let store = self.store.lock().await;
        let executions = store.list_executions(project_id, limit).map_err(|e| e.to_string())?;
        Ok(executions.into_iter().map(|e| RemoteExecution {
            id: e.id,
            project_id: e.project_id,
            deployment_id: e.deployment_id.unwrap_or_default(),
            started_at: e.started_at.to_rfc3339(),
            finished_at: e.finished_at.map(|t| t.to_rfc3339()).unwrap_or_default(),
            exit_code: e.exit_code.unwrap_or(0),
            trigger: e.trigger,
            status: e.status,
        }).collect())
    }

    pub async fn do_get_execution_logs(&self, execution_id: &str, since_seq: i64) -> Result<Vec<LogLine>, String> {
        let store = self.store.lock().await;
        let logs = store.get_logs(execution_id, since_seq).map_err(|e| e.to_string())?;
        Ok(logs.into_iter().map(|l| {
            let stream = match l.stream.as_str() {
                "stderr" => LogStream::Stderr,
                _ => LogStream::Stdout,
            };
            LogLine { stream, text: l.text, timestamp: l.timestamp }
        }).collect())
    }

    pub async fn do_add_schedule(&self, project_id: &str, cron_expr: &str) -> Result<(String, String), String> {
        let now = chrono::Utc::now();
        let next = berth_proto::schedule::parse_next_run(cron_expr, now);
        if next.is_none() {
            return Err(format!("Invalid cron expression: {cron_expr}"));
        }
        let id = Uuid::new_v4().to_string();
        let schedule = crate::agent_store::AgentSchedule {
            id: id.clone(),
            project_id: project_id.to_string(),
            cron_expr: cron_expr.to_string(),
            enabled: true,
            created_at: now,
            last_triggered_at: None,
            next_run_at: next,
        };
        let store = self.store.lock().await;
        store.insert_schedule(&schedule).map_err(|e| e.to_string())?;
        Ok((id, next.map(|t: chrono::DateTime<chrono::Utc>| t.to_rfc3339()).unwrap_or_default()))
    }

    pub async fn do_remove_schedule(&self, schedule_id: &str) -> Result<bool, String> {
        let store = self.store.lock().await;
        store.delete_schedule(schedule_id).map_err(|e| e.to_string())
    }

    pub async fn do_list_schedules(&self, project_id: &str) -> Result<Vec<RemoteSchedule>, String> {
        let store = self.store.lock().await;
        let schedules = store.list_schedules(project_id).map_err(|e| e.to_string())?;
        Ok(schedules.into_iter().map(|s| RemoteSchedule {
            id: s.id,
            project_id: s.project_id,
            cron_expr: s.cron_expr,
            enabled: s.enabled,
            created_at: s.created_at.to_rfc3339(),
            last_triggered_at: s.last_triggered_at.map(|t| t.to_rfc3339()).unwrap_or_default(),
            next_run_at: s.next_run_at.map(|t| t.to_rfc3339()).unwrap_or_default(),
        }).collect())
    }

    pub async fn do_upgrade_from_bytes(&self, data: Vec<u8>) -> Result<(bool, String, String), String> {
        let berth_dir = Self::berth_dir();
        let bin_dir = berth_dir.join("bin");
        let active_path = bin_dir.join("berth-agent");
        let staging_path = bin_dir.join("berth-agent.new");
        let backup_path = bin_dir.join("berth-agent.old");

        if data.is_empty() {
            return Err("No upgrade data received".into());
        }

        tracing::info!("Received upgrade binary: {} bytes", data.len());

        // Write to staging file
        std::fs::create_dir_all(&bin_dir)
            .map_err(|e| format!("Failed to create bin dir: {e}"))?;
        std::fs::write(&staging_path, &data)
            .map_err(|e| format!("Failed to write staging binary: {e}"))?;

        // Make executable
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&staging_path, std::fs::Permissions::from_mode(0o755))
                .map_err(|e| format!("Failed to chmod: {e}"))?;
        }

        // Verify the new binary
        let verify = std::process::Command::new(&staging_path)
            .arg("--version")
            .output();

        match verify {
            Ok(output) if output.status.success() => {
                let new_version = String::from_utf8_lossy(&output.stdout).trim().to_string();
                tracing::info!("Verified new agent version: {new_version}");

                // Backup: rename current → .old (remove stale backup first)
                let _ = std::fs::remove_file(&backup_path);
                if active_path.exists() {
                    std::fs::rename(&active_path, &backup_path)
                        .map_err(|e| format!("Failed to backup current binary: {e}"))?;
                }

                // Promote: rename .new → active (atomic on same filesystem)
                std::fs::rename(&staging_path, &active_path)
                    .map_err(|e| format!("Failed to promote new binary: {e}. Backup at {}", backup_path.display()))?;

                // Emit upgrade event
                let event_data = serde_json::json!({
                    "old_version": env!("CARGO_PKG_VERSION"),
                    "new_version": &new_version,
                }).to_string();
                {
                    let store = self.store.lock().await;
                    let _ = store.insert_event("agent_upgraded", None, None, &event_data);
                }
                nats_publisher::maybe_publish_event(&self.nats, "agent_upgraded", None, None, &event_data).await;

                // Write markers for probation mode
                let upgrade_meta = serde_json::json!({
                    "old_version": env!("CARGO_PKG_VERSION"),
                    "new_version": &new_version,
                    "upgraded_at": chrono::Utc::now().to_rfc3339(),
                }).to_string();
                let _ = std::fs::write(berth_dir.join(".upgrading"), &upgrade_meta);
                let _ = std::fs::write(berth_dir.join(".probation"), &upgrade_meta);
                let _ = std::fs::remove_file(berth_dir.join(".rollback-count"));

                // Flush NATS before exiting so the response is delivered
                if let Some(nats) = &self.nats {
                    let _ = nats.client().flush().await;
                }

                // Exit with code 42 — systemd (SuccessExitStatus=42) will
                // restart us with the new binary. No sudo/systemctl needed.
                tokio::spawn(async {
                    tokio::time::sleep(tokio::time::Duration::from_secs(2)).await;
                    tracing::info!("Exiting for upgrade restart (exit code 42)");
                    std::process::exit(42);
                });

                Ok((true, new_version, "Upgrade successful, restarting...".into()))
            }
            Ok(output) => {
                let _ = std::fs::remove_file(&staging_path);
                Err(format!("New binary verification failed: {}", String::from_utf8_lossy(&output.stderr)))
            }
            Err(e) => {
                let _ = std::fs::remove_file(&staging_path);
                Err(format!("Failed to verify new binary: {e}"))
            }
        }
    }

    /// Called at startup (when not in probation) to check if a previous
    /// probation passed or if the agent was auto-rolled-back.
    pub async fn check_post_upgrade_status(&self) {
        let berth_dir = Self::berth_dir();

        let passed_file = berth_dir.join(".probation-passed");
        let rollback_count_file = berth_dir.join(".rollback-count");

        // Probation passed in a previous run — emit verification event
        if passed_file.exists() {
            if let Ok(data) = std::fs::read_to_string(&passed_file) {
                tracing::info!("Post-upgrade verification: probation passed");
                let event_data = serde_json::json!({
                    "status": "verified",
                    "details": data,
                }).to_string();
                {
                    let store = self.store.lock().await;
                    let _ = store.insert_event("agent_upgrade_verified", None, None, &event_data);
                }
                nats_publisher::maybe_publish_event(&self.nats, "agent_upgrade_verified", None, None, &event_data).await;
            }
            let _ = std::fs::remove_file(&passed_file);
        }

        // Auto-rollback happened — emit rollback event so desktop knows
        if rollback_count_file.exists() {
            if let Ok(count_str) = std::fs::read_to_string(&rollback_count_file) {
                tracing::warn!("Agent was auto-rolled-back (count={})", count_str.trim());
                let event_data = serde_json::json!({
                    "status": "auto_rolled_back",
                    "rollback_count": count_str.trim(),
                    "version": env!("CARGO_PKG_VERSION"),
                }).to_string();
                {
                    let store = self.store.lock().await;
                    let _ = store.insert_event("agent_auto_rollback", None, None, &event_data);
                }
                nats_publisher::maybe_publish_event(&self.nats, "agent_auto_rollback", None, None, &event_data).await;
            }
            let _ = std::fs::remove_file(&rollback_count_file);
        }
    }

    pub async fn do_rollback(&self) -> Result<(bool, String, String), String> {
        let berth_dir = Self::berth_dir();
        let bin_dir = berth_dir.join("bin");
        let active_path = bin_dir.join("berth-agent");
        let backup_path = bin_dir.join("berth-agent.old");

        if !backup_path.exists() {
            return Err("No backup available — no previous version to rollback to".into());
        }

        // Verify the backup binary
        let verify = std::process::Command::new(&backup_path)
            .arg("--version")
            .output();

        match verify {
            Ok(output) if output.status.success() => {
                let restored_version = String::from_utf8_lossy(&output.stdout).trim().to_string();
                tracing::info!("Verified backup agent version: {restored_version}");

                // Swap: remove active, rename backup → active
                let _ = std::fs::remove_file(&active_path);
                std::fs::rename(&backup_path, &active_path)
                    .map_err(|e| format!("Failed to restore backup: {e}"))?;

                // Emit rollback event
                let event_data = serde_json::json!({
                    "from_version": env!("CARGO_PKG_VERSION"),
                    "restored_version": &restored_version,
                }).to_string();
                {
                    let store = self.store.lock().await;
                    let _ = store.insert_event("agent_rollback", None, None, &event_data);
                }
                nats_publisher::maybe_publish_event(&self.nats, "agent_rollback", None, None, &event_data).await;

                // Write .upgrading marker so ExecStopPost doesn't double-rollback
                let _ = std::fs::write(berth_dir.join(".upgrading"), "rollback");

                // Flush NATS before exiting
                if let Some(nats) = &self.nats {
                    let _ = nats.client().flush().await;
                }

                // Exit with code 42 — systemd restarts with restored binary
                tokio::spawn(async {
                    tokio::time::sleep(tokio::time::Duration::from_secs(1)).await;
                    tracing::info!("Exiting for rollback restart (exit code 42)");
                    std::process::exit(42);
                });

                Ok((true, restored_version, "Rollback successful, restarting...".into()))
            }
            Ok(output) => {
                Err(format!("Backup binary verification failed: {}", String::from_utf8_lossy(&output.stderr)))
            }
            Err(e) => {
                Err(format!("Failed to verify backup binary: {e}"))
            }
        }
    }
}

pub struct HealthInfo {
    pub version: String,
    pub status: String,
    pub uptime_seconds: u64,
    pub podman_version: String,
    pub container_ready: bool,
    pub os: String,
    pub arch: String,
    pub probation_status: String,
    pub tunnel_providers: Vec<String>,
}

pub struct StatusInfo {
    pub agent_id: String,
    pub status: String,
    pub cpu_usage: f64,
    pub memory_bytes: u64,
    pub projects: Vec<ProjectStatusInfo>,
}

pub struct ProjectStatusInfo {
    pub project_id: String,
    pub status: String,
    pub started_at: String,
    pub run_mode: String,
    pub restart_count: u32,
    pub supervisor_state: String,
    pub uptime_secs: u64,
}

fn parse_runtime(s: &str) -> Runtime {
    match s {
        "python" => Runtime::Python,
        "node" => Runtime::Node,
        "go" => Runtime::Go,
        "rust" => Runtime::Rust,
        "shell" => Runtime::Shell,
        _ => Runtime::Unknown,
    }
}

/// Simple deterministic jitter — returns a value in [0.0, 1.0) based on current time nanos.
fn rand_simple() -> f64 {
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .subsec_nanos();
    (nanos as f64) / 1_000_000_000.0
}

#[tonic::async_trait]
impl AgentService for PersistentAgentService {
    type DeployStream = ReceiverStream<Result<DeployResponse, Status>>;

    async fn deploy(
        &self,
        request: Request<DeployRequest>,
    ) -> Result<Response<Self::DeployStream>, Status> {
        let req = request.into_inner();
        let version = req.version;
        let project_id = req.project_id.clone();
        let runtime = req.runtime.clone();
        let entrypoint = req.entrypoint.clone();
        let deploy_dir = self.deploy_dir(&project_id, version)
            .map_err(|e| tonic::Status::invalid_argument(e))?;
        let use_container = self.podman_version.is_some() && !req.containerfile.is_empty();

        let (tx, rx) = tokio::sync::mpsc::channel(256);

        let _ = tx
            .send(Ok(DeployResponse {
                phase: "extracting".into(),
                text: format!("Extracting source to {}", deploy_dir.display()),
                timestamp: chrono::Utc::now().to_rfc3339(),
                image_tag: String::new(),
                version,
                is_final: false,
                success: false,
            }))
            .await;

        std::fs::create_dir_all(&deploy_dir)
            .map_err(|e| Status::internal(format!("Failed to create deploy dir: {e}")))?;

        archive::extract(&req.source_archive, &deploy_dir)
            .map_err(|e| Status::internal(format!("Failed to extract archive: {e}")))?;

        let store = self.store.clone();
        let deploy_dir_str = deploy_dir.to_string_lossy().to_string();

        if use_container {
            let containerfile = req.containerfile.clone();
            let tx_build = tx.clone();
            let deploy_dir_clone = deploy_dir.clone();
            let project_id_clone = project_id.clone();
            let store_clone = store.clone();
            let runtime_clone = runtime.clone();
            let entrypoint_clone = entrypoint.clone();
            let deploy_dir_str_clone = deploy_dir_str.clone();
            let nats_clone = self.nats.clone();

            tokio::spawn(async move {
                let _ = tx_build
                    .send(Ok(DeployResponse {
                        phase: "building".into(),
                        text: "Building container image...".into(),
                        timestamp: chrono::Utc::now().to_rfc3339(),
                        image_tag: String::new(),
                        version,
                        is_final: false,
                        success: false,
                    }))
                    .await;

                let (build_tx, mut build_rx) = tokio::sync::mpsc::channel(256);

                let build_task = tokio::spawn({
                    let project_id = project_id_clone.clone();
                    let deploy_dir = deploy_dir_clone.clone();
                    async move {
                        container::build_image(&project_id, version, &containerfile, &deploy_dir, build_tx)
                            .await
                    }
                });

                while let Some(line) = build_rx.recv().await {
                    let _ = tx_build
                        .send(Ok(DeployResponse {
                            phase: "building".into(),
                            text: line.text,
                            timestamp: line.timestamp.to_rfc3339(),
                            image_tag: String::new(),
                            version,
                            is_final: false,
                            success: false,
                        }))
                        .await;
                }

                match build_task.await {
                    Ok(Ok(image_tag)) => {
                        // Persist deployment
                        {
                            let s = store_clone.lock().await;
                            let _ = s.insert_deployment(&Deployment {
                                id: Uuid::new_v4().to_string(),
                                project_id: project_id_clone.clone(),
                                version,
                                runtime: runtime_clone,
                                entrypoint: entrypoint_clone,
                                working_dir: deploy_dir_str_clone,
                                image_tag: Some(image_tag.clone()),
                                status: "deployed".into(),
                                deployed_at: chrono::Utc::now(),
                            });
                            let data = serde_json::json!({"version": version, "image_tag": &image_tag, "success": true}).to_string();
                            let _ = s.insert_event(
                                "deploy_completed",
                                Some(&project_id_clone),
                                None,
                                &data,
                            );
                            let _ = s.prune_old_deployments(&project_id_clone, 5);
                        }
                        nats_publisher::maybe_publish_event(&nats_clone, "deploy_completed", Some(&project_id_clone), None, &serde_json::json!({"version": version, "image_tag": &image_tag, "success": true}).to_string()).await;

                        let _ = tx_build
                            .send(Ok(DeployResponse {
                                phase: "ready".into(),
                                text: format!("Image built: {image_tag}"),
                                timestamp: chrono::Utc::now().to_rfc3339(),
                                image_tag,
                                version,
                                is_final: true,
                                success: true,
                            }))
                            .await;
                    }
                    Ok(Err(e)) => {
                        let _ = tx_build
                            .send(Ok(DeployResponse {
                                phase: "error".into(),
                                text: format!("Build failed: {e}"),
                                timestamp: chrono::Utc::now().to_rfc3339(),
                                image_tag: String::new(),
                                version,
                                is_final: true,
                                success: false,
                            }))
                            .await;
                    }
                    Err(e) => {
                        let _ = tx_build
                            .send(Ok(DeployResponse {
                                phase: "error".into(),
                                text: format!("Build task panicked: {e}"),
                                timestamp: chrono::Utc::now().to_rfc3339(),
                                image_tag: String::new(),
                                version,
                                is_final: true,
                                success: false,
                            }))
                            .await;
                    }
                }
            });
        } else {
            // Bare process setup path
            let setup_commands = req.setup_commands.clone();
            let deploy_dir_clone = deploy_dir.clone();
            let store_clone = store.clone();
            let project_id_clone = project_id.clone();
            let runtime_clone = runtime.clone();
            let entrypoint_clone = entrypoint.clone();
            let deploy_dir_str_clone = deploy_dir_str.clone();
            let nats_clone = self.nats.clone();

            tokio::spawn(async move {
                if setup_commands.is_empty() {
                    // Persist deployment even with no setup
                    {
                        let s = store_clone.lock().await;
                        let _ = s.insert_deployment(&Deployment {
                            id: Uuid::new_v4().to_string(),
                            project_id: project_id_clone.clone(),
                            version,
                            runtime: runtime_clone,
                            entrypoint: entrypoint_clone,
                            working_dir: deploy_dir_str_clone,
                            image_tag: None,
                            status: "deployed".into(),
                            deployed_at: chrono::Utc::now(),
                        });
                        let data = serde_json::json!({"version": version, "success": true}).to_string();
                        let _ = s.insert_event(
                            "deploy_completed",
                            Some(&project_id_clone),
                            None,
                            &data,
                        );
                        let _ = s.prune_old_deployments(&project_id_clone, 5);
                    }
                    nats_publisher::maybe_publish_event(&nats_clone, "deploy_completed", Some(&project_id_clone), None, &serde_json::json!({"version": version, "success": true}).to_string()).await;

                    let _ = tx
                        .send(Ok(DeployResponse {
                            phase: "ready".into(),
                            text: "No setup needed".into(),
                            timestamp: chrono::Utc::now().to_rfc3339(),
                            image_tag: String::new(),
                            version,
                            is_final: true,
                            success: true,
                        }))
                        .await;
                    return;
                }

                let _ = tx
                    .send(Ok(DeployResponse {
                        phase: "setup".into(),
                        text: "Running setup commands...".into(),
                        timestamp: chrono::Utc::now().to_rfc3339(),
                        image_tag: String::new(),
                        version,
                        is_final: false,
                        success: false,
                    }))
                    .await;

                let (setup_tx, mut setup_rx) = tokio::sync::mpsc::channel(256);

                let setup_task = tokio::spawn({
                    let dir = deploy_dir_clone.clone();
                    let cmds = setup_commands.clone();
                    async move { setup::run_setup_commands(&dir, &cmds, setup_tx).await }
                });

                while let Some(line) = setup_rx.recv().await {
                    let _ = tx
                        .send(Ok(DeployResponse {
                            phase: "setup".into(),
                            text: line.text,
                            timestamp: line.timestamp.to_rfc3339(),
                            image_tag: String::new(),
                            version,
                            is_final: false,
                            success: false,
                        }))
                        .await;
                }

                match setup_task.await {
                    Ok(Ok(())) => {
                        // Persist deployment
                        {
                            let s = store_clone.lock().await;
                            let _ = s.insert_deployment(&Deployment {
                                id: Uuid::new_v4().to_string(),
                                project_id: project_id_clone.clone(),
                                version,
                                runtime: runtime_clone,
                                entrypoint: entrypoint_clone,
                                working_dir: deploy_dir_str_clone,
                                image_tag: None,
                                status: "deployed".into(),
                                deployed_at: chrono::Utc::now(),
                            });
                            let data = serde_json::json!({"version": version, "success": true}).to_string();
                            let _ = s.insert_event(
                                "deploy_completed",
                                Some(&project_id_clone),
                                None,
                                &data,
                            );
                            let _ = s.prune_old_deployments(&project_id_clone, 5);
                        }
                        nats_publisher::maybe_publish_event(&nats_clone, "deploy_completed", Some(&project_id_clone), None, &serde_json::json!({"version": version, "success": true}).to_string()).await;

                        let _ = tx
                            .send(Ok(DeployResponse {
                                phase: "ready".into(),
                                text: "Setup complete".into(),
                                timestamp: chrono::Utc::now().to_rfc3339(),
                                image_tag: String::new(),
                                version,
                                is_final: true,
                                success: true,
                            }))
                            .await;
                    }
                    Ok(Err(e)) => {
                        let _ = tx
                            .send(Ok(DeployResponse {
                                phase: "error".into(),
                                text: format!("Setup failed: {e}"),
                                timestamp: chrono::Utc::now().to_rfc3339(),
                                image_tag: String::new(),
                                version,
                                is_final: true,
                                success: false,
                            }))
                            .await;
                    }
                    Err(e) => {
                        let _ = tx
                            .send(Ok(DeployResponse {
                                phase: "error".into(),
                                text: format!("Setup task panicked: {e}"),
                                timestamp: chrono::Utc::now().to_rfc3339(),
                                image_tag: String::new(),
                                version,
                                is_final: true,
                                success: false,
                            }))
                            .await;
                    }
                }
            });
        }

        Ok(Response::new(ReceiverStream::new(rx)))
    }

    type ExecuteStream = ReceiverStream<Result<ExecuteResponse, Status>>;

    async fn execute(
        &self,
        request: Request<ExecuteRequest>,
    ) -> Result<Response<Self::ExecuteStream>, Status> {
        let req = request.into_inner();
        let code = if req.code.is_empty() { None } else { Some(req.code.as_slice()) };
        let image_tag = if req.image_tag.is_empty() { None } else { Some(req.image_tag.as_str()) };
        let container_name = if req.container_name.is_empty() { None } else { Some(req.container_name.as_str()) };
        let run_mode = if req.run_mode.is_empty() { "oneshot" } else { &req.run_mode };

        let mut rx = self.do_execute(
            &req.project_id,
            &req.runtime,
            &req.entrypoint,
            &req.working_dir,
            code,
            image_tag,
            req.env_vars,
            container_name,
            run_mode,
            req.service_port as u16,
        ).await.map_err(|e| Status::internal(e))?;

        let (tx, stream_rx) = tokio::sync::mpsc::channel(256);
        tokio::spawn(async move {
            while let Some(line) = rx.recv().await {
                let resp = ExecuteResponse {
                    stream: line.stream,
                    text: line.text,
                    timestamp: line.timestamp,
                    exit_code: line.exit_code,
                    is_final: line.is_final,
                };
                if tx.send(Ok(resp)).await.is_err() {
                    break;
                }
            }
        });

        Ok(Response::new(ReceiverStream::new(stream_rx)))
    }

    async fn status(
        &self,
        _request: Request<StatusRequest>,
    ) -> Result<Response<StatusResponse>, Status> {
        let info = self.do_status().await;
        let projects: Vec<ProjectStatus> = info.projects
            .into_iter()
            .map(|p| ProjectStatus {
                project_id: p.project_id,
                status: p.status,
                pid: 0,
                started_at: p.started_at,
                run_mode: p.run_mode,
                restart_count: p.restart_count,
                supervisor_state: p.supervisor_state,
                uptime_secs: p.uptime_secs as u64,
            })
            .collect();

        Ok(Response::new(StatusResponse {
            agent_id: info.agent_id,
            status: info.status,
            cpu_usage: info.cpu_usage,
            memory_bytes: info.memory_bytes,
            projects,
        }))
    }

    type StreamLogsStream = ReceiverStream<Result<LogStreamResponse, Status>>;

    async fn stream_logs(
        &self,
        _request: Request<LogStreamRequest>,
    ) -> Result<Response<Self::StreamLogsStream>, Status> {
        Err(Status::unimplemented(
            "Use GetExecutionLogs for stored logs or Execute stream for live logs",
        ))
    }

    async fn stop(
        &self,
        request: Request<StopRequest>,
    ) -> Result<Response<StopResponse>, Status> {
        let project_id = request.into_inner().project_id;
        let (success, message) = self.do_stop(&project_id).await;
        Ok(Response::new(StopResponse { success, message }))
    }

    async fn health(
        &self,
        _request: Request<HealthRequest>,
    ) -> Result<Response<HealthResponse>, Status> {
        let h = self.do_health();
        Ok(Response::new(HealthResponse {
            agent_version: h.version,
            status: h.status,
            uptime_seconds: h.uptime_seconds,
            podman_version: h.podman_version,
            container_ready: h.container_ready,
            os: h.os,
            arch: h.arch,
            probation_status: h.probation_status,
            tunnel_providers: h.tunnel_providers,
        }))
    }

    // --- Tunnel RPCs ---

    async fn publish(
        &self,
        request: Request<PublishRequest>,
    ) -> Result<Response<PublishResponse>, Status> {
        let req = request.into_inner();
        match self
            .do_publish(&req.project_id, req.port as u16, &req.provider, &req.provider_config)
            .await
        {
            Ok((success, url, provider, message)) => Ok(Response::new(PublishResponse {
                success,
                url,
                message,
                provider,
            })),
            Err(e) => Ok(Response::new(PublishResponse {
                success: false,
                url: String::new(),
                message: e,
                provider: String::new(),
            })),
        }
    }

    async fn unpublish(
        &self,
        request: Request<UnpublishRequest>,
    ) -> Result<Response<UnpublishResponse>, Status> {
        let req = request.into_inner();
        match self.do_unpublish(&req.project_id).await {
            Ok((success, message)) => Ok(Response::new(UnpublishResponse { success, message })),
            Err(e) => Ok(Response::new(UnpublishResponse {
                success: false,
                message: e,
            })),
        }
    }

    // --- New persistent RPCs ---

    async fn get_executions(
        &self,
        request: Request<GetExecutionsRequest>,
    ) -> Result<Response<GetExecutionsResponse>, Status> {
        let req = request.into_inner();
        let store = self.store.lock().await;
        let executions = store
            .list_executions(&req.project_id, req.limit)
            .map_err(|e| Status::internal(format!("Failed to list executions: {e}")))?;

        let infos = executions
            .into_iter()
            .map(|e| ExecutionInfo {
                id: e.id,
                project_id: e.project_id,
                deployment_id: e.deployment_id.unwrap_or_default(),
                started_at: e.started_at.to_rfc3339(),
                finished_at: e.finished_at.map(|t| t.to_rfc3339()).unwrap_or_default(),
                exit_code: e.exit_code.unwrap_or(0),
                trigger: e.trigger,
                status: e.status,
            })
            .collect();

        Ok(Response::new(GetExecutionsResponse { executions: infos }))
    }

    type GetExecutionLogsStream = ReceiverStream<Result<LogStreamResponse, Status>>;

    async fn get_execution_logs(
        &self,
        request: Request<GetExecutionLogsRequest>,
    ) -> Result<Response<Self::GetExecutionLogsStream>, Status> {
        let req = request.into_inner();
        let store = self.store.lock().await;
        let logs = store
            .get_logs(&req.execution_id, req.since_seq)
            .map_err(|e| Status::internal(format!("Failed to get logs: {e}")))?;

        let (tx, rx) = tokio::sync::mpsc::channel(256);

        tokio::spawn(async move {
            for log in logs {
                let resp = LogStreamResponse {
                    stream: log.stream,
                    text: log.text,
                    timestamp: log.timestamp.to_rfc3339(),
                };
                if tx.send(Ok(resp)).await.is_err() {
                    break;
                }
            }
        });

        Ok(Response::new(ReceiverStream::new(rx)))
    }

    async fn get_events(
        &self,
        request: Request<GetEventsRequest>,
    ) -> Result<Response<GetEventsResponse>, Status> {
        let req = request.into_inner();
        let store = self.store.lock().await;
        let events = store
            .get_events_since(req.since_id, req.limit)
            .map_err(|e| Status::internal(format!("Failed to get events: {e}")))?;

        let proto_events = events
            .into_iter()
            .map(|e| berth_proto::proto::AgentEvent {
                id: e.id,
                event_type: e.event_type,
                project_id: e.project_id.unwrap_or_default(),
                execution_id: e.execution_id.unwrap_or_default(),
                data: e.data,
                created_at: e.created_at.to_rfc3339(),
            })
            .collect();

        Ok(Response::new(GetEventsResponse {
            events: proto_events,
        }))
    }

    async fn ack_events(
        &self,
        request: Request<AckEventsRequest>,
    ) -> Result<Response<AckEventsResponse>, Status> {
        let req = request.into_inner();
        let store = self.store.lock().await;
        let pruned = store
            .prune_events(req.up_to_id)
            .map_err(|e| Status::internal(format!("Failed to prune events: {e}")))?;

        // Hard cap at 10,000 events
        let _ = store.prune_events_hard_cap(10_000);

        Ok(Response::new(AckEventsResponse {
            pruned_count: pruned,
        }))
    }

    async fn add_schedule(
        &self,
        request: Request<AddScheduleRequest>,
    ) -> Result<Response<AddScheduleResponse>, Status> {
        let req = request.into_inner();
        let now = chrono::Utc::now();
        let next = berth_proto::schedule::parse_next_run(&req.cron_expr, now);

        if next.is_none() {
            return Err(Status::invalid_argument(format!(
                "Invalid cron expression: {}",
                req.cron_expr
            )));
        }

        let id = Uuid::new_v4().to_string();
        let schedule = crate::agent_store::AgentSchedule {
            id: id.clone(),
            project_id: req.project_id,
            cron_expr: req.cron_expr,
            enabled: true,
            created_at: now,
            last_triggered_at: None,
            next_run_at: next,
        };

        let store = self.store.lock().await;
        store
            .insert_schedule(&schedule)
            .map_err(|e| Status::internal(format!("Failed to add schedule: {e}")))?;

        Ok(Response::new(AddScheduleResponse {
            schedule_id: id,
            next_run_at: next.map(|t: chrono::DateTime<chrono::Utc>| t.to_rfc3339()).unwrap_or_default(),
        }))
    }

    async fn remove_schedule(
        &self,
        request: Request<RemoveScheduleRequest>,
    ) -> Result<Response<RemoveScheduleResponse>, Status> {
        let req = request.into_inner();
        let store = self.store.lock().await;
        let success = store
            .delete_schedule(&req.schedule_id)
            .map_err(|e| Status::internal(format!("Failed to remove schedule: {e}")))?;

        Ok(Response::new(RemoveScheduleResponse { success }))
    }

    async fn list_schedules(
        &self,
        request: Request<ListSchedulesRequest>,
    ) -> Result<Response<ListSchedulesResponse>, Status> {
        let req = request.into_inner();
        let store = self.store.lock().await;
        let schedules = store
            .list_schedules(&req.project_id)
            .map_err(|e| Status::internal(format!("Failed to list schedules: {e}")))?;

        let infos = schedules
            .into_iter()
            .map(|s| AgentScheduleInfo {
                id: s.id,
                project_id: s.project_id,
                cron_expr: s.cron_expr,
                enabled: s.enabled,
                created_at: s.created_at.to_rfc3339(),
                last_triggered_at: s
                    .last_triggered_at
                    .map(|t| t.to_rfc3339())
                    .unwrap_or_default(),
                next_run_at: s
                    .next_run_at
                    .map(|t| t.to_rfc3339())
                    .unwrap_or_default(),
            })
            .collect();

        Ok(Response::new(ListSchedulesResponse { schedules: infos }))
    }

    async fn upgrade(
        &self,
        request: Request<Streaming<UpgradeChunk>>,
    ) -> Result<Response<UpgradeResponse>, Status> {
        let mut stream = request.into_inner();

        // Receive binary chunks
        let mut data = Vec::new();
        while let Some(chunk) = stream
            .message()
            .await
            .map_err(|e| Status::internal(format!("Failed to receive upgrade chunk: {e}")))?
        {
            data.extend_from_slice(&chunk.data);
        }

        match self.do_upgrade_from_bytes(data).await {
            Ok((success, new_version, message)) => {
                Ok(Response::new(UpgradeResponse { success, new_version, message }))
            }
            Err(e) => Err(Status::internal(e)),
        }
    }

    async fn rollback(
        &self,
        _request: Request<RollbackRequest>,
    ) -> Result<Response<RollbackResponse>, Status> {
        match self.do_rollback().await {
            Ok((success, restored_version, message)) => {
                Ok(Response::new(RollbackResponse { success, restored_version, message }))
            }
            Err(e) => Err(Status::internal(e)),
        }
    }
}

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;

use sysinfo::System;
use tokio::sync::Mutex;
use tokio_stream::wrappers::ReceiverStream;
use tonic::{Request, Response, Status, Streaming};
use uuid::Uuid;

use berth_core::agent_client::proto::agent_service_server::AgentService;
use berth_core::agent_client::proto::*;
use berth_core::agent_client::{RemoteExecution, RemoteSchedule};
use berth_core::agent_transport::{ExecuteResponseLine, DeployResponseLine};
use berth_core::executor::{self, LogLine, LogStream};
use berth_core::runtime::Runtime;
use berth_core::{archive, container, setup};

use crate::agent_store::{AgentStore, Deployment, Execution};
use crate::nats_publisher::{self, NatsPublisher};

fn gethostname() -> String {
    System::host_name().unwrap_or_else(|| "unknown".into())
}

enum ProcessKind {
    BareProcess {
        abort_handle: tokio::task::AbortHandle,
    },
    Container {
        container_name: String,
        abort_handle: tokio::task::AbortHandle,
    },
}

struct RunningChild {
    kind: ProcessKind,
    started_at: chrono::DateTime<chrono::Utc>,
}

pub struct PersistentAgentService {
    store: Arc<Mutex<AgentStore>>,
    processes: Arc<Mutex<HashMap<String, RunningChild>>>,
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
            start_time: std::time::Instant::now(),
            deploys_dir,
            podman_version,
            nats,
        }
    }

    fn deploy_dir(&self, project_id: &str, version: u32) -> PathBuf {
        self.deploys_dir
            .join(project_id)
            .join(format!("v{version}"))
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
        }
    }

    pub async fn do_status(&self) -> StatusInfo {
        let procs = self.processes.lock().await;
        let projects: Vec<ProjectStatusInfo> = procs
            .iter()
            .map(|(id, child)| ProjectStatusInfo {
                project_id: id.clone(),
                status: "running".into(),
                started_at: child.started_at.to_rfc3339(),
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

        if let Some(child) = procs.remove(project_id) {
            match child.kind {
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

            (true, format!("Stopped project {project_id}"))
        } else {
            (false, format!("Project {project_id} is not running"))
        }
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
    ) -> Result<tokio::sync::mpsc::Receiver<ExecuteResponseLine>, String> {
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
                    RunningChild {
                        kind: ProcessKind::Container {
                            container_name: container_name_clone,
                            abort_handle: task.abort_handle(),
                        },
                        started_at: chrono::Utc::now(),
                    },
                );
            }
        } else {
            let (actual_working_dir, actual_entrypoint) = if let Some(code_bytes) = code {
                let tmp = std::env::temp_dir().join(format!("berth-agent-{}", Uuid::new_v4()));
                std::fs::create_dir_all(&tmp).map_err(|e| format!("Failed to create temp dir: {e}"))?;
                let ep = if entrypoint.is_empty() { "main.py" } else { entrypoint };
                std::fs::write(tmp.join(ep), code_bytes)
                    .map_err(|e| format!("Failed to write code: {e}"))?;
                (tmp.to_string_lossy().to_string(), ep.to_string())
            } else {
                (working_dir.to_string(), entrypoint.to_string())
            };

            let env_ref = if env_vars.is_empty() { None } else { Some(&env_vars) };

            let (mut child, mut child_rx) =
                executor::spawn_and_stream(runtime, &actual_entrypoint, &actual_working_dir, env_ref)
                    .await
                    .map_err(|e| format!("Failed to spawn: {e}"))?;

            let exec_id = execution_id.clone();
            let pid = project_id.clone();
            let nats_clone = nats.clone();
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
                    RunningChild {
                        kind: ProcessKind::BareProcess {
                            abort_handle: task.abort_handle(),
                        },
                        started_at: chrono::Utc::now(),
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
        let deploy_dir = self.deploy_dir(project_id, version);
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
        let next = berth_core::scheduler::parse_next_run(cron_expr, now);
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
        Ok((id, next.map(|t| t.to_rfc3339()).unwrap_or_default()))
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
        let deploy_dir = self.deploy_dir(&project_id, version);
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
        let runtime = parse_runtime(&req.runtime);
        let project_id = req.project_id.clone();
        let env_vars: HashMap<String, String> = req.env_vars.clone();
        let use_container = !req.image_tag.is_empty();

        // Create execution record
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

        let (tx, stream_rx) = tokio::sync::mpsc::channel(256);
        let processes = self.processes.clone();
        let store = self.store.clone();
        let nats = self.nats.clone();

        if use_container {
            let container_name = if req.container_name.is_empty() {
                format!("berth-{project_id}")
            } else {
                req.container_name.clone()
            };

            let (mut child, mut rx) =
                container::run_container(&req.image_tag, &container_name, &env_vars)
                    .await
                    .map_err(|e| Status::internal(format!("Failed to run container: {e}")))?;

            let container_name_clone = container_name.clone();
            let exec_id = execution_id.clone();
            let nats_clone = nats.clone();
            let task = tokio::spawn(async move {
                let mut seq: i64 = 0;
                while let Some(line) = rx.recv().await {
                    let stream_str = match line.stream {
                        LogStream::Stdout => "stdout",
                        LogStream::Stderr => "stderr",
                    };

                    // Persist log line
                    {
                        let s = store.lock().await;
                        let _ = s.append_log_line(&exec_id, seq, stream_str, &line.text, &line.timestamp);
                    }
                    nats_publisher::maybe_publish_log_line(&nats_clone, &project_id, &exec_id, stream_str, &line.text, seq).await;

                    let resp = ExecuteResponse {
                        stream: stream_str.into(),
                        text: line.text,
                        timestamp: line.timestamp.to_rfc3339(),
                        exit_code: 0,
                        is_final: false,
                    };
                    if tx.send(Ok(resp)).await.is_err() {
                        break;
                    }
                    seq += 1;
                }

                let exit_code = match child.wait().await {
                    Ok(status) => status.code().unwrap_or(-1),
                    Err(_) => -1,
                };

                // Finalize execution
                let status_str = if exit_code == 0 { "completed" } else { "failed" };
                let data = serde_json::json!({"exit_code": exit_code, "status": status_str}).to_string();
                {
                    let s = store.lock().await;
                    let _ = s.finish_execution(&exec_id, exit_code, status_str);
                    let _ = s.insert_event(
                        "execution_completed",
                        Some(&project_id),
                        Some(&exec_id),
                        &data,
                    );
                }
                nats_publisher::maybe_publish_event(&nats_clone, "execution_completed", Some(&project_id), Some(&exec_id), &data).await;

                let final_resp = ExecuteResponse {
                    stream: String::new(),
                    text: String::new(),
                    timestamp: chrono::Utc::now().to_rfc3339(),
                    exit_code,
                    is_final: true,
                };
                let _ = tx.send(Ok(final_resp)).await;

                let mut procs = processes.lock().await;
                procs.remove(&project_id);
            });

            {
                let mut procs = self.processes.lock().await;
                procs.insert(
                    req.project_id,
                    RunningChild {
                        kind: ProcessKind::Container {
                            container_name: container_name_clone,
                            abort_handle: task.abort_handle(),
                        },
                        started_at: chrono::Utc::now(),
                    },
                );
            }
        } else {
            // Bare process execution
            let (working_dir, entrypoint) = if !req.code.is_empty() {
                let tmp = std::env::temp_dir().join(format!("berth-agent-{}", Uuid::new_v4()));
                std::fs::create_dir_all(&tmp)
                    .map_err(|e| Status::internal(format!("Failed to create temp dir: {e}")))?;
                let ep = if req.entrypoint.is_empty() {
                    "main.py"
                } else {
                    &req.entrypoint
                };
                std::fs::write(tmp.join(ep), &req.code)
                    .map_err(|e| Status::internal(format!("Failed to write code: {e}")))?;
                (tmp.to_string_lossy().to_string(), ep.to_string())
            } else {
                (req.working_dir.clone(), req.entrypoint.clone())
            };

            let env_ref = if env_vars.is_empty() {
                None
            } else {
                Some(&env_vars)
            };

            let (mut child, mut rx) =
                executor::spawn_and_stream(runtime, &entrypoint, &working_dir, env_ref)
                    .await
                    .map_err(|e| Status::internal(format!("Failed to spawn: {e}")))?;

            let exec_id = execution_id.clone();
            let nats_clone = nats.clone();
            let task = tokio::spawn(async move {
                let mut seq: i64 = 0;
                while let Some(line) = rx.recv().await {
                    let stream_str = match line.stream {
                        LogStream::Stdout => "stdout",
                        LogStream::Stderr => "stderr",
                    };

                    // Persist log line
                    {
                        let s = store.lock().await;
                        let _ = s.append_log_line(&exec_id, seq, stream_str, &line.text, &line.timestamp);
                    }
                    nats_publisher::maybe_publish_log_line(&nats_clone, &project_id, &exec_id, stream_str, &line.text, seq).await;

                    let resp = ExecuteResponse {
                        stream: stream_str.into(),
                        text: line.text,
                        timestamp: line.timestamp.to_rfc3339(),
                        exit_code: 0,
                        is_final: false,
                    };
                    if tx.send(Ok(resp)).await.is_err() {
                        break;
                    }
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
                    let _ = s.insert_event(
                        "execution_completed",
                        Some(&project_id),
                        Some(&exec_id),
                        &data,
                    );
                }
                nats_publisher::maybe_publish_event(&nats_clone, "execution_completed", Some(&project_id), Some(&exec_id), &data).await;

                let final_resp = ExecuteResponse {
                    stream: String::new(),
                    text: String::new(),
                    timestamp: chrono::Utc::now().to_rfc3339(),
                    exit_code,
                    is_final: true,
                };
                let _ = tx.send(Ok(final_resp)).await;

                let mut procs = processes.lock().await;
                procs.remove(&project_id);
            });

            {
                let mut procs = self.processes.lock().await;
                procs.insert(
                    req.project_id,
                    RunningChild {
                        kind: ProcessKind::BareProcess {
                            abort_handle: task.abort_handle(),
                        },
                        started_at: chrono::Utc::now(),
                    },
                );
            }
        }

        Ok(Response::new(ReceiverStream::new(stream_rx)))
    }

    async fn status(
        &self,
        _request: Request<StatusRequest>,
    ) -> Result<Response<StatusResponse>, Status> {
        let procs = self.processes.lock().await;
        let projects: Vec<ProjectStatus> = procs
            .iter()
            .map(|(id, child)| ProjectStatus {
                project_id: id.clone(),
                status: "running".into(),
                pid: 0,
                started_at: child.started_at.to_rfc3339(),
            })
            .collect();

        let mut sys = sysinfo::System::new();
        sys.refresh_cpu_usage();
        sys.refresh_memory();
        let cpu_usage = sys.global_cpu_usage() as f64;
        let memory_bytes = sys.used_memory();

        Ok(Response::new(StatusResponse {
            agent_id: gethostname(),
            status: "running".into(),
            cpu_usage,
            memory_bytes,
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
        let mut procs = self.processes.lock().await;

        if let Some(child) = procs.remove(&project_id) {
            match child.kind {
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

            // Emit stop event
            let data = serde_json::json!({"project_id": &project_id}).to_string();
            {
                let store = self.store.lock().await;
                let _ = store.insert_event(
                    "execution_stopped",
                    Some(&project_id),
                    None,
                    &data,
                );
            }
            nats_publisher::maybe_publish_event(&self.nats, "execution_stopped", Some(&project_id), None, &data).await;

            Ok(Response::new(StopResponse {
                success: true,
                message: format!("Stopped project {project_id}"),
            }))
        } else {
            Ok(Response::new(StopResponse {
                success: false,
                message: format!("Project {project_id} is not running"),
            }))
        }
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
        }))
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
            .map(|e| berth_core::agent_client::proto::AgentEvent {
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
        let next = berth_core::scheduler::parse_next_run(&req.cron_expr, now);

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
            next_run_at: next.map(|t| t.to_rfc3339()).unwrap_or_default(),
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

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};

use sysinfo::System;
use tokio::sync::Mutex;
use tokio_stream::wrappers::ReceiverStream;
use tonic::{Request, Response, Status};
use uuid::Uuid;

use berth_proto::proto::agent_service_server::AgentService;
use berth_proto::proto::*;
use crate::executor::{self, LogStream};
use berth_proto::runtime::parse_runtime;
use crate::tunnel::{TunnelManager, TunnelProvider};
use crate::{archive, container, setup};

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

struct ManagedProcess {
    kind: ProcessKind,
    started_at: chrono::DateTime<chrono::Utc>,
    cancelled: Arc<AtomicBool>,
    run_mode: String,
    restart_count: Arc<AtomicU32>,
    supervisor_state: Arc<std::sync::Mutex<String>>,
}

pub struct AgentServiceImpl {
    processes: Arc<Mutex<HashMap<String, ManagedProcess>>>,
    tunnel_mgr: Arc<TunnelManager>,
    start_time: std::time::Instant,
    deploys_dir: PathBuf,
    podman_version: Option<String>,
}

impl AgentServiceImpl {
    pub fn new() -> Self {
        let deploys_dir = dirs_next::home_dir()
            .unwrap_or_else(|| PathBuf::from("/tmp"))
            .join(".berth/deploys");

        // Check podman availability at startup (sync, one-time)
        let podman_version = std::process::Command::new("podman")
            .args(["version", "--format", "{{.Client.Version}}"])
            .output()
            .ok()
            .filter(|o| o.status.success())
            .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string());

        Self {
            processes: Arc::new(Mutex::new(HashMap::new())),
            tunnel_mgr: Arc::new(TunnelManager::new()),
            start_time: std::time::Instant::now(),
            deploys_dir,
            podman_version,
        }
    }

    fn deploy_dir(&self, project_id: &str, version: u32) -> PathBuf {
        self.deploys_dir
            .join(project_id)
            .join(format!("v{version}"))
    }
}

#[tonic::async_trait]
impl AgentService for AgentServiceImpl {
    type DeployStream = ReceiverStream<Result<DeployResponse, Status>>;

    async fn deploy(
        &self,
        request: Request<DeployRequest>,
    ) -> Result<Response<Self::DeployStream>, Status> {
        let req = request.into_inner();
        let version = req.version;
        let project_id = req.project_id.clone();
        let deploy_dir = self.deploy_dir(&project_id, version);
        let use_container = self.podman_version.is_some() && !req.containerfile.is_empty();

        let (tx, rx) = tokio::sync::mpsc::channel(256);

        // Phase: extracting
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

        if use_container {
            // Container build path
            let containerfile = req.containerfile.clone();
            let tx_build = tx.clone();
            let deploy_dir_clone = deploy_dir.clone();
            let project_id_clone = project_id.clone();

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
                        container::build_image(
                            &project_id,
                            version,
                            &containerfile,
                            &deploy_dir,
                            build_tx,
                        )
                        .await
                    }
                });

                // Stream build output
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

            tokio::spawn(async move {
                if setup_commands.is_empty() {
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

        let (tx, stream_rx) = tokio::sync::mpsc::channel(256);
        let processes = self.processes.clone();

        if use_container {
            // Container execution path
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
            let task = tokio::spawn(async move {
                while let Some(line) = rx.recv().await {
                    let resp = ExecuteResponse {
                        stream: match line.stream {
                            LogStream::Stdout => "stdout".into(),
                            LogStream::Stderr => "stderr".into(),
                        },
                        text: line.text,
                        timestamp: line.timestamp.to_rfc3339(),
                        exit_code: 0,
                        is_final: false,
                    };
                    if tx.send(Ok(resp)).await.is_err() {
                        break;
                    }
                }

                let exit_code = match child.wait().await {
                    Ok(status) => status.code().unwrap_or(-1),
                    Err(_) => -1,
                };

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
                    ManagedProcess {
                        kind: ProcessKind::Container {
                            container_name: container_name_clone,
                            abort_handle: task.abort_handle(),
                        },
                        started_at: chrono::Utc::now(),
                        cancelled: Arc::new(AtomicBool::new(false)),
                        run_mode: "oneshot".to_string(),
                        restart_count: Arc::new(AtomicU32::new(0)),
                        supervisor_state: Arc::new(std::sync::Mutex::new("running".to_string())),
                    },
                );
            }
        } else {
            // Bare process execution path
            let (working_dir, entrypoint) = if !req.code.is_empty() {
                // Legacy inline code path
                let tmp =
                    std::env::temp_dir().join(format!("berth-agent-{}", Uuid::new_v4()));
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

            let task = tokio::spawn(async move {
                while let Some(line) = rx.recv().await {
                    let resp = ExecuteResponse {
                        stream: match line.stream {
                            LogStream::Stdout => "stdout".into(),
                            LogStream::Stderr => "stderr".into(),
                        },
                        text: line.text,
                        timestamp: line.timestamp.to_rfc3339(),
                        exit_code: 0,
                        is_final: false,
                    };
                    if tx.send(Ok(resp)).await.is_err() {
                        break;
                    }
                }

                let exit_code = match child.wait().await {
                    Ok(status) => status.code().unwrap_or(-1),
                    Err(_) => -1,
                };

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
                    ManagedProcess {
                        kind: ProcessKind::BareProcess {
                            abort_handle: task.abort_handle(),
                        },
                        started_at: chrono::Utc::now(),
                        cancelled: Arc::new(AtomicBool::new(false)),
                        run_mode: "oneshot".to_string(),
                        restart_count: Arc::new(AtomicU32::new(0)),
                        supervisor_state: Arc::new(std::sync::Mutex::new("running".to_string())),
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
            .map(|(id, managed)| {
                let supervisor_state = managed.supervisor_state.lock()
                    .map(|s| s.clone())
                    .unwrap_or_else(|_| "running".into());
                let uptime_secs = (chrono::Utc::now() - managed.started_at).num_seconds().max(0) as u64;
                ProjectStatus {
                    project_id: id.clone(),
                    status: "running".into(),
                    pid: 0,
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
            "Log streaming via gRPC not yet implemented — use Execute stream instead",
        ))
    }

    async fn stop(
        &self,
        request: Request<StopRequest>,
    ) -> Result<Response<StopResponse>, Status> {
        let project_id = request.into_inner().project_id;
        let mut procs = self.processes.lock().await;

        if let Some(managed) = procs.remove(&project_id) {
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
        Ok(Response::new(HealthResponse {
            agent_version: env!("CARGO_PKG_VERSION").into(),
            status: "healthy".into(),
            uptime_seconds: self.start_time.elapsed().as_secs(),
            podman_version: self.podman_version.clone().unwrap_or_default(),
            container_ready: self.podman_version.is_some(),
            os: std::env::consts::OS.into(),
            arch: std::env::consts::ARCH.into(),
            probation_status: String::new(),
            tunnel_providers: TunnelManager::available_providers(),
        }))
    }

    // --- Stub implementations for persistent agent RPCs ---
    // These are only implemented on the remote PersistentAgentService.

    async fn get_executions(
        &self,
        _request: Request<GetExecutionsRequest>,
    ) -> Result<Response<GetExecutionsResponse>, Status> {
        Err(Status::unimplemented(
            "GetExecutions is only available on remote persistent agents",
        ))
    }

    type GetExecutionLogsStream = ReceiverStream<Result<LogStreamResponse, Status>>;

    async fn get_execution_logs(
        &self,
        _request: Request<GetExecutionLogsRequest>,
    ) -> Result<Response<Self::GetExecutionLogsStream>, Status> {
        Err(Status::unimplemented(
            "GetExecutionLogs is only available on remote persistent agents",
        ))
    }

    async fn get_events(
        &self,
        _request: Request<GetEventsRequest>,
    ) -> Result<Response<GetEventsResponse>, Status> {
        Err(Status::unimplemented(
            "GetEvents is only available on remote persistent agents",
        ))
    }

    async fn ack_events(
        &self,
        _request: Request<AckEventsRequest>,
    ) -> Result<Response<AckEventsResponse>, Status> {
        Err(Status::unimplemented(
            "AckEvents is only available on remote persistent agents",
        ))
    }

    async fn add_schedule(
        &self,
        _request: Request<AddScheduleRequest>,
    ) -> Result<Response<AddScheduleResponse>, Status> {
        Err(Status::unimplemented(
            "AddSchedule is only available on remote persistent agents",
        ))
    }

    async fn remove_schedule(
        &self,
        _request: Request<RemoveScheduleRequest>,
    ) -> Result<Response<RemoveScheduleResponse>, Status> {
        Err(Status::unimplemented(
            "RemoveSchedule is only available on remote persistent agents",
        ))
    }

    async fn list_schedules(
        &self,
        _request: Request<ListSchedulesRequest>,
    ) -> Result<Response<ListSchedulesResponse>, Status> {
        Err(Status::unimplemented(
            "ListSchedules is only available on remote persistent agents",
        ))
    }

    async fn upgrade(
        &self,
        _request: Request<tonic::Streaming<UpgradeChunk>>,
    ) -> Result<Response<UpgradeResponse>, Status> {
        Err(Status::unimplemented(
            "Upgrade is only available on remote persistent agents",
        ))
    }

    async fn rollback(
        &self,
        _request: Request<RollbackRequest>,
    ) -> Result<Response<RollbackResponse>, Status> {
        Err(Status::unimplemented(
            "Rollback is only available on remote persistent agents",
        ))
    }

    async fn publish(
        &self,
        request: Request<PublishRequest>,
    ) -> Result<Response<PublishResponse>, Status> {
        let req = request.into_inner();
        let provider = match req.provider.as_str() {
            "cloudflared" | "" => TunnelProvider::Cloudflared,
            other => {
                return Ok(Response::new(PublishResponse {
                    success: false,
                    url: String::new(),
                    message: format!("Unknown tunnel provider: {other}. Available: cloudflared"),
                    provider: String::new(),
                }));
            }
        };

        match self.tunnel_mgr.start(&req.project_id, req.port as u16, &provider).await {
            Ok(info) => Ok(Response::new(PublishResponse {
                success: true,
                url: info.public_url,
                provider: info.provider,
                message: "Tunnel started".into(),
            })),
            Err(e) => Ok(Response::new(PublishResponse {
                success: false,
                url: String::new(),
                message: e.to_string(),
                provider: String::new(),
            })),
        }
    }

    async fn unpublish(
        &self,
        request: Request<UnpublishRequest>,
    ) -> Result<Response<UnpublishResponse>, Status> {
        let req = request.into_inner();
        match self.tunnel_mgr.stop(&req.project_id).await {
            Ok(()) => Ok(Response::new(UnpublishResponse {
                success: true,
                message: "Tunnel stopped".into(),
            })),
            Err(e) => Ok(Response::new(UnpublishResponse {
                success: false,
                message: e.to_string(),
            })),
        }
    }
}

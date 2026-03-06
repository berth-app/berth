use std::collections::HashMap;
use std::sync::Arc;

use tokio::sync::Mutex;
use tokio_stream::wrappers::ReceiverStream;
use tonic::{Request, Response, Status};
use uuid::Uuid;

use runway_core::executor::{self, LogStream};
use runway_core::runtime::Runtime;

use crate::proto::agent_service_server::AgentService;
use crate::proto::*;

struct RunningChild {
    abort_handle: tokio::task::AbortHandle,
    started_at: chrono::DateTime<chrono::Utc>,
}

pub struct AgentServiceImpl {
    processes: Arc<Mutex<HashMap<String, RunningChild>>>,
    start_time: std::time::Instant,
}

impl AgentServiceImpl {
    pub fn new() -> Self {
        Self {
            processes: Arc::new(Mutex::new(HashMap::new())),
            start_time: std::time::Instant::now(),
        }
    }
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
impl AgentService for AgentServiceImpl {
    type ExecuteStream = ReceiverStream<Result<ExecuteResponse, Status>>;

    async fn execute(
        &self,
        request: Request<ExecuteRequest>,
    ) -> Result<Response<Self::ExecuteStream>, Status> {
        let req = request.into_inner();
        let runtime = parse_runtime(&req.runtime);
        let project_id = req.project_id.clone();

        // If code was sent inline, write to a temp dir
        let (working_dir, entrypoint) = if !req.code.is_empty() {
            let tmp = std::env::temp_dir().join(format!("runway-agent-{}", Uuid::new_v4()));
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

        let (child, mut rx) = executor::spawn_and_stream(runtime, &entrypoint, &working_dir)
            .await
            .map_err(|e| Status::internal(format!("Failed to spawn: {e}")))?;

        let (tx, stream_rx) = tokio::sync::mpsc::channel(256);
        let processes = self.processes.clone();

        let task = tokio::spawn(async move {
            while let Some(line) = rx.recv().await {
                let resp = ExecuteResponse {
                    stream: match line.stream {
                        LogStream::Stdout => "stdout".into(),
                        LogStream::Stderr => "stderr".into(),
                    },
                    text: line.text,
                    timestamp: line.timestamp.to_rfc3339(),
                };
                if tx.send(Ok(resp)).await.is_err() {
                    break;
                }
            }
            // Clean up
            let mut procs = processes.lock().await;
            procs.remove(&project_id);
            drop(child);
        });

        {
            let mut procs = self.processes.lock().await;
            procs.insert(
                req.project_id,
                RunningChild {
                    abort_handle: task.abort_handle(),
                    started_at: chrono::Utc::now(),
                },
            );
        }

        Ok(Response::new(ReceiverStream::new(stream_rx)))
    }

    async fn status(
        &self,
        _request: Request<StatusRequest>,
    ) -> Result<Response<StatusResponse>, Status> {
        let procs = self.processes.lock().await;
        let projects: Vec<crate::proto::ProjectStatus> = procs
            .iter()
            .map(|(id, child)| crate::proto::ProjectStatus {
                project_id: id.clone(),
                status: "running".into(),
                pid: 0,
                started_at: child.started_at.to_rfc3339(),
            })
            .collect();

        Ok(Response::new(StatusResponse {
            agent_id: "local".into(),
            status: "running".into(),
            cpu_usage: 0.0,
            memory_bytes: 0,
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

        if let Some(child) = procs.remove(&project_id) {
            child.abort_handle.abort();
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
        }))
    }
}

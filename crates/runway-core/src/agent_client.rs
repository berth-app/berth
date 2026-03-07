use std::collections::HashMap;
use std::path::Path;
use std::time::Duration;

use anyhow::{Context, Result};
use tonic::transport::Channel;

use crate::executor::{LogLine, LogStream};

pub mod proto {
    tonic::include_proto!("runway");
}

use proto::agent_service_client::AgentServiceClient;

/// Health information returned by a remote agent.
pub struct AgentHealth {
    pub version: String,
    pub status: String,
    pub uptime_seconds: u64,
    pub podman_version: Option<String>,
    pub container_ready: bool,
    pub os: Option<String>,
    pub arch: Option<String>,
}

/// Execution history entry from a remote agent.
pub struct RemoteExecution {
    pub id: String,
    pub project_id: String,
    pub deployment_id: String,
    pub started_at: String,
    pub finished_at: String,
    pub exit_code: i32,
    pub trigger: String,
    pub status: String,
}

/// Event from a remote agent's store-and-forward queue.
pub struct RemoteEvent {
    pub id: i64,
    pub event_type: String,
    pub project_id: String,
    pub execution_id: String,
    pub data: String,
    pub created_at: String,
}

/// Schedule info from a remote agent.
pub struct RemoteSchedule {
    pub id: String,
    pub project_id: String,
    pub cron_expr: String,
    pub enabled: bool,
    pub created_at: String,
    pub last_triggered_at: String,
    pub next_run_at: String,
}

/// Result of deploying a project (build or setup).
pub struct DeployResult {
    pub image_tag: Option<String>,
    pub version: u32,
    pub success: bool,
}

/// Status of a remote agent including resource usage and running projects.
pub struct AgentStatus {
    pub agent_id: String,
    pub status: String,
    pub cpu_usage: f64,
    pub memory_bytes: u64,
    pub running_projects: Vec<RunningProject>,
}

/// A project currently running on the remote agent.
pub struct RunningProject {
    pub project_id: String,
    pub status: String,
    pub started_at: String,
}

/// Result of executing a project, including logs and exit code.
pub struct ExecuteResult {
    pub logs: Vec<LogLine>,
    pub exit_code: i32,
}

/// gRPC client for communicating with a remote Runway agent.
pub struct AgentClient {
    inner: AgentServiceClient<Channel>,
}

impl AgentClient {
    /// Connect to a remote agent at the given endpoint (e.g. "http://192.168.1.50:50051").
    pub async fn connect(endpoint: &str) -> Result<Self> {
        let channel = Channel::from_shared(endpoint.to_string())
            .context("Invalid agent endpoint URL")?
            .connect()
            .await
            .context("Failed to connect to remote agent — verify the agent is running and the endpoint is reachable")?;

        let inner = AgentServiceClient::new(channel)
            .max_decoding_message_size(64 * 1024 * 1024)
            .max_encoding_message_size(64 * 1024 * 1024);
        Ok(Self { inner })
    }

    /// Connect to a local agent via Unix domain socket.
    pub async fn connect_uds(path: &Path) -> Result<Self> {
        let channel = crate::uds::connect_uds(path).await?;
        let inner = AgentServiceClient::new(channel)
            .max_decoding_message_size(64 * 1024 * 1024)
            .max_encoding_message_size(64 * 1024 * 1024);
        Ok(Self { inner })
    }

    /// Check agent health (version, status, uptime).
    pub async fn health(&mut self) -> Result<AgentHealth> {
        let response = self
            .inner
            .health(proto::HealthRequest {})
            .await
            .context("Health RPC failed — the agent may be unreachable or unhealthy")?
            .into_inner();

        let podman_version = if response.podman_version.is_empty() {
            None
        } else {
            Some(response.podman_version)
        };

        let os = if response.os.is_empty() {
            None
        } else {
            Some(response.os)
        };
        let arch = if response.arch.is_empty() {
            None
        } else {
            Some(response.arch)
        };

        Ok(AgentHealth {
            version: response.agent_version,
            status: response.status,
            uptime_seconds: response.uptime_seconds,
            podman_version,
            container_ready: response.container_ready,
            os,
            arch,
        })
    }

    /// Deploy source code to an agent (build container or setup environment).
    /// Returns the raw gRPC stream for real-time build/setup output.
    pub async fn deploy_streaming(
        &mut self,
        project_id: &str,
        runtime: &str,
        entrypoint: &str,
        source_archive: &[u8],
        containerfile: &str,
        version: u32,
        setup_commands: Vec<String>,
    ) -> Result<tonic::Streaming<proto::DeployResponse>> {
        let request = proto::DeployRequest {
            project_id: project_id.to_string(),
            runtime: runtime.to_string(),
            entrypoint: entrypoint.to_string(),
            source_archive: source_archive.to_vec(),
            containerfile: containerfile.to_string(),
            version,
            setup_commands,
        };

        let response =
            tokio::time::timeout(Duration::from_secs(600), self.inner.deploy(request))
                .await
                .context("Deploy RPC timed out after 10 minutes")?
                .context("Deploy RPC failed — check the agent logs for details")?;

        Ok(response.into_inner())
    }

    /// Deploy source code and collect the result.
    pub async fn deploy(
        &mut self,
        project_id: &str,
        runtime: &str,
        entrypoint: &str,
        source_archive: &[u8],
        containerfile: &str,
        version: u32,
        setup_commands: Vec<String>,
    ) -> Result<DeployResult> {
        let mut stream = self
            .deploy_streaming(
                project_id,
                runtime,
                entrypoint,
                source_archive,
                containerfile,
                version,
                setup_commands,
            )
            .await?;

        let mut result = DeployResult {
            image_tag: None,
            version,
            success: false,
        };

        while let Some(msg) =
            tokio::time::timeout(Duration::from_secs(600), stream.message())
                .await
                .context("Timed out waiting for deploy stream")?
                .context("Error reading deploy stream")?
        {
            if msg.is_final {
                result.success = msg.success;
                if !msg.image_tag.is_empty() {
                    result.image_tag = Some(msg.image_tag);
                }
                result.version = msg.version;
            }
        }

        Ok(result)
    }

    /// Execute a project on an agent and return the raw gRPC stream.
    ///
    /// Callers can process log lines one at a time for real-time streaming.
    pub async fn execute_streaming(
        &mut self,
        project_id: &str,
        runtime: &str,
        entrypoint: &str,
        working_dir: &str,
        code: Option<&[u8]>,
        image_tag: Option<&str>,
        env_vars: HashMap<String, String>,
    ) -> Result<tonic::Streaming<proto::ExecuteResponse>> {
        let container_name = if image_tag.is_some() {
            format!("runway-{project_id}")
        } else {
            String::new()
        };

        let request = proto::ExecuteRequest {
            project_id: project_id.to_string(),
            runtime: runtime.to_string(),
            entrypoint: entrypoint.to_string(),
            working_dir: working_dir.to_string(),
            code: code.map(|c| c.to_vec()).unwrap_or_default(),
            image_tag: image_tag.unwrap_or_default().to_string(),
            env_vars,
            container_name,
        };

        let response = tokio::time::timeout(Duration::from_secs(300), self.inner.execute(request))
            .await
            .context("Execute RPC timed out after 5 minutes")?
            .context("Execute RPC failed — check the agent logs for details")?;

        Ok(response.into_inner())
    }

    /// Execute a project on an agent and collect all streaming log output.
    ///
    /// Returns logs and exit code. Uses a 5-minute timeout per message
    /// to avoid hanging on stuck processes.
    pub async fn execute(
        &mut self,
        project_id: &str,
        runtime: &str,
        entrypoint: &str,
        working_dir: &str,
        code: Option<&[u8]>,
        image_tag: Option<&str>,
        env_vars: HashMap<String, String>,
    ) -> Result<ExecuteResult> {
        let mut stream = self
            .execute_streaming(project_id, runtime, entrypoint, working_dir, code, image_tag, env_vars)
            .await?;

        let mut logs = Vec::new();
        let mut exit_code = 0i32;

        while let Some(msg) = tokio::time::timeout(Duration::from_secs(300), stream.message())
            .await
            .context("Timed out waiting for execute stream data")?
            .context("Error reading from execute stream")?
        {
            if msg.is_final {
                exit_code = msg.exit_code;
                continue;
            }

            let stream_type = match msg.stream.as_str() {
                "stderr" => LogStream::Stderr,
                _ => LogStream::Stdout,
            };

            let timestamp = chrono::DateTime::parse_from_rfc3339(&msg.timestamp)
                .map(|dt| dt.with_timezone(&chrono::Utc))
                .unwrap_or_else(|_| chrono::Utc::now());

            logs.push(LogLine {
                stream: stream_type,
                text: msg.text,
                timestamp,
            });
        }

        Ok(ExecuteResult { logs, exit_code })
    }

    /// Stop a running project on the remote agent. Returns true if stopped successfully.
    pub async fn stop(&mut self, project_id: &str) -> Result<bool> {
        let response = self
            .inner
            .stop(proto::StopRequest {
                project_id: project_id.to_string(),
            })
            .await
            .context("Stop RPC failed — the project may already be stopped or the agent is unreachable")?
            .into_inner();

        Ok(response.success)
    }

    /// Get the status of the remote agent and its running projects.
    pub async fn status(&mut self) -> Result<AgentStatus> {
        let response = self
            .inner
            .status(proto::StatusRequest {
                project_id: String::new(),
            })
            .await
            .context("Status RPC failed — the agent may be unreachable")?
            .into_inner();

        let running_projects = response
            .projects
            .into_iter()
            .map(|p| RunningProject {
                project_id: p.project_id,
                status: p.status,
                started_at: p.started_at,
            })
            .collect();

        Ok(AgentStatus {
            agent_id: response.agent_id,
            status: response.status,
            cpu_usage: response.cpu_usage,
            memory_bytes: response.memory_bytes,
            running_projects,
        })
    }

    // --- New persistent agent client methods ---

    /// Get execution history from the remote agent.
    pub async fn get_executions(
        &mut self,
        project_id: &str,
        limit: u32,
    ) -> Result<Vec<RemoteExecution>> {
        let response = self
            .inner
            .get_executions(proto::GetExecutionsRequest {
                project_id: project_id.to_string(),
                limit,
            })
            .await
            .context("GetExecutions RPC failed")?
            .into_inner();

        Ok(response
            .executions
            .into_iter()
            .map(|e| RemoteExecution {
                id: e.id,
                project_id: e.project_id,
                deployment_id: e.deployment_id,
                started_at: e.started_at,
                finished_at: e.finished_at,
                exit_code: e.exit_code,
                trigger: e.trigger,
                status: e.status,
            })
            .collect())
    }

    /// Get stored execution logs from the remote agent.
    pub async fn get_execution_logs(
        &mut self,
        execution_id: &str,
        since_seq: i64,
    ) -> Result<Vec<LogLine>> {
        let response = self
            .inner
            .get_execution_logs(proto::GetExecutionLogsRequest {
                execution_id: execution_id.to_string(),
                since_seq,
            })
            .await
            .context("GetExecutionLogs RPC failed")?;

        let mut stream = response.into_inner();
        let mut logs = Vec::new();

        while let Some(msg) = stream
            .message()
            .await
            .context("Error reading execution logs stream")?
        {
            let stream_type = match msg.stream.as_str() {
                "stderr" => LogStream::Stderr,
                _ => LogStream::Stdout,
            };
            let timestamp = chrono::DateTime::parse_from_rfc3339(&msg.timestamp)
                .map(|dt| dt.with_timezone(&chrono::Utc))
                .unwrap_or_else(|_| chrono::Utc::now());

            logs.push(LogLine {
                stream: stream_type,
                text: msg.text,
                timestamp,
            });
        }

        Ok(logs)
    }

    /// Poll for events from the remote agent (store-and-forward).
    pub async fn get_events(&mut self, since_id: i64, limit: u32) -> Result<Vec<RemoteEvent>> {
        let response = self
            .inner
            .get_events(proto::GetEventsRequest { since_id, limit })
            .await
            .context("GetEvents RPC failed")?
            .into_inner();

        Ok(response
            .events
            .into_iter()
            .map(|e| RemoteEvent {
                id: e.id,
                event_type: e.event_type,
                project_id: e.project_id,
                execution_id: e.execution_id,
                data: e.data,
                created_at: e.created_at,
            })
            .collect())
    }

    /// Acknowledge events, allowing the agent to prune them.
    pub async fn ack_events(&mut self, up_to_id: i64) -> Result<i64> {
        let response = self
            .inner
            .ack_events(proto::AckEventsRequest { up_to_id })
            .await
            .context("AckEvents RPC failed")?
            .into_inner();

        Ok(response.pruned_count)
    }

    /// Add a schedule on the remote agent.
    pub async fn add_remote_schedule(
        &mut self,
        project_id: &str,
        cron_expr: &str,
    ) -> Result<(String, String)> {
        let response = self
            .inner
            .add_schedule(proto::AddScheduleRequest {
                project_id: project_id.to_string(),
                cron_expr: cron_expr.to_string(),
            })
            .await
            .context("AddSchedule RPC failed")?
            .into_inner();

        Ok((response.schedule_id, response.next_run_at))
    }

    /// Remove a schedule from the remote agent.
    pub async fn remove_remote_schedule(&mut self, schedule_id: &str) -> Result<bool> {
        let response = self
            .inner
            .remove_schedule(proto::RemoveScheduleRequest {
                schedule_id: schedule_id.to_string(),
            })
            .await
            .context("RemoveSchedule RPC failed")?
            .into_inner();

        Ok(response.success)
    }

    /// List schedules on the remote agent.
    pub async fn list_remote_schedules(
        &mut self,
        project_id: &str,
    ) -> Result<Vec<RemoteSchedule>> {
        let response = self
            .inner
            .list_schedules(proto::ListSchedulesRequest {
                project_id: project_id.to_string(),
            })
            .await
            .context("ListSchedules RPC failed")?
            .into_inner();

        Ok(response
            .schedules
            .into_iter()
            .map(|s| RemoteSchedule {
                id: s.id,
                project_id: s.project_id,
                cron_expr: s.cron_expr,
                enabled: s.enabled,
                created_at: s.created_at,
                last_triggered_at: s.last_triggered_at,
                next_run_at: s.next_run_at,
            })
            .collect())
    }

    /// Upgrade the remote agent binary.
    pub async fn upgrade(&mut self, binary_data: &[u8]) -> Result<(bool, String, String)> {
        let total_size = binary_data.len() as u64;
        let chunk_size = 1024 * 1024; // 1MB chunks

        let chunks: Vec<proto::UpgradeChunk> = binary_data
            .chunks(chunk_size)
            .enumerate()
            .map(|(i, chunk)| proto::UpgradeChunk {
                data: chunk.to_vec(),
                total_size: if i == 0 { total_size } else { 0 },
            })
            .collect();

        let stream = tokio_stream::iter(chunks);

        let response = self
            .inner
            .upgrade(stream)
            .await
            .context("Upgrade RPC failed")?
            .into_inner();

        Ok((response.success, response.new_version, response.message))
    }
}

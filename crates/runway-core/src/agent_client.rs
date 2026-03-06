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

        let inner = AgentServiceClient::new(channel);
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

        Ok(AgentHealth {
            version: response.agent_version,
            status: response.status,
            uptime_seconds: response.uptime_seconds,
        })
    }

    /// Execute a project on the remote agent and collect all streaming log output.
    ///
    /// Returns the full log output as a Vec<LogLine>. Uses a 5-minute timeout
    /// to avoid hanging on long-running or stuck processes.
    pub async fn execute(
        &mut self,
        project_id: &str,
        runtime: &str,
        entrypoint: &str,
        working_dir: &str,
        code: Option<&[u8]>,
    ) -> Result<Vec<LogLine>> {
        let request = proto::ExecuteRequest {
            project_id: project_id.to_string(),
            runtime: runtime.to_string(),
            entrypoint: entrypoint.to_string(),
            working_dir: working_dir.to_string(),
            code: code.map(|c| c.to_vec()).unwrap_or_default(),
        };

        let response = tokio::time::timeout(Duration::from_secs(300), self.inner.execute(request))
            .await
            .context("Execute RPC timed out after 5 minutes")?
            .context("Execute RPC failed — check the agent logs for details")?;

        let mut stream = response.into_inner();
        let mut logs = Vec::new();

        while let Some(msg) = tokio::time::timeout(Duration::from_secs(300), stream.message())
            .await
            .context("Timed out waiting for execute stream data")?
            .context("Error reading from execute stream")?
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
}

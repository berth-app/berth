use std::collections::HashMap;

use anyhow::Result;
use async_trait::async_trait;

use crate::agent_client::{AgentHealth, AgentStatus, DeployResult, ExecuteResult, RemoteExecution, RemoteSchedule};
use crate::executor::LogLine;

pub struct ExecuteParams {
    pub project_id: String,
    pub runtime: String,
    pub entrypoint: String,
    pub working_dir: String,
    pub code: Option<Vec<u8>>,
    pub image_tag: Option<String>,
    pub env_vars: HashMap<String, String>,
}

pub struct DeployParams {
    pub project_id: String,
    pub runtime: String,
    pub entrypoint: String,
    pub source_archive: Vec<u8>,
    pub containerfile: String,
    pub version: u32,
    pub setup_commands: Vec<String>,
}

/// Streaming execute response line (mirrors gRPC ExecuteResponse).
pub struct ExecuteResponseLine {
    pub stream: String,
    pub text: String,
    pub timestamp: String,
    pub exit_code: i32,
    pub is_final: bool,
}

/// Streaming deploy response line (mirrors gRPC DeployResponse).
pub struct DeployResponseLine {
    pub phase: String,
    pub text: String,
    pub timestamp: String,
    pub image_tag: String,
    pub version: u32,
    pub is_final: bool,
    pub success: bool,
}

#[async_trait]
pub trait AgentTransport: Send + Sync {
    async fn health(&self) -> Result<AgentHealth>;
    async fn status(&self) -> Result<AgentStatus>;
    async fn stop(&self, project_id: &str) -> Result<bool>;

    async fn execute_streaming(
        &self,
        params: &ExecuteParams,
    ) -> Result<tokio::sync::mpsc::Receiver<ExecuteResponseLine>>;

    async fn execute(&self, params: &ExecuteParams) -> Result<ExecuteResult> {
        let mut rx = self.execute_streaming(params).await?;
        let mut logs = Vec::new();
        let mut exit_code = 0i32;

        while let Some(line) = rx.recv().await {
            if line.is_final {
                exit_code = line.exit_code;
                continue;
            }
            let stream_type = match line.stream.as_str() {
                "stderr" => crate::executor::LogStream::Stderr,
                _ => crate::executor::LogStream::Stdout,
            };
            let timestamp = chrono::DateTime::parse_from_rfc3339(&line.timestamp)
                .map(|dt| dt.with_timezone(&chrono::Utc))
                .unwrap_or_else(|_| chrono::Utc::now());
            logs.push(LogLine {
                stream: stream_type,
                text: line.text,
                timestamp,
            });
        }

        Ok(ExecuteResult { logs, exit_code })
    }

    async fn deploy_streaming(
        &self,
        params: &DeployParams,
    ) -> Result<tokio::sync::mpsc::Receiver<DeployResponseLine>>;

    async fn deploy(&self, params: &DeployParams) -> Result<DeployResult> {
        let mut rx = self.deploy_streaming(params).await?;
        let mut result = DeployResult {
            image_tag: None,
            version: params.version,
            success: false,
        };

        while let Some(line) = rx.recv().await {
            if line.is_final {
                result.success = line.success;
                if !line.image_tag.is_empty() {
                    result.image_tag = Some(line.image_tag);
                }
                result.version = line.version;
            }
        }

        Ok(result)
    }

    async fn get_executions(&self, project_id: &str, limit: u32) -> Result<Vec<RemoteExecution>>;
    async fn get_execution_logs(&self, execution_id: &str, since_seq: i64) -> Result<Vec<LogLine>>;
    async fn add_schedule(&self, project_id: &str, cron_expr: &str) -> Result<(String, String)>;
    async fn remove_schedule(&self, schedule_id: &str) -> Result<bool>;
    async fn list_schedules(&self, project_id: &str) -> Result<Vec<RemoteSchedule>>;
    async fn upgrade(&self, version: &str, download_url: &str, github_token: Option<&str>, checksum: &str) -> Result<(bool, String, String)>;
    async fn rollback(&self) -> Result<(bool, String, String)>;
}

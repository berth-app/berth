use std::time::Duration;

use anyhow::{Context, Result};
use async_trait::async_trait;
use base64::Engine;
use futures::StreamExt;

use crate::agent_client::{AgentHealth, AgentStatus, RemoteExecution, RemoteSchedule, RunningProject};
use crate::agent_transport::{AgentTransport, DeployParams, DeployResponseLine, ExecuteParams, ExecuteResponseLine};
use crate::executor::{LogLine, LogStream};
use crate::nats_relay::*;

/// NATS-based command client for communicating with remote agents through NATS relay.
#[derive(Clone)]
pub struct NatsAgentClient {
    client: async_nats::Client,
    agent_id: String,
}

impl NatsAgentClient {
    pub fn new(client: async_nats::Client, agent_id: String) -> Self {
        Self { client, agent_id }
    }

    async fn request_reply(&self, cmd_type: &str, cmd: NatsCommandKind) -> Result<NatsResponse> {
        let request_id = uuid::Uuid::new_v4().to_string();
        let command = NatsCommand {
            request_id,
            reply_to: String::new(), // async-nats request() manages this
            cmd,
        };

        let payload = serde_json::to_vec(&command).context("Failed to serialize NATS command")?;
        let subject = cmd_subject(&self.agent_id, cmd_type);

        let msg = tokio::time::timeout(
            Duration::from_secs(15),
            self.client.request(subject, payload.into()),
        )
        .await
        .context("NATS command timed out — agent may be offline")?
        .context("NATS request failed")?;

        let resp: NatsResponse =
            serde_json::from_slice(&msg.payload).context("Failed to deserialize NATS response")?;

        match &resp.status {
            NatsResponseStatus::Ok => Ok(resp),
            NatsResponseStatus::Error(e) => anyhow::bail!("Agent error: {e}"),
        }
    }

    async fn streaming_command(
        &self,
        cmd_type: &str,
        command: NatsCommand,
    ) -> Result<async_nats::Subscriber> {
        let resp_subj = resp_subject(&self.agent_id, &command.request_id);

        // Subscribe BEFORE publishing
        let sub = self
            .client
            .subscribe(resp_subj)
            .await
            .context("Failed to subscribe to response subject")?;

        let payload = serde_json::to_vec(&command).context("Failed to serialize NATS command")?;
        let subject = cmd_subject(&self.agent_id, cmd_type);

        self.client
            .publish(subject, payload.into())
            .await
            .context("Failed to publish NATS command")?;

        Ok(sub)
    }
}

#[async_trait]
impl AgentTransport for NatsAgentClient {
    async fn health(&self) -> Result<AgentHealth> {
        let resp = self.request_reply("health", NatsCommandKind::Health).await?;
        match resp.body {
            NatsResponseBody::Health {
                version,
                status,
                uptime_seconds,
                podman_version,
                container_ready,
                os,
                arch,
                probation_status,
            } => Ok(AgentHealth {
                version,
                status,
                uptime_seconds,
                podman_version: if podman_version.is_empty() { None } else { Some(podman_version) },
                container_ready,
                os: if os.is_empty() { None } else { Some(os) },
                arch: if arch.is_empty() { None } else { Some(arch) },
                probation_status,
            }),
            _ => anyhow::bail!("Unexpected response type for health"),
        }
    }

    async fn status(&self) -> Result<AgentStatus> {
        let resp = self.request_reply("status", NatsCommandKind::Status).await?;
        match resp.body {
            NatsResponseBody::Status {
                agent_id,
                status,
                cpu_usage,
                memory_bytes,
                projects,
            } => Ok(AgentStatus {
                agent_id,
                status,
                cpu_usage,
                memory_bytes,
                running_projects: projects
                    .into_iter()
                    .map(|p| RunningProject {
                        project_id: p.project_id,
                        status: p.status,
                        started_at: p.started_at,
                    })
                    .collect(),
            }),
            _ => anyhow::bail!("Unexpected response type for status"),
        }
    }

    async fn stop(&self, project_id: &str) -> Result<bool> {
        let resp = self
            .request_reply(
                "stop",
                NatsCommandKind::Stop {
                    project_id: project_id.to_string(),
                },
            )
            .await?;
        match resp.body {
            NatsResponseBody::Stop { success, .. } => Ok(success),
            _ => anyhow::bail!("Unexpected response type for stop"),
        }
    }

    async fn execute_streaming(
        &self,
        params: &ExecuteParams,
    ) -> Result<tokio::sync::mpsc::Receiver<ExecuteResponseLine>> {
        let request_id = uuid::Uuid::new_v4().to_string();
        let command = NatsCommand {
            request_id: request_id.clone(),
            reply_to: resp_subject(&self.agent_id, &request_id),
            cmd: NatsCommandKind::Execute {
                project_id: params.project_id.clone(),
                runtime: params.runtime.clone(),
                entrypoint: params.entrypoint.clone(),
                working_dir: params.working_dir.clone(),
                code: params.code.as_ref().map(|c| {
                    base64::engine::general_purpose::STANDARD.encode(c)
                }),
                image_tag: params.image_tag.clone(),
                env_vars: params.env_vars.clone(),
                container_name: None,
            },
        };

        let mut sub = self.streaming_command("execute", command).await?;
        let (tx, rx) = tokio::sync::mpsc::channel(256);

        tokio::spawn(async move {
            while let Ok(Some(msg)) = tokio::time::timeout(Duration::from_secs(300), sub.next())
                .await
            {
                let resp: NatsResponse = match serde_json::from_slice(&msg.payload) {
                    Ok(r) => r,
                    Err(_) => continue,
                };
                match resp.body {
                    NatsResponseBody::ExecuteLine {
                        stream,
                        text,
                        timestamp,
                        exit_code,
                        is_final,
                    } => {
                        let line = ExecuteResponseLine {
                            stream,
                            text,
                            timestamp,
                            exit_code,
                            is_final,
                        };
                        let done = is_final;
                        if tx.send(line).await.is_err() || done {
                            break;
                        }
                    }
                    _ => {}
                }
            }
        });

        Ok(rx)
    }

    async fn deploy_streaming(
        &self,
        params: &DeployParams,
    ) -> Result<tokio::sync::mpsc::Receiver<DeployResponseLine>> {
        let request_id = uuid::Uuid::new_v4().to_string();
        let archive_b64 = base64::engine::general_purpose::STANDARD.encode(&params.source_archive);

        let command = NatsCommand {
            request_id: request_id.clone(),
            reply_to: resp_subject(&self.agent_id, &request_id),
            cmd: NatsCommandKind::Deploy {
                project_id: params.project_id.clone(),
                runtime: params.runtime.clone(),
                entrypoint: params.entrypoint.clone(),
                containerfile: params.containerfile.clone(),
                version: params.version,
                setup_commands: params.setup_commands.clone(),
                archive_base64: archive_b64,
            },
        };

        let mut sub = self.streaming_command("deploy", command).await?;
        let (tx, rx) = tokio::sync::mpsc::channel(256);

        tokio::spawn(async move {
            while let Ok(Some(msg)) = tokio::time::timeout(Duration::from_secs(600), sub.next())
                .await
            {
                let resp: NatsResponse = match serde_json::from_slice(&msg.payload) {
                    Ok(r) => r,
                    Err(_) => continue,
                };
                match resp.body {
                    NatsResponseBody::DeployLine {
                        phase,
                        text,
                        timestamp,
                        image_tag,
                        version,
                        is_final,
                        success,
                    } => {
                        let line = DeployResponseLine {
                            phase,
                            text,
                            timestamp,
                            image_tag,
                            version,
                            is_final,
                            success,
                        };
                        let done = is_final;
                        if tx.send(line).await.is_err() || done {
                            break;
                        }
                    }
                    _ => {}
                }
            }
        });

        Ok(rx)
    }

    async fn get_executions(&self, project_id: &str, limit: u32) -> Result<Vec<RemoteExecution>> {
        let resp = self
            .request_reply(
                "executions",
                NatsCommandKind::GetExecutions {
                    project_id: project_id.to_string(),
                    limit,
                },
            )
            .await?;
        match resp.body {
            NatsResponseBody::Executions { executions } => Ok(executions
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
                .collect()),
            _ => anyhow::bail!("Unexpected response type for get_executions"),
        }
    }

    async fn get_execution_logs(&self, execution_id: &str, since_seq: i64) -> Result<Vec<LogLine>> {
        let request_id = uuid::Uuid::new_v4().to_string();
        let command = NatsCommand {
            request_id: request_id.clone(),
            reply_to: resp_subject(&self.agent_id, &request_id),
            cmd: NatsCommandKind::GetExecutionLogs {
                execution_id: execution_id.to_string(),
                since_seq,
            },
        };

        let mut sub = self.streaming_command("logs", command).await?;
        let mut logs = Vec::new();

        while let Ok(Some(msg)) = tokio::time::timeout(Duration::from_secs(30), sub.next()).await {
            let resp: NatsResponse = match serde_json::from_slice(&msg.payload) {
                Ok(r) => r,
                Err(_) => continue,
            };
            match resp.body {
                NatsResponseBody::ExecutionLogLine {
                    stream,
                    text,
                    timestamp,
                    is_final,
                } => {
                    if !is_final {
                        let stream_type = match stream.as_str() {
                            "stderr" => LogStream::Stderr,
                            _ => LogStream::Stdout,
                        };
                        let ts = chrono::DateTime::parse_from_rfc3339(&timestamp)
                            .map(|dt| dt.with_timezone(&chrono::Utc))
                            .unwrap_or_else(|_| chrono::Utc::now());
                        logs.push(LogLine {
                            stream: stream_type,
                            text,
                            timestamp: ts,
                        });
                    } else {
                        break;
                    }
                }
                _ => {}
            }
        }

        Ok(logs)
    }

    async fn add_schedule(&self, project_id: &str, cron_expr: &str) -> Result<(String, String)> {
        let resp = self
            .request_reply(
                "schedule.add",
                NatsCommandKind::AddSchedule {
                    project_id: project_id.to_string(),
                    cron_expr: cron_expr.to_string(),
                },
            )
            .await?;
        match resp.body {
            NatsResponseBody::ScheduleAdded {
                schedule_id,
                next_run_at,
            } => Ok((schedule_id, next_run_at)),
            _ => anyhow::bail!("Unexpected response type for add_schedule"),
        }
    }

    async fn remove_schedule(&self, schedule_id: &str) -> Result<bool> {
        let resp = self
            .request_reply(
                "schedule.remove",
                NatsCommandKind::RemoveSchedule {
                    schedule_id: schedule_id.to_string(),
                },
            )
            .await?;
        match resp.body {
            NatsResponseBody::ScheduleRemoved { success } => Ok(success),
            _ => anyhow::bail!("Unexpected response type for remove_schedule"),
        }
    }

    async fn list_schedules(&self, project_id: &str) -> Result<Vec<RemoteSchedule>> {
        let resp = self
            .request_reply(
                "schedule.list",
                NatsCommandKind::ListSchedules {
                    project_id: project_id.to_string(),
                },
            )
            .await?;
        match resp.body {
            NatsResponseBody::Schedules { schedules } => Ok(schedules
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
                .collect()),
            _ => anyhow::bail!("Unexpected response type for list_schedules"),
        }
    }

    async fn upgrade(&self, binary_data: &[u8]) -> Result<(bool, String, String)> {
        use sha2::{Sha256, Digest};

        let chunk_size = 768 * 1024; // 768KB — well under NATS 1MB limit
        let total_size = binary_data.len() as u64;
        let chunk_count = ((binary_data.len() + chunk_size - 1) / chunk_size) as u32;

        let mut hasher = Sha256::new();
        hasher.update(binary_data);
        let checksum = format!("{:x}", hasher.finalize());

        // Step 1: Send UpgradeStart command
        let request_id = uuid::Uuid::new_v4().to_string();
        let command = NatsCommand {
            request_id: request_id.clone(),
            reply_to: String::new(),
            cmd: NatsCommandKind::UpgradeStart {
                total_size,
                chunk_count,
                checksum_sha256: checksum,
            },
        };

        let payload = serde_json::to_vec(&command).context("Failed to serialize upgrade command")?;
        let subject = cmd_subject(&self.agent_id, "upgrade");

        // Use request-reply for the initial handshake
        let msg = tokio::time::timeout(
            Duration::from_secs(15),
            self.client.request(subject, payload.into()),
        )
        .await
        .context("Upgrade handshake timed out")?
        .context("Upgrade handshake failed")?;

        let ready_resp: NatsResponse = serde_json::from_slice(&msg.payload)
            .context("Failed to deserialize upgrade ready response")?;

        let upload_subject = match &ready_resp.body {
            NatsResponseBody::UpgradeReady { upload_subject } => upload_subject.clone(),
            _ => anyhow::bail!("Unexpected response to UpgradeStart"),
        };

        // Step 2: Subscribe to result subject before sending chunks
        let result_subject = resp_subject(&self.agent_id, &request_id);
        let mut result_sub = self.client.subscribe(result_subject).await
            .context("Failed to subscribe to upgrade result")?;

        // Step 3: Send binary chunks
        for (i, chunk) in binary_data.chunks(chunk_size).enumerate() {
            let chunk_payload = serde_json::json!({
                "seq": i,
                "data": base64::engine::general_purpose::STANDARD.encode(chunk),
            });
            let chunk_bytes = serde_json::to_vec(&chunk_payload)?;
            self.client.publish(upload_subject.clone(), chunk_bytes.into()).await
                .context("Failed to publish upgrade chunk")?;
        }

        // Step 4: Send "done" marker
        let done_payload = serde_json::json!({"seq": -1, "done": true});
        let done_bytes = serde_json::to_vec(&done_payload)?;
        self.client.publish(upload_subject, done_bytes.into()).await
            .context("Failed to publish upgrade done marker")?;

        // Step 5: Wait for result (longer timeout for upgrade — binary verification + restart)
        let result_msg = tokio::time::timeout(
            Duration::from_secs(120),
            result_sub.next(),
        )
        .await
        .context("Upgrade timed out waiting for result")?
        .context("Upgrade result stream ended unexpectedly")?;

        let result_resp: NatsResponse = serde_json::from_slice(&result_msg.payload)
            .context("Failed to deserialize upgrade result")?;

        // Check for error status first
        if let NatsResponseStatus::Error(e) = &result_resp.status {
            anyhow::bail!("Agent upgrade failed: {e}");
        }

        match result_resp.body {
            NatsResponseBody::UpgradeResult { success, new_version, message } => {
                Ok((success, new_version, message))
            }
            other => anyhow::bail!("Unexpected upgrade result response type: {:?}", other),
        }
    }

    async fn rollback(&self) -> Result<(bool, String, String)> {
        let resp = self.request_reply("rollback", NatsCommandKind::Rollback).await?;
        match resp.body {
            NatsResponseBody::Rollback { success, restored_version, message } => {
                Ok((success, restored_version, message))
            }
            _ => anyhow::bail!("Unexpected response type for rollback"),
        }
    }
}

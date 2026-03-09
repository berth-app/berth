use std::sync::Arc;

use base64::Engine;
use futures::StreamExt;

use berth_proto::nats_relay::*;
use berth_proto::executor::LogStream;
use berth_proto::message_auth::{self, NonceTracker};

use crate::persistent_service::PersistentAgentService;

pub struct NatsCommandHandler {
    client: async_nats::Client,
    agent_id: String,
    owner_id: String,
    service: Arc<PersistentAgentService>,
    /// Shared secret for HMAC verification (established during pairing).
    shared_secret: Option<Vec<u8>>,
    nonce_tracker: Arc<NonceTracker>,
}

impl NatsCommandHandler {
    pub fn new(
        client: async_nats::Client,
        agent_id: String,
        owner_id: String,
        service: Arc<PersistentAgentService>,
    ) -> Self {
        Self {
            client,
            agent_id,
            owner_id,
            service,
            shared_secret: None,
            nonce_tracker: Arc::new(NonceTracker::new()),
        }
    }

    /// Set the shared secret for HMAC command verification.
    pub fn with_secret(mut self, secret: Vec<u8>) -> Self {
        self.shared_secret = Some(secret);
        self
    }

    /// Verify a command's HMAC signature, timestamp, and nonce.
    /// Returns `true` if the command is authentic.
    fn verify_command(&self, cmd: &NatsCommand) -> bool {
        let Some(secret) = &self.shared_secret else {
            // No secret configured — accept all commands (pre-pairing compatibility)
            return true;
        };

        // Require signature when secret is configured
        if cmd.signature.is_empty() {
            tracing::warn!("Rejecting unsigned NATS command (request_id={})", cmd.request_id);
            return false;
        }

        // Check timestamp freshness
        if !message_auth::is_timestamp_valid(cmd.timestamp) {
            tracing::warn!(
                "Rejecting expired NATS command (request_id={}, age={}s)",
                cmd.request_id,
                message_auth::current_timestamp() - cmd.timestamp
            );
            return false;
        }

        // Check nonce replay
        if !self.nonce_tracker.check_and_record(&cmd.nonce) {
            tracing::warn!("Rejecting replayed NATS command (request_id={}, nonce={})", cmd.request_id, cmd.nonce);
            return false;
        }

        // Verify HMAC signature
        let cmd_payload = serde_json::to_vec(&cmd.cmd).unwrap_or_default();
        if !message_auth::verify_signature(&cmd_payload, &cmd.nonce, cmd.timestamp, &cmd.signature, secret) {
            tracing::warn!("Rejecting NATS command with invalid signature (request_id={})", cmd.request_id);
            return false;
        }

        true
    }

    pub async fn run(self) {
        let subject = format!("berth.{}.{}.cmd.>", self.owner_id, self.agent_id);
        let mut sub = match self.client.subscribe(subject.clone()).await {
            Ok(s) => s,
            Err(e) => {
                tracing::error!("Failed to subscribe to {subject}: {e}");
                return;
            }
        };

        tracing::info!("NATS command handler listening on {subject}");
        if self.shared_secret.is_some() {
            tracing::info!("HMAC command verification enabled");
        } else {
            tracing::warn!("HMAC command verification disabled — no shared secret configured. Pair this agent to enable security.");
        }

        while let Some(msg) = sub.next().await {
            let cmd: NatsCommand = match serde_json::from_slice(&msg.payload) {
                Ok(c) => c,
                Err(e) => {
                    tracing::warn!("Failed to deserialize NATS command: {e}");
                    continue;
                }
            };

            // Verify HMAC signature before processing
            if !self.verify_command(&cmd) {
                continue;
            }

            let client = self.client.clone();
            let service = self.service.clone();
            let agent_id = self.agent_id.clone();
            let owner_id = self.owner_id.clone();
            let reply_to = msg.reply.map(|s| s.to_string()).unwrap_or_else(|| cmd.reply_to.clone());

            tokio::spawn(async move {
                handle_command(client, service, cmd, reply_to, agent_id, owner_id).await;
            });
        }
    }
}

async fn handle_command(
    client: async_nats::Client,
    service: Arc<PersistentAgentService>,
    cmd: NatsCommand,
    reply_to: String,
    agent_id: String,
    owner_id: String,
) {
    let request_id = cmd.request_id.clone();

    match cmd.cmd {
        NatsCommandKind::Health => {
            let info = service.do_health();
            let resp = NatsResponse {
                request_id,
                status: NatsResponseStatus::Ok,
                body: NatsResponseBody::Health {
                    version: info.version,
                    status: info.status,
                    uptime_seconds: info.uptime_seconds,
                    podman_version: info.podman_version,
                    container_ready: info.container_ready,
                    os: info.os,
                    arch: info.arch,
                    probation_status: info.probation_status,
                    tunnel_providers: info.tunnel_providers,
                },
            };
            send_reply(&client, &reply_to, &resp).await;
        }

        NatsCommandKind::Status => {
            let info = service.do_status().await;
            let projects: Vec<NatsProjectStatus> = info
                .projects
                .into_iter()
                .map(|p| NatsProjectStatus {
                    project_id: p.project_id,
                    status: p.status,
                    started_at: p.started_at,
                })
                .collect();

            let resp = NatsResponse {
                request_id,
                status: NatsResponseStatus::Ok,
                body: NatsResponseBody::Status {
                    agent_id: info.agent_id,
                    status: info.status,
                    cpu_usage: info.cpu_usage,
                    memory_bytes: info.memory_bytes,
                    projects,
                },
            };
            send_reply(&client, &reply_to, &resp).await;
        }

        NatsCommandKind::Stop { project_id } => {
            let (success, message) = service.do_stop(&project_id).await;
            let resp = NatsResponse {
                request_id,
                status: NatsResponseStatus::Ok,
                body: NatsResponseBody::Stop { success, message },
            };
            send_reply(&client, &reply_to, &resp).await;
        }

        NatsCommandKind::Execute {
            project_id,
            runtime,
            entrypoint,
            working_dir,
            code,
            image_tag,
            env_vars,
            container_name,
            run_mode,
            service_port,
        } => {
            let code_bytes = code.and_then(|c| {
                base64::engine::general_purpose::STANDARD.decode(c).ok()
            });

            let mut rx = match service
                .do_execute(
                    &project_id,
                    &runtime,
                    &entrypoint,
                    &working_dir,
                    code_bytes.as_deref(),
                    image_tag.as_deref(),
                    env_vars,
                    container_name.as_deref(),
                    &run_mode,
                    service_port,
                )
                .await
            {
                Ok(rx) => rx,
                Err(e) => {
                    let resp = NatsResponse {
                        request_id,
                        status: NatsResponseStatus::Error(e.to_string()),
                        body: NatsResponseBody::Empty,
                    };
                    send_reply(&client, &reply_to, &resp).await;
                    return;
                }
            };

            while let Some(line) = rx.recv().await {
                let resp = NatsResponse {
                    request_id: request_id.clone(),
                    status: NatsResponseStatus::Ok,
                    body: NatsResponseBody::ExecuteLine {
                        stream: line.stream,
                        text: line.text,
                        timestamp: line.timestamp,
                        exit_code: line.exit_code,
                        is_final: line.is_final,
                    },
                };
                send_reply(&client, &reply_to, &resp).await;
                if line.is_final {
                    break;
                }
            }
        }

        NatsCommandKind::Deploy {
            project_id,
            runtime,
            entrypoint,
            containerfile,
            version,
            setup_commands,
            archive_base64,
        } => {
            let archive = match base64::engine::general_purpose::STANDARD.decode(&archive_base64) {
                Ok(a) => a,
                Err(e) => {
                    let resp = NatsResponse {
                        request_id,
                        status: NatsResponseStatus::Error(format!("Failed to decode archive: {e}")),
                        body: NatsResponseBody::Empty,
                    };
                    send_reply(&client, &reply_to, &resp).await;
                    return;
                }
            };

            let mut rx = match service
                .do_deploy(&project_id, &runtime, &entrypoint, &archive, &containerfile, version, setup_commands)
                .await
            {
                Ok(rx) => rx,
                Err(e) => {
                    let resp = NatsResponse {
                        request_id,
                        status: NatsResponseStatus::Error(e.to_string()),
                        body: NatsResponseBody::Empty,
                    };
                    send_reply(&client, &reply_to, &resp).await;
                    return;
                }
            };

            while let Some(line) = rx.recv().await {
                let resp = NatsResponse {
                    request_id: request_id.clone(),
                    status: NatsResponseStatus::Ok,
                    body: NatsResponseBody::DeployLine {
                        phase: line.phase,
                        text: line.text,
                        timestamp: line.timestamp,
                        image_tag: line.image_tag,
                        version: line.version,
                        is_final: line.is_final,
                        success: line.success,
                    },
                };
                send_reply(&client, &reply_to, &resp).await;
                if line.is_final {
                    break;
                }
            }
        }

        NatsCommandKind::GetExecutions { project_id, limit } => {
            match service.do_get_executions(&project_id, limit).await {
                Ok(execs) => {
                    let resp = NatsResponse {
                        request_id,
                        status: NatsResponseStatus::Ok,
                        body: NatsResponseBody::Executions {
                            executions: execs
                                .into_iter()
                                .map(|e| NatsExecutionInfo {
                                    id: e.id,
                                    project_id: e.project_id,
                                    deployment_id: e.deployment_id,
                                    started_at: e.started_at,
                                    finished_at: e.finished_at,
                                    exit_code: e.exit_code,
                                    trigger: e.trigger,
                                    status: e.status,
                                })
                                .collect(),
                        },
                    };
                    send_reply(&client, &reply_to, &resp).await;
                }
                Err(e) => {
                    let resp = NatsResponse {
                        request_id,
                        status: NatsResponseStatus::Error(e.to_string()),
                        body: NatsResponseBody::Empty,
                    };
                    send_reply(&client, &reply_to, &resp).await;
                }
            }
        }

        NatsCommandKind::GetExecutionLogs {
            execution_id,
            since_seq,
        } => {
            match service.do_get_execution_logs(&execution_id, since_seq).await {
                Ok(logs) => {
                    for log in &logs {
                        let stream_str = match log.stream {
                            LogStream::Stdout => "stdout",
                            LogStream::Stderr => "stderr",
                        };
                        let resp = NatsResponse {
                            request_id: request_id.clone(),
                            status: NatsResponseStatus::Ok,
                            body: NatsResponseBody::ExecutionLogLine {
                                stream: stream_str.to_string(),
                                text: log.text.clone(),
                                timestamp: log.timestamp.to_rfc3339(),
                                is_final: false,
                            },
                        };
                        send_reply(&client, &reply_to, &resp).await;
                    }
                    // Final marker
                    let resp = NatsResponse {
                        request_id,
                        status: NatsResponseStatus::Ok,
                        body: NatsResponseBody::ExecutionLogLine {
                            stream: String::new(),
                            text: String::new(),
                            timestamp: String::new(),
                            is_final: true,
                        },
                    };
                    send_reply(&client, &reply_to, &resp).await;
                }
                Err(e) => {
                    let resp = NatsResponse {
                        request_id,
                        status: NatsResponseStatus::Error(e.to_string()),
                        body: NatsResponseBody::Empty,
                    };
                    send_reply(&client, &reply_to, &resp).await;
                }
            }
        }

        NatsCommandKind::AddSchedule {
            project_id,
            cron_expr,
        } => {
            match service.do_add_schedule(&project_id, &cron_expr).await {
                Ok((schedule_id, next_run_at)) => {
                    let resp = NatsResponse {
                        request_id,
                        status: NatsResponseStatus::Ok,
                        body: NatsResponseBody::ScheduleAdded {
                            schedule_id,
                            next_run_at,
                        },
                    };
                    send_reply(&client, &reply_to, &resp).await;
                }
                Err(e) => {
                    let resp = NatsResponse {
                        request_id,
                        status: NatsResponseStatus::Error(e.to_string()),
                        body: NatsResponseBody::Empty,
                    };
                    send_reply(&client, &reply_to, &resp).await;
                }
            }
        }

        NatsCommandKind::RemoveSchedule { schedule_id } => {
            match service.do_remove_schedule(&schedule_id).await {
                Ok(success) => {
                    let resp = NatsResponse {
                        request_id,
                        status: NatsResponseStatus::Ok,
                        body: NatsResponseBody::ScheduleRemoved { success },
                    };
                    send_reply(&client, &reply_to, &resp).await;
                }
                Err(e) => {
                    let resp = NatsResponse {
                        request_id,
                        status: NatsResponseStatus::Error(e.to_string()),
                        body: NatsResponseBody::Empty,
                    };
                    send_reply(&client, &reply_to, &resp).await;
                }
            }
        }

        NatsCommandKind::Rollback => {
            match service.do_rollback().await {
                Ok((success, restored_version, message)) => {
                    let resp = NatsResponse {
                        request_id,
                        status: NatsResponseStatus::Ok,
                        body: NatsResponseBody::Rollback {
                            success,
                            restored_version,
                            message,
                        },
                    };
                    send_reply(&client, &reply_to, &resp).await;
                }
                Err(e) => {
                    let resp = NatsResponse {
                        request_id,
                        status: NatsResponseStatus::Error(e),
                        body: NatsResponseBody::Empty,
                    };
                    send_reply(&client, &reply_to, &resp).await;
                }
            }
        }

        NatsCommandKind::UpgradeDownload {
            version,
            download_url,
            github_token,
            checksum_sha256,
        } => {
            let result_subject = berth_proto::nats_relay::resp_subject(&owner_id, &agent_id, &request_id);

            tracing::info!("Upgrade requested: v{version}, downloading from {download_url}");

            // Download binary from URL
            let http_client = match reqwest::Client::builder().user_agent("berth-agent").build() {
                Ok(c) => c,
                Err(e) => {
                    let resp = NatsResponse {
                        request_id,
                        status: NatsResponseStatus::Error(format!("Failed to create HTTP client: {e}")),
                        body: NatsResponseBody::Empty,
                    };
                    send_reply(&client, &result_subject, &resp).await;
                    return;
                }
            };

            let mut req = http_client.get(&download_url);
            if let Some(token) = &github_token {
                req = req.header("Authorization", format!("Bearer {token}"));
                req = req.header("Accept", "application/octet-stream");
            }

            let response = match req.send().await {
                Ok(r) => r,
                Err(e) => {
                    let resp = NatsResponse {
                        request_id,
                        status: NatsResponseStatus::Error(format!("Download failed: {e}")),
                        body: NatsResponseBody::Empty,
                    };
                    send_reply(&client, &result_subject, &resp).await;
                    return;
                }
            };

            if !response.status().is_success() {
                let status = response.status();
                let resp = NatsResponse {
                    request_id,
                    status: NatsResponseStatus::Error(format!("Download failed: HTTP {status}")),
                    body: NatsResponseBody::Empty,
                };
                send_reply(&client, &result_subject, &resp).await;
                return;
            }

            let data = match response.bytes().await {
                Ok(b) => b.to_vec(),
                Err(e) => {
                    let resp = NatsResponse {
                        request_id,
                        status: NatsResponseStatus::Error(format!("Failed to read download: {e}")),
                        body: NatsResponseBody::Empty,
                    };
                    send_reply(&client, &result_subject, &resp).await;
                    return;
                }
            };

            tracing::info!("Downloaded {} bytes", data.len());

            // Verify SHA-256 checksum
            use sha2::{Sha256, Digest};
            let mut hasher = Sha256::new();
            hasher.update(&data);
            let actual_checksum = format!("{:x}", hasher.finalize());

            if actual_checksum != checksum_sha256 {
                let resp = NatsResponse {
                    request_id,
                    status: NatsResponseStatus::Error(format!(
                        "Checksum mismatch: expected {checksum_sha256}, got {actual_checksum}"
                    )),
                    body: NatsResponseBody::Empty,
                };
                send_reply(&client, &result_subject, &resp).await;
                return;
            }

            // Perform the upgrade
            let resp = match service.do_upgrade_from_bytes(data).await {
                Ok((success, new_version, message)) => NatsResponse {
                    request_id,
                    status: NatsResponseStatus::Ok,
                    body: NatsResponseBody::UpgradeResult {
                        success,
                        new_version,
                        message,
                    },
                },
                Err(e) => NatsResponse {
                    request_id,
                    status: NatsResponseStatus::Error(e),
                    body: NatsResponseBody::Empty,
                },
            };
            send_reply(&client, &result_subject, &resp).await;
            let _ = client.flush().await;
        }

        NatsCommandKind::DeployChunked {
            project_id,
            runtime,
            entrypoint,
            containerfile,
            version,
            setup_commands,
            total_size,
            chunk_count: _,
            checksum_sha256,
        } => {
            // Create upload subject for receiving archive chunks
            let upload_subject = berth_proto::nats_relay::upload_subject(&owner_id, &agent_id, &request_id);

            // Generate a random upload token to authorize chunk uploads
            let upload_token = uuid::Uuid::new_v4().to_string();

            let mut chunk_sub = match client.subscribe(upload_subject.clone()).await {
                Ok(s) => s,
                Err(e) => {
                    let resp = NatsResponse {
                        request_id,
                        status: NatsResponseStatus::Error(format!("Failed to subscribe for chunks: {e}")),
                        body: NatsResponseBody::Empty,
                    };
                    send_reply(&client, &reply_to, &resp).await;
                    return;
                }
            };

            // Reply with DeployReady including the upload token
            let ready_resp = NatsResponse {
                request_id: request_id.clone(),
                status: NatsResponseStatus::Ok,
                body: NatsResponseBody::DeployReady {
                    upload_subject: upload_subject.clone(),
                    upload_token: upload_token.clone(),
                },
            };
            send_reply(&client, &reply_to, &ready_resp).await;

            // Collect chunks — verify upload_token on each
            let mut archive = Vec::with_capacity(total_size as usize);
            let mut received_chunks = 0u32;
            let chunk_timeout = tokio::time::Duration::from_secs(120);

            loop {
                match tokio::time::timeout(chunk_timeout, chunk_sub.next()).await {
                    Ok(Some(msg)) => {
                        let chunk_msg: serde_json::Value = match serde_json::from_slice(&msg.payload) {
                            Ok(v) => v,
                            Err(e) => {
                                tracing::warn!("Failed to parse deploy chunk: {e}");
                                continue;
                            }
                        };

                        // Verify upload token
                        let token = chunk_msg.get("token").and_then(|v| v.as_str()).unwrap_or("");
                        if token != upload_token {
                            tracing::warn!("Rejecting deploy chunk with invalid upload token");
                            continue;
                        }

                        if chunk_msg.get("done").and_then(|v| v.as_bool()).unwrap_or(false) {
                            break;
                        }

                        if let Some(b64_data) = chunk_msg.get("data").and_then(|v| v.as_str()) {
                            match base64::engine::general_purpose::STANDARD.decode(b64_data) {
                                Ok(chunk_bytes) => {
                                    archive.extend_from_slice(&chunk_bytes);
                                    received_chunks += 1;
                                }
                                Err(e) => {
                                    tracing::warn!("Failed to decode chunk {}: {e}", received_chunks);
                                }
                            }
                        }
                    }
                    Ok(None) => {
                        let result_subject = berth_proto::nats_relay::resp_subject(&owner_id, &agent_id, &request_id);
                        let resp = NatsResponse {
                            request_id: request_id.clone(),
                            status: NatsResponseStatus::Error("Deploy chunk stream ended unexpectedly".into()),
                            body: NatsResponseBody::Empty,
                        };
                        send_reply(&client, &result_subject, &resp).await;
                        return;
                    }
                    Err(_) => {
                        let result_subject = berth_proto::nats_relay::resp_subject(&owner_id, &agent_id, &request_id);
                        let resp = NatsResponse {
                            request_id: request_id.clone(),
                            status: NatsResponseStatus::Error("Deploy chunk transfer timed out".into()),
                            body: NatsResponseBody::Empty,
                        };
                        send_reply(&client, &result_subject, &resp).await;
                        return;
                    }
                }
            }

            tracing::info!("Received {} deploy chunks, {} bytes total", received_chunks, archive.len());

            // Verify checksum
            use sha2::{Sha256, Digest};
            let mut hasher = Sha256::new();
            hasher.update(&archive);
            let actual_checksum = format!("{:x}", hasher.finalize());

            let resp_subj = berth_proto::nats_relay::resp_subject(&owner_id, &agent_id, &request_id);

            if actual_checksum != checksum_sha256 {
                let resp = NatsResponse {
                    request_id,
                    status: NatsResponseStatus::Error(format!(
                        "Archive checksum mismatch: expected {checksum_sha256}, got {actual_checksum}"
                    )),
                    body: NatsResponseBody::Empty,
                };
                send_reply(&client, &resp_subj, &resp).await;
                return;
            }

            // Proceed with deploy using existing do_deploy
            let mut rx = match service
                .do_deploy(&project_id, &runtime, &entrypoint, &archive, &containerfile, version, setup_commands)
                .await
            {
                Ok(rx) => rx,
                Err(e) => {
                    let resp = NatsResponse {
                        request_id,
                        status: NatsResponseStatus::Error(e.to_string()),
                        body: NatsResponseBody::Empty,
                    };
                    send_reply(&client, &resp_subj, &resp).await;
                    return;
                }
            };

            while let Some(line) = rx.recv().await {
                let resp = NatsResponse {
                    request_id: request_id.clone(),
                    status: NatsResponseStatus::Ok,
                    body: NatsResponseBody::DeployLine {
                        phase: line.phase,
                        text: line.text,
                        timestamp: line.timestamp,
                        image_tag: line.image_tag,
                        version: line.version,
                        is_final: line.is_final,
                        success: line.success,
                    },
                };
                send_reply(&client, &resp_subj, &resp).await;
                if line.is_final {
                    break;
                }
            }
        }

        NatsCommandKind::ListSchedules { project_id } => {
            match service.do_list_schedules(&project_id).await {
                Ok(schedules) => {
                    let resp = NatsResponse {
                        request_id,
                        status: NatsResponseStatus::Ok,
                        body: NatsResponseBody::Schedules {
                            schedules: schedules
                                .into_iter()
                                .map(|s| NatsScheduleInfo {
                                    id: s.id,
                                    project_id: s.project_id,
                                    cron_expr: s.cron_expr,
                                    enabled: s.enabled,
                                    created_at: s.created_at,
                                    last_triggered_at: s.last_triggered_at,
                                    next_run_at: s.next_run_at,
                                })
                                .collect(),
                        },
                    };
                    send_reply(&client, &reply_to, &resp).await;
                }
                Err(e) => {
                    let resp = NatsResponse {
                        request_id,
                        status: NatsResponseStatus::Error(e.to_string()),
                        body: NatsResponseBody::Empty,
                    };
                    send_reply(&client, &reply_to, &resp).await;
                }
            }
        }

        NatsCommandKind::Publish {
            project_id,
            port,
            provider,
            provider_config,
        } => {
            match service
                .do_publish(&project_id, port as u16, &provider, &provider_config)
                .await
            {
                Ok((success, url, provider, message)) => {
                    let resp = NatsResponse {
                        request_id,
                        status: NatsResponseStatus::Ok,
                        body: NatsResponseBody::Publish {
                            success,
                            url,
                            provider,
                            message,
                        },
                    };
                    send_reply(&client, &reply_to, &resp).await;
                }
                Err(e) => {
                    let resp = NatsResponse {
                        request_id,
                        status: NatsResponseStatus::Error(e),
                        body: NatsResponseBody::Empty,
                    };
                    send_reply(&client, &reply_to, &resp).await;
                }
            }
        }

        NatsCommandKind::Unpublish { project_id } => {
            match service.do_unpublish(&project_id).await {
                Ok((success, message)) => {
                    let resp = NatsResponse {
                        request_id,
                        status: NatsResponseStatus::Ok,
                        body: NatsResponseBody::Unpublish { success, message },
                    };
                    send_reply(&client, &reply_to, &resp).await;
                }
                Err(e) => {
                    let resp = NatsResponse {
                        request_id,
                        status: NatsResponseStatus::Error(e),
                        body: NatsResponseBody::Empty,
                    };
                    send_reply(&client, &reply_to, &resp).await;
                }
            }
        }
    }
}

async fn send_reply(client: &async_nats::Client, subject: &str, resp: &NatsResponse) {
    if subject.is_empty() {
        return;
    }
    match serde_json::to_vec(resp) {
        Ok(payload) => {
            if let Err(e) = client.publish(subject.to_string(), payload.into()).await {
                tracing::warn!("Failed to send NATS reply: {e}");
            }
        }
        Err(e) => {
            tracing::warn!("Failed to serialize NATS response: {e}");
        }
    }
}

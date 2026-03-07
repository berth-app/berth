use std::sync::Arc;

use base64::Engine;
use futures::StreamExt;

use runway_core::nats_relay::*;
use runway_core::executor::LogStream;

use crate::persistent_service::PersistentAgentService;

pub struct NatsCommandHandler {
    client: async_nats::Client,
    agent_id: String,
    service: Arc<PersistentAgentService>,
}

impl NatsCommandHandler {
    pub fn new(
        client: async_nats::Client,
        agent_id: String,
        service: Arc<PersistentAgentService>,
    ) -> Self {
        Self {
            client,
            agent_id,
            service,
        }
    }

    pub async fn run(self) {
        let subject = format!("runway.{}.cmd.>", self.agent_id);
        let mut sub = match self.client.subscribe(subject.clone()).await {
            Ok(s) => s,
            Err(e) => {
                tracing::error!("Failed to subscribe to {subject}: {e}");
                return;
            }
        };

        tracing::info!("NATS command handler listening on {subject}");

        while let Some(msg) = sub.next().await {
            let cmd: NatsCommand = match serde_json::from_slice(&msg.payload) {
                Ok(c) => c,
                Err(e) => {
                    tracing::warn!("Failed to deserialize NATS command: {e}");
                    continue;
                }
            };

            let client = self.client.clone();
            let service = self.service.clone();
            let reply_to = msg.reply.map(|s| s.to_string()).unwrap_or_else(|| cmd.reply_to.clone());

            tokio::spawn(async move {
                handle_command(client, service, cmd, reply_to).await;
            });
        }
    }
}

async fn handle_command(
    client: async_nats::Client,
    service: Arc<PersistentAgentService>,
    cmd: NatsCommand,
    reply_to: String,
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

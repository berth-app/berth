use std::sync::Arc;

use async_nats::jetstream;
use chrono::Utc;
use berth_proto::nats_relay::{
    self, NatsConfig, NatsEvent, NatsHeartbeat, NatsLogLine,
};

pub struct NatsPublisher {
    client: async_nats::Client,
    jetstream: jetstream::Context,
    agent_id: String,
    owner_id: String,
}

impl NatsPublisher {
    pub async fn connect(config: &NatsConfig) -> anyhow::Result<Self> {
        let mut opts = async_nats::ConnectOptions::new();
        if let Some(ref creds_path) = config.creds_path {
            opts = opts.credentials_file(creds_path).await?;
        }

        let client = opts.connect(&config.url).await?;
        let jetstream = jetstream::new(client.clone());

        tracing::info!(
            agent_id = config.agent_id,
            url = config.url,
            "NATS publisher connected"
        );

        Ok(Self {
            client,
            jetstream,
            agent_id: config.agent_id.clone(),
            owner_id: config.owner_id.clone(),
        })
    }

    pub fn client(&self) -> &async_nats::Client {
        &self.client
    }

    pub async fn publish_event(
        &self,
        event_type: &str,
        project_id: Option<&str>,
        execution_id: Option<&str>,
        data: &str,
    ) {
        let event = NatsEvent {
            agent_id: self.agent_id.clone(),
            event_type: event_type.to_string(),
            project_id: project_id.map(String::from),
            execution_id: execution_id.map(String::from),
            data: data.to_string(),
            created_at: Utc::now(),
        };

        let subject = nats_relay::event_subject(&self.owner_id, &self.agent_id, event_type);
        match serde_json::to_vec(&event) {
            Ok(payload) => {
                if let Err(e) = self.jetstream.publish(subject, payload.into()).await {
                    tracing::warn!("Failed to publish NATS event: {e}");
                }
            }
            Err(e) => {
                tracing::warn!("Failed to serialize NATS event: {e}");
            }
        }
    }

    pub async fn publish_log_line(
        &self,
        project_id: &str,
        execution_id: &str,
        stream: &str,
        text: &str,
        seq: i64,
    ) {
        let log_line = NatsLogLine {
            agent_id: self.agent_id.clone(),
            project_id: project_id.to_string(),
            execution_id: execution_id.to_string(),
            stream: stream.to_string(),
            text: text.to_string(),
            timestamp: Utc::now(),
            seq,
        };

        let subject = nats_relay::log_subject(&self.owner_id, &self.agent_id, project_id);
        match serde_json::to_vec(&log_line) {
            Ok(payload) => {
                if let Err(e) = self.jetstream.publish(subject, payload.into()).await {
                    tracing::warn!("Failed to publish NATS log line: {e}");
                }
            }
            Err(e) => {
                tracing::warn!("Failed to serialize NATS log line: {e}");
            }
        }
    }

    pub async fn publish_heartbeat(&self, version: &str, uptime_seconds: u64, cpu_usage: f32, memory_bytes: u64) {
        let hb = NatsHeartbeat {
            agent_id: self.agent_id.clone(),
            version: version.to_string(),
            uptime_seconds,
            cpu_usage,
            memory_bytes,
            timestamp: Utc::now(),
        };

        let subject = nats_relay::heartbeat_subject(&self.owner_id, &self.agent_id);
        match serde_json::to_vec(&hb) {
            Ok(payload) => {
                // Heartbeats are ephemeral — use core NATS, not JetStream
                if let Err(e) = self.client.publish(subject, payload.into()).await {
                    tracing::warn!("Failed to publish heartbeat: {e}");
                }
            }
            Err(e) => {
                tracing::warn!("Failed to serialize heartbeat: {e}");
            }
        }
    }
}

/// Helper to call publish_event on an optional publisher without blocking on None
pub async fn maybe_publish_event(
    publisher: &Option<Arc<NatsPublisher>>,
    event_type: &str,
    project_id: Option<&str>,
    execution_id: Option<&str>,
    data: &str,
) {
    if let Some(pub_ref) = publisher {
        pub_ref.publish_event(event_type, project_id, execution_id, data).await;
    }
}

/// Helper to call publish_log_line on an optional publisher
pub async fn maybe_publish_log_line(
    publisher: &Option<Arc<NatsPublisher>>,
    project_id: &str,
    execution_id: &str,
    stream: &str,
    text: &str,
    seq: i64,
) {
    if let Some(pub_ref) = publisher {
        pub_ref.publish_log_line(project_id, execution_id, stream, text, seq).await;
    }
}

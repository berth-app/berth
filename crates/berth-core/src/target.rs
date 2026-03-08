use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Target {
    pub id: Uuid,
    pub name: String,
    pub kind: TargetKind,
    pub host: Option<String>,
    pub port: u16,
    pub status: TargetStatus,
    pub created_at: DateTime<Utc>,
    pub last_seen_at: Option<DateTime<Utc>>,
    pub agent_version: Option<String>,
    pub nats_agent_id: Option<String>,
    pub nats_enabled: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TargetKind {
    Local,
    Remote,
    Lan,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TargetStatus {
    Online,
    Offline,
    Unknown,
}

impl Target {
    pub fn local() -> Self {
        Self {
            id: Uuid::nil(),
            name: "local".into(),
            kind: TargetKind::Local,
            host: Some("127.0.0.1".into()),
            port: 50051,
            status: TargetStatus::Online,
            created_at: Utc::now(),
            last_seen_at: Some(Utc::now()),
            agent_version: None,
            nats_agent_id: None,
            nats_enabled: false,
        }
    }

    pub fn new_remote(name: String, host: String, port: u16) -> Self {
        Self {
            id: Uuid::new_v4(),
            name,
            kind: TargetKind::Remote,
            host: Some(host),
            port,
            status: TargetStatus::Unknown,
            created_at: Utc::now(),
            last_seen_at: None,
            agent_version: None,
            nats_agent_id: None,
            nats_enabled: false,
        }
    }

    pub fn grpc_endpoint(&self) -> String {
        let host = self.host.as_deref().unwrap_or("127.0.0.1");
        format!("http://{}:{}", host, self.port)
    }
}

use std::collections::HashMap;

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NatsConfig {
    pub url: String,
    pub creds_path: Option<String>,
    pub agent_id: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NatsEvent {
    pub agent_id: String,
    pub event_type: String,
    pub project_id: Option<String>,
    pub execution_id: Option<String>,
    pub data: String,
    pub created_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NatsLogLine {
    pub agent_id: String,
    pub project_id: String,
    pub execution_id: String,
    pub stream: String,
    pub text: String,
    pub timestamp: DateTime<Utc>,
    pub seq: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NatsHeartbeat {
    pub agent_id: String,
    pub version: String,
    pub uptime_seconds: u64,
    pub cpu_usage: f32,
    pub memory_bytes: u64,
    pub timestamp: DateTime<Utc>,
}

pub fn event_subject(agent_id: &str, event_type: &str) -> String {
    let category = match event_type {
        t if t.starts_with("deploy") => "deploy",
        t if t.starts_with("execution") => "execution",
        t if t.starts_with("schedule") => "schedule",
        _ => "agent",
    };
    format!("berth.{agent_id}.event.{category}")
}

pub fn log_subject(agent_id: &str, project_id: &str) -> String {
    format!("berth.{agent_id}.log.{project_id}")
}

pub fn heartbeat_subject(agent_id: &str) -> String {
    format!("berth.{agent_id}.heartbeat")
}

// --- NATS Command Channel Types ---

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NatsCommand {
    pub request_id: String,
    pub reply_to: String,
    pub cmd: NatsCommandKind,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type")]
pub enum NatsCommandKind {
    Health,
    Status,
    Stop { project_id: String },
    Execute {
        project_id: String,
        runtime: String,
        entrypoint: String,
        working_dir: String,
        code: Option<String>,
        image_tag: Option<String>,
        env_vars: HashMap<String, String>,
        container_name: Option<String>,
    },
    Deploy {
        project_id: String,
        runtime: String,
        entrypoint: String,
        containerfile: String,
        version: u32,
        setup_commands: Vec<String>,
        archive_base64: String,
    },
    GetExecutions { project_id: String, limit: u32 },
    GetExecutionLogs { execution_id: String, since_seq: i64 },
    AddSchedule { project_id: String, cron_expr: String },
    RemoveSchedule { schedule_id: String },
    ListSchedules { project_id: String },
    UpgradeDownload {
        version: String,
        download_url: String,
        #[serde(default)]
        github_token: Option<String>,
        checksum_sha256: String,
    },
    DeployChunked {
        project_id: String,
        runtime: String,
        entrypoint: String,
        containerfile: String,
        version: u32,
        setup_commands: Vec<String>,
        total_size: u64,
        chunk_count: u32,
        checksum_sha256: String,
    },
    Rollback,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NatsResponse {
    pub request_id: String,
    pub status: NatsResponseStatus,
    pub body: NatsResponseBody,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum NatsResponseStatus {
    Ok,
    Error(String),
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type")]
pub enum NatsResponseBody {
    Health {
        version: String,
        status: String,
        uptime_seconds: u64,
        podman_version: String,
        container_ready: bool,
        os: String,
        arch: String,
        #[serde(default)]
        probation_status: String,
    },
    Status {
        agent_id: String,
        status: String,
        cpu_usage: f64,
        memory_bytes: u64,
        projects: Vec<NatsProjectStatus>,
    },
    Stop {
        success: bool,
        message: String,
    },
    ExecuteLine {
        stream: String,
        text: String,
        timestamp: String,
        exit_code: i32,
        is_final: bool,
    },
    DeployLine {
        phase: String,
        text: String,
        timestamp: String,
        image_tag: String,
        version: u32,
        is_final: bool,
        success: bool,
    },
    Executions {
        executions: Vec<NatsExecutionInfo>,
    },
    ExecutionLogLine {
        stream: String,
        text: String,
        timestamp: String,
        is_final: bool,
    },
    ScheduleAdded {
        schedule_id: String,
        next_run_at: String,
    },
    ScheduleRemoved {
        success: bool,
    },
    Schedules {
        schedules: Vec<NatsScheduleInfo>,
    },
    DeployReady {
        upload_subject: String,
    },
    UpgradeResult {
        success: bool,
        new_version: String,
        message: String,
    },
    Rollback {
        success: bool,
        restored_version: String,
        message: String,
    },
    Empty,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NatsProjectStatus {
    pub project_id: String,
    pub status: String,
    pub started_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NatsExecutionInfo {
    pub id: String,
    pub project_id: String,
    pub deployment_id: String,
    pub started_at: String,
    pub finished_at: String,
    pub exit_code: i32,
    pub trigger: String,
    pub status: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NatsScheduleInfo {
    pub id: String,
    pub project_id: String,
    pub cron_expr: String,
    pub enabled: bool,
    pub created_at: String,
    pub last_triggered_at: String,
    pub next_run_at: String,
}

pub fn cmd_subject(agent_id: &str, cmd_type: &str) -> String {
    format!("berth.{agent_id}.cmd.{cmd_type}")
}

pub fn resp_subject(agent_id: &str, request_id: &str) -> String {
    format!("berth.{agent_id}.resp.{request_id}")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_event_subjects() {
        assert_eq!(event_subject("vps1", "deploy_completed"), "berth.vps1.event.deploy");
        assert_eq!(event_subject("vps1", "execution_completed"), "berth.vps1.event.execution");
        assert_eq!(event_subject("vps1", "schedule_triggered"), "berth.vps1.event.schedule");
        assert_eq!(event_subject("vps1", "agent_upgraded"), "berth.vps1.event.agent");
    }

    #[test]
    fn test_log_subject() {
        assert_eq!(log_subject("vps1", "proj-123"), "berth.vps1.log.proj-123");
    }

    #[test]
    fn test_heartbeat_subject() {
        assert_eq!(heartbeat_subject("vps1"), "berth.vps1.heartbeat");
    }

    #[test]
    fn test_nats_event_serialization() {
        let event = NatsEvent {
            agent_id: "vps1".into(),
            event_type: "execution_completed".into(),
            project_id: Some("proj-123".into()),
            execution_id: Some("exec-456".into()),
            data: r#"{"exit_code":0}"#.into(),
            created_at: Utc::now(),
        };
        let json = serde_json::to_string(&event).unwrap();
        let parsed: NatsEvent = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed.agent_id, "vps1");
        assert_eq!(parsed.event_type, "execution_completed");
    }
}

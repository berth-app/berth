use std::collections::HashMap;

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NatsConfig {
    pub url: String,
    pub creds_path: Option<String>,
    pub agent_id: String,
    pub owner_id: String,
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

pub fn event_subject(owner_id: &str, agent_id: &str, event_type: &str) -> String {
    let category = match event_type {
        t if t.starts_with("deploy") => "deploy",
        t if t.starts_with("execution") => "execution",
        t if t.starts_with("schedule") => "schedule",
        _ => "agent",
    };
    format!("berth.{owner_id}.{agent_id}.event.{category}")
}

pub fn log_subject(owner_id: &str, agent_id: &str, project_id: &str) -> String {
    format!("berth.{owner_id}.{agent_id}.log.{project_id}")
}

pub fn heartbeat_subject(owner_id: &str, agent_id: &str) -> String {
    format!("berth.{owner_id}.{agent_id}.heartbeat")
}

// --- NATS Command Channel Types ---

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NatsCommand {
    pub request_id: String,
    pub reply_to: String,
    pub cmd: NatsCommandKind,
    /// HMAC-SHA256 signature over the serialized `cmd` field.
    #[serde(default)]
    pub signature: String,
    /// Unique nonce to prevent replay attacks.
    #[serde(default)]
    pub nonce: String,
    /// Unix timestamp (seconds) — commands older than 60s are rejected.
    #[serde(default)]
    pub timestamp: i64,
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
        #[serde(default)]
        run_mode: String,
        #[serde(default)]
        service_port: u16,
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
    Publish {
        project_id: String,
        port: u32,
        provider: String,
        provider_config: String,
    },
    Unpublish {
        project_id: String,
    },
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
        #[serde(default)]
        tunnel_providers: Vec<String>,
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
        /// Token required on each upload chunk to prove authorization.
        #[serde(default)]
        upload_token: String,
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
    Publish {
        success: bool,
        url: String,
        provider: String,
        message: String,
    },
    Unpublish {
        success: bool,
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

pub fn cmd_subject(owner_id: &str, agent_id: &str, cmd_type: &str) -> String {
    format!("berth.{owner_id}.{agent_id}.cmd.{cmd_type}")
}

pub fn resp_subject(owner_id: &str, agent_id: &str, request_id: &str) -> String {
    format!("berth.{owner_id}.{agent_id}.resp.{request_id}")
}

pub fn upload_subject(owner_id: &str, agent_id: &str, request_id: &str) -> String {
    format!("berth.{owner_id}.{agent_id}.upload.{request_id}")
}

// --- Pairing Protocol Types ---

const PAIRING_CODE_CHARSET: &[u8] = b"ABCDEFGHJKLMNPQRSTUVWXYZ23456789";
pub const PAIRING_CODE_LENGTH: usize = 8;
pub const PAIRING_EXPIRY_SECS: u64 = 300; // 5 minutes

pub fn generate_pairing_code() -> String {
    use rand::Rng;
    let mut rng = rand::thread_rng();
    (0..PAIRING_CODE_LENGTH)
        .map(|_| PAIRING_CODE_CHARSET[rng.gen_range(0..PAIRING_CODE_CHARSET.len())] as char)
        .collect()
}

pub fn pairing_advertise_subject(code: &str) -> String {
    format!("berth.pairing.advertise.{code}")
}

pub fn pairing_claim_subject(code: &str) -> String {
    format!("berth.pairing.claim.{code}")
}

pub fn pairing_ack_subject(code: &str) -> String {
    format!("berth.pairing.ack.{code}")
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PairingAdvertisement {
    pub agent_id: String,
    pub code: String,
    pub hostname: String,
    pub os: String,
    pub arch: String,
    pub version: String,
    pub timestamp: DateTime<Utc>,
    /// Random challenge — claimer must HMAC-sign this with the pairing code to prove knowledge.
    #[serde(default)]
    pub challenge: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PairingClaim {
    pub owner_id: String,
    /// HMAC-SHA256(challenge, code) — proves the claimer knows the pairing code.
    #[serde(default)]
    pub challenge_response: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PairingAck {
    pub success: bool,
    pub agent_id: String,
    pub owner_id: String,
    pub message: String,
    /// Hex-encoded 256-bit shared secret for HMAC command signing.
    #[serde(default)]
    pub shared_secret: String,
}

/// Compute the challenge response: HMAC-SHA256(challenge, code_as_key).
pub fn compute_challenge_response(challenge: &str, code: &str) -> String {
    crate::message_auth::sign_command(challenge.as_bytes(), code, 0, code.as_bytes())
}

/// Verify a challenge response.
pub fn verify_challenge_response(challenge: &str, response: &str, code: &str) -> bool {
    let expected = compute_challenge_response(challenge, code);
    crate::message_auth::constant_time_eq_public(expected.as_bytes(), response.as_bytes())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_event_subjects() {
        assert_eq!(event_subject("owner1", "vps1", "deploy_completed"), "berth.owner1.vps1.event.deploy");
        assert_eq!(event_subject("owner1", "vps1", "execution_completed"), "berth.owner1.vps1.event.execution");
        assert_eq!(event_subject("owner1", "vps1", "schedule_triggered"), "berth.owner1.vps1.event.schedule");
        assert_eq!(event_subject("owner1", "vps1", "agent_upgraded"), "berth.owner1.vps1.event.agent");
    }

    #[test]
    fn test_log_subject() {
        assert_eq!(log_subject("owner1", "vps1", "proj-123"), "berth.owner1.vps1.log.proj-123");
    }

    #[test]
    fn test_heartbeat_subject() {
        assert_eq!(heartbeat_subject("owner1", "vps1"), "berth.owner1.vps1.heartbeat");
    }

    #[test]
    fn test_cmd_subject() {
        assert_eq!(cmd_subject("owner1", "vps1", "health"), "berth.owner1.vps1.cmd.health");
    }

    #[test]
    fn test_resp_subject() {
        assert_eq!(resp_subject("owner1", "vps1", "req-123"), "berth.owner1.vps1.resp.req-123");
    }

    #[test]
    fn test_upload_subject() {
        assert_eq!(upload_subject("owner1", "vps1", "req-123"), "berth.owner1.vps1.upload.req-123");
    }

    #[test]
    fn test_pairing_subjects() {
        assert_eq!(pairing_advertise_subject("K7M4XNAB"), "berth.pairing.advertise.K7M4XNAB");
        assert_eq!(pairing_claim_subject("K7M4XNAB"), "berth.pairing.claim.K7M4XNAB");
        assert_eq!(pairing_ack_subject("K7M4XNAB"), "berth.pairing.ack.K7M4XNAB");
    }

    #[test]
    fn test_generate_pairing_code() {
        let code = generate_pairing_code();
        assert_eq!(code.len(), PAIRING_CODE_LENGTH);
        assert_eq!(code.len(), 8);
        for c in code.chars() {
            assert!(PAIRING_CODE_CHARSET.contains(&(c as u8)), "invalid char: {c}");
        }
        // Two codes should be different (probabilistic but 32^8 = ~1.1T)
        let code2 = generate_pairing_code();
        assert_ne!(code, code2);
    }

    #[test]
    fn test_challenge_response() {
        let challenge = "random-challenge-string";
        let code = "K7M4XNAB";
        let response = compute_challenge_response(challenge, code);
        assert!(verify_challenge_response(challenge, &response, code));
        assert!(!verify_challenge_response(challenge, "wrong-response", code));
        assert!(!verify_challenge_response(challenge, &response, "WRONGCOD"));
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

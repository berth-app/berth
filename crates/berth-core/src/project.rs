use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use berth_proto::runtime::Runtime;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RunMode {
    Oneshot,
    Service,
}

impl Default for RunMode {
    fn default() -> Self {
        RunMode::Oneshot
    }
}

impl std::fmt::Display for RunMode {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            RunMode::Oneshot => write!(f, "oneshot"),
            RunMode::Service => write!(f, "service"),
        }
    }
}

impl std::str::FromStr for RunMode {
    type Err = String;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "oneshot" => Ok(RunMode::Oneshot),
            "service" => Ok(RunMode::Service),
            other => Err(format!("Invalid run mode: {other}. Use 'oneshot' or 'service'.")),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Project {
    pub id: Uuid,
    pub name: String,
    pub path: String,
    pub runtime: Runtime,
    pub entrypoint: Option<String>,
    pub status: ProjectStatus,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
    pub last_run_at: Option<DateTime<Utc>>,
    pub last_exit_code: Option<i32>,
    pub run_count: u32,
    pub notify_on_complete: bool,
    pub default_target: Option<String>,
    pub tunnel_url: Option<String>,
    pub tunnel_provider: Option<String>,
    pub run_mode: RunMode,
    pub service_port: Option<u16>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ProjectStatus {
    Idle,
    Running,
    Stopped,
    Failed,
}

impl Project {
    pub fn new(name: String, path: String, runtime: Runtime) -> Self {
        let now = Utc::now();
        Self {
            id: Uuid::new_v4(),
            name,
            path,
            runtime,
            entrypoint: None,
            status: ProjectStatus::Idle,
            created_at: now,
            updated_at: now,
            last_run_at: None,
            last_exit_code: None,
            run_count: 0,
            notify_on_complete: true,
            default_target: None,
            tunnel_url: None,
            tunnel_provider: None,
            run_mode: RunMode::Oneshot,
            service_port: None,
        }
    }
}

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::runtime::Runtime;

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
        }
    }
}

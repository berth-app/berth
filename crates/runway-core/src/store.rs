use anyhow::Result;
use chrono::{DateTime, Utc};
use rusqlite::Connection;
use std::collections::HashMap;
use uuid::Uuid;

use crate::project::{Project, ProjectStatus};
use crate::runtime::Runtime;
use crate::scheduler::Schedule;
use crate::target::{Target, TargetKind, TargetStatus};

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct ExecutionLog {
    pub id: Uuid,
    pub project_id: Uuid,
    pub started_at: DateTime<Utc>,
    pub finished_at: Option<DateTime<Utc>>,
    pub exit_code: Option<i32>,
    pub output: String,
    pub trigger: String, // "manual" or "schedule"
}

impl ExecutionLog {
    pub fn new(project_id: Uuid, trigger: &str) -> Self {
        Self {
            id: Uuid::new_v4(),
            project_id,
            started_at: Utc::now(),
            finished_at: None,
            exit_code: None,
            output: String::new(),
            trigger: trigger.to_string(),
        }
    }
}

pub struct ProjectStore {
    conn: Connection,
}

impl ProjectStore {
    pub fn open(path: &str) -> Result<Self> {
        let conn = Connection::open(path)?;
        let store = Self { conn };
        store.migrate()?;
        Ok(store)
    }

    pub fn open_in_memory() -> Result<Self> {
        let conn = Connection::open_in_memory()?;
        let store = Self { conn };
        store.migrate()?;
        Ok(store)
    }

    fn migrate(&self) -> Result<()> {
        self.conn.execute_batch(
            "CREATE TABLE IF NOT EXISTS projects (
                id TEXT PRIMARY KEY,
                name TEXT NOT NULL,
                path TEXT NOT NULL,
                runtime TEXT NOT NULL,
                entrypoint TEXT,
                status TEXT NOT NULL DEFAULT 'idle',
                created_at TEXT NOT NULL,
                updated_at TEXT NOT NULL
            );",
        )?;

        // Migration: add monitoring columns
        let has_last_run_at = self
            .conn
            .prepare("SELECT last_run_at FROM projects LIMIT 0")
            .is_ok();

        if !has_last_run_at {
            self.conn.execute_batch(
                "ALTER TABLE projects ADD COLUMN last_run_at TEXT;
                 ALTER TABLE projects ADD COLUMN last_exit_code INTEGER;
                 ALTER TABLE projects ADD COLUMN run_count INTEGER NOT NULL DEFAULT 0;",
            )?;
        }

        // Migration: add notify_on_complete column
        let has_notify = self
            .conn
            .prepare("SELECT notify_on_complete FROM projects LIMIT 0")
            .is_ok();

        if !has_notify {
            self.conn.execute_batch(
                "ALTER TABLE projects ADD COLUMN notify_on_complete INTEGER NOT NULL DEFAULT 1;",
            )?;
        }

        self.conn.execute_batch(
            "CREATE TABLE IF NOT EXISTS schedules (
                id TEXT PRIMARY KEY,
                project_id TEXT NOT NULL REFERENCES projects(id) ON DELETE CASCADE,
                cron_expr TEXT NOT NULL,
                enabled INTEGER NOT NULL DEFAULT 1,
                created_at TEXT NOT NULL,
                last_triggered_at TEXT,
                next_run_at TEXT
            );",
        )?;

        self.conn.execute_batch(
            "CREATE TABLE IF NOT EXISTS execution_logs (
                id TEXT PRIMARY KEY,
                project_id TEXT NOT NULL REFERENCES projects(id) ON DELETE CASCADE,
                started_at TEXT NOT NULL,
                finished_at TEXT,
                exit_code INTEGER,
                output TEXT NOT NULL DEFAULT '',
                trigger TEXT NOT NULL DEFAULT 'manual'
            );",
        )?;

        self.conn.execute_batch(
            "CREATE TABLE IF NOT EXISTS settings (
                key TEXT PRIMARY KEY,
                value TEXT NOT NULL
            );
            INSERT OR IGNORE INTO settings (key, value) VALUES ('default_target', 'local');
            INSERT OR IGNORE INTO settings (key, value) VALUES ('auto_run_on_create', 'false');
            INSERT OR IGNORE INTO settings (key, value) VALUES ('log_scrollback_lines', '10000');
            INSERT OR IGNORE INTO settings (key, value) VALUES ('theme', 'system');",
        )?;

        self.conn.execute_batch(
            "CREATE TABLE IF NOT EXISTS targets (
                id TEXT PRIMARY KEY,
                name TEXT NOT NULL UNIQUE,
                kind TEXT NOT NULL DEFAULT 'remote',
                host TEXT,
                port INTEGER NOT NULL DEFAULT 50051,
                status TEXT NOT NULL DEFAULT 'unknown',
                created_at TEXT NOT NULL,
                last_seen_at TEXT,
                agent_version TEXT
            );",
        )?;

        // Migration: add deploy versioning columns
        let has_deploy_version = self
            .conn
            .prepare("SELECT deploy_version FROM projects LIMIT 0")
            .is_ok();

        if !has_deploy_version {
            self.conn.execute_batch(
                "ALTER TABLE projects ADD COLUMN deploy_version INTEGER NOT NULL DEFAULT 0;
                 ALTER TABLE projects ADD COLUMN latest_image_tag TEXT;",
            )?;
        }

        // Migration: add default_target column
        let has_default_target = self
            .conn
            .prepare("SELECT default_target FROM projects LIMIT 0")
            .is_ok();

        if !has_default_target {
            self.conn.execute_batch(
                "ALTER TABLE projects ADD COLUMN default_target TEXT;",
            )?;
        }

        // Migration: add NATS columns to targets
        let has_nats_agent_id = self
            .conn
            .prepare("SELECT nats_agent_id FROM targets LIMIT 0")
            .is_ok();
        if !has_nats_agent_id {
            self.conn.execute_batch(
                "ALTER TABLE targets ADD COLUMN nats_agent_id TEXT;
                 ALTER TABLE targets ADD COLUMN nats_enabled INTEGER NOT NULL DEFAULT 0;",
            )?;
        }

        // Environment variables table
        self.conn.execute_batch(
            "CREATE TABLE IF NOT EXISTS project_env_vars (
                project_id TEXT NOT NULL,
                key TEXT NOT NULL,
                value TEXT NOT NULL,
                PRIMARY KEY (project_id, key)
            );",
        )?;

        Ok(())
    }

    pub fn list(&self) -> Result<Vec<Project>> {
        let mut stmt = self.conn.prepare(
            "SELECT id, name, path, runtime, entrypoint, status, created_at, updated_at, last_run_at, last_exit_code, run_count, notify_on_complete, default_target FROM projects ORDER BY updated_at DESC",
        )?;

        let projects = stmt
            .query_map([], |row| {
                Ok(Project {
                    id: row.get::<_, String>(0)?.parse().unwrap_or_default(),
                    name: row.get(1)?,
                    path: row.get(2)?,
                    runtime: parse_runtime(&row.get::<_, String>(3)?),
                    entrypoint: row.get(4)?,
                    status: parse_status(&row.get::<_, String>(5)?),
                    created_at: row
                        .get::<_, String>(6)?
                        .parse()
                        .unwrap_or_default(),
                    updated_at: row
                        .get::<_, String>(7)?
                        .parse()
                        .unwrap_or_default(),
                    last_run_at: row
                        .get::<_, Option<String>>(8)?
                        .and_then(|s| s.parse().ok()),
                    last_exit_code: row.get(9)?,
                    run_count: row.get::<_, Option<u32>>(10)?.unwrap_or(0),
                    notify_on_complete: row.get::<_, Option<i32>>(11)?.unwrap_or(1) != 0,
                    default_target: row.get(12)?,
                })
            })?
            .collect::<Result<Vec<_>, _>>()?;

        Ok(projects)
    }

    pub fn insert(&self, project: &Project) -> Result<()> {
        self.conn.execute(
            "INSERT INTO projects (id, name, path, runtime, entrypoint, status, created_at, updated_at, last_run_at, last_exit_code, run_count, notify_on_complete)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12)",
            (
                project.id.to_string(),
                &project.name,
                &project.path,
                format_runtime(project.runtime),
                &project.entrypoint,
                format_status(project.status),
                project.created_at.to_rfc3339(),
                project.updated_at.to_rfc3339(),
                project.last_run_at.map(|t| t.to_rfc3339()),
                project.last_exit_code,
                project.run_count,
                project.notify_on_complete as i32,
            ),
        )?;
        Ok(())
    }

    pub fn get(&self, id: Uuid) -> Result<Option<Project>> {
        let mut stmt = self.conn.prepare(
            "SELECT id, name, path, runtime, entrypoint, status, created_at, updated_at, last_run_at, last_exit_code, run_count, notify_on_complete, default_target FROM projects WHERE id = ?1",
        )?;

        let mut rows = stmt.query_map([id.to_string()], |row| {
            Ok(Project {
                id: row.get::<_, String>(0)?.parse().unwrap_or_default(),
                name: row.get(1)?,
                path: row.get(2)?,
                runtime: parse_runtime(&row.get::<_, String>(3)?),
                entrypoint: row.get(4)?,
                status: parse_status(&row.get::<_, String>(5)?),
                created_at: row.get::<_, String>(6)?.parse().unwrap_or_default(),
                updated_at: row.get::<_, String>(7)?.parse().unwrap_or_default(),
                last_run_at: row
                    .get::<_, Option<String>>(8)?
                    .and_then(|s| s.parse().ok()),
                last_exit_code: row.get(9)?,
                run_count: row.get::<_, Option<u32>>(10)?.unwrap_or(0),
                notify_on_complete: row.get::<_, Option<i32>>(11)?.unwrap_or(1) != 0,
                default_target: row.get(12)?,
            })
        })?;

        Ok(rows.next().transpose()?)
    }

    pub fn update_status(&self, id: Uuid, status: ProjectStatus) -> Result<()> {
        self.conn.execute(
            "UPDATE projects SET status = ?1, updated_at = ?2 WHERE id = ?3",
            (
                format_status(status),
                chrono::Utc::now().to_rfc3339(),
                id.to_string(),
            ),
        )?;
        Ok(())
    }

    pub fn record_run_start(&self, id: Uuid) -> Result<()> {
        let now = chrono::Utc::now().to_rfc3339();
        self.conn.execute(
            "UPDATE projects SET status = 'running', last_run_at = ?1, run_count = run_count + 1, updated_at = ?1 WHERE id = ?2",
            (&now, id.to_string()),
        )?;
        Ok(())
    }

    pub fn record_run_end(&self, id: Uuid, exit_code: Option<i32>) -> Result<()> {
        let status = match exit_code {
            Some(0) => "idle",
            _ => "failed",
        };
        let now = chrono::Utc::now().to_rfc3339();
        self.conn.execute(
            "UPDATE projects SET status = ?1, last_exit_code = ?2, updated_at = ?3 WHERE id = ?4",
            (status, exit_code, &now, id.to_string()),
        )?;
        Ok(())
    }

    pub fn update_project(&self, id: Uuid, name: &str, entrypoint: Option<&str>) -> Result<()> {
        let now = chrono::Utc::now().to_rfc3339();
        self.conn.execute(
            "UPDATE projects SET name = ?1, entrypoint = ?2, updated_at = ?3 WHERE id = ?4",
            (name, entrypoint, &now, id.to_string()),
        )?;
        Ok(())
    }

    pub fn set_project_notify(&self, id: Uuid, enabled: bool) -> Result<()> {
        let now = chrono::Utc::now().to_rfc3339();
        self.conn.execute(
            "UPDATE projects SET notify_on_complete = ?1, updated_at = ?2 WHERE id = ?3",
            (enabled as i32, &now, id.to_string()),
        )?;
        Ok(())
    }

    pub fn set_project_target(&self, id: Uuid, target_id: Option<&str>) -> Result<()> {
        let now = chrono::Utc::now().to_rfc3339();
        self.conn.execute(
            "UPDATE projects SET default_target = ?1, updated_at = ?2 WHERE id = ?3",
            (target_id, &now, id.to_string()),
        )?;
        Ok(())
    }

    pub fn delete(&self, id: Uuid) -> Result<()> {
        self.conn
            .execute("DELETE FROM projects WHERE id = ?1", [id.to_string()])?;
        Ok(())
    }

    // --- Schedule methods ---

    pub fn insert_schedule(&self, schedule: &Schedule) -> Result<()> {
        self.conn.execute(
            "INSERT INTO schedules (id, project_id, cron_expr, enabled, created_at, last_triggered_at, next_run_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
            (
                schedule.id.to_string(),
                schedule.project_id.to_string(),
                &schedule.cron_expr,
                schedule.enabled as i32,
                schedule.created_at.to_rfc3339(),
                schedule.last_triggered_at.map(|t| t.to_rfc3339()),
                schedule.next_run_at.map(|t| t.to_rfc3339()),
            ),
        )?;
        Ok(())
    }

    pub fn list_schedules(&self) -> Result<Vec<Schedule>> {
        let mut stmt = self.conn.prepare(
            "SELECT id, project_id, cron_expr, enabled, created_at, last_triggered_at, next_run_at FROM schedules ORDER BY created_at DESC",
        )?;

        let schedules = stmt
            .query_map([], |row| {
                Ok(Schedule {
                    id: row.get::<_, String>(0)?.parse().unwrap_or_default(),
                    project_id: row.get::<_, String>(1)?.parse().unwrap_or_default(),
                    cron_expr: row.get(2)?,
                    enabled: row.get::<_, i32>(3)? != 0,
                    created_at: row
                        .get::<_, String>(4)?
                        .parse()
                        .unwrap_or_default(),
                    last_triggered_at: row
                        .get::<_, Option<String>>(5)?
                        .and_then(|s| s.parse().ok()),
                    next_run_at: row
                        .get::<_, Option<String>>(6)?
                        .and_then(|s| s.parse().ok()),
                })
            })?
            .collect::<Result<Vec<_>, _>>()?;

        Ok(schedules)
    }

    pub fn get_schedules_for_project(&self, project_id: Uuid) -> Result<Vec<Schedule>> {
        let mut stmt = self.conn.prepare(
            "SELECT id, project_id, cron_expr, enabled, created_at, last_triggered_at, next_run_at FROM schedules WHERE project_id = ?1",
        )?;

        let schedules = stmt
            .query_map([project_id.to_string()], |row| {
                Ok(Schedule {
                    id: row.get::<_, String>(0)?.parse().unwrap_or_default(),
                    project_id: row.get::<_, String>(1)?.parse().unwrap_or_default(),
                    cron_expr: row.get(2)?,
                    enabled: row.get::<_, i32>(3)? != 0,
                    created_at: row
                        .get::<_, String>(4)?
                        .parse()
                        .unwrap_or_default(),
                    last_triggered_at: row
                        .get::<_, Option<String>>(5)?
                        .and_then(|s| s.parse().ok()),
                    next_run_at: row
                        .get::<_, Option<String>>(6)?
                        .and_then(|s| s.parse().ok()),
                })
            })?
            .collect::<Result<Vec<_>, _>>()?;

        Ok(schedules)
    }

    pub fn update_schedule_after_run(
        &self,
        id: Uuid,
        triggered_at: DateTime<Utc>,
        next_run_at: Option<DateTime<Utc>>,
    ) -> Result<()> {
        self.conn.execute(
            "UPDATE schedules SET last_triggered_at = ?1, next_run_at = ?2 WHERE id = ?3",
            (
                triggered_at.to_rfc3339(),
                next_run_at.map(|t| t.to_rfc3339()),
                id.to_string(),
            ),
        )?;
        Ok(())
    }

    pub fn set_schedule_enabled(&self, id: Uuid, enabled: bool) -> Result<()> {
        self.conn.execute(
            "UPDATE schedules SET enabled = ?1 WHERE id = ?2",
            (enabled as i32, id.to_string()),
        )?;
        Ok(())
    }

    pub fn delete_schedule(&self, id: Uuid) -> Result<()> {
        self.conn
            .execute("DELETE FROM schedules WHERE id = ?1", [id.to_string()])?;
        Ok(())
    }

    // --- Target methods ---

    pub fn insert_target(&self, target: &Target) -> Result<()> {
        self.conn.execute(
            "INSERT INTO targets (id, name, kind, host, port, status, created_at, last_seen_at, agent_version, nats_agent_id, nats_enabled)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11)",
            (
                target.id.to_string(),
                &target.name,
                format_target_kind(target.kind),
                &target.host,
                target.port as i32,
                format_target_status(target.status),
                target.created_at.to_rfc3339(),
                target.last_seen_at.map(|t| t.to_rfc3339()),
                &target.agent_version,
                &target.nats_agent_id,
                target.nats_enabled as i32,
            ),
        )?;
        Ok(())
    }

    pub fn list_targets(&self) -> Result<Vec<Target>> {
        let mut stmt = self.conn.prepare(
            "SELECT id, name, kind, host, port, status, created_at, last_seen_at, agent_version, nats_agent_id, nats_enabled FROM targets ORDER BY name ASC",
        )?;

        let targets = stmt
            .query_map([], |row| {
                Ok(Target {
                    id: row.get::<_, String>(0)?.parse().unwrap_or_default(),
                    name: row.get(1)?,
                    kind: parse_target_kind(&row.get::<_, String>(2)?),
                    host: row.get(3)?,
                    port: row.get::<_, i32>(4)? as u16,
                    status: parse_target_status(&row.get::<_, String>(5)?),
                    created_at: row.get::<_, String>(6)?.parse().unwrap_or_default(),
                    last_seen_at: row
                        .get::<_, Option<String>>(7)?
                        .and_then(|s| s.parse().ok()),
                    agent_version: row.get(8)?,
                    nats_agent_id: row.get(9)?,
                    nats_enabled: row.get::<_, i32>(10).unwrap_or(0) != 0,
                })
            })?
            .collect::<Result<Vec<_>, _>>()?;

        Ok(targets)
    }

    pub fn get_target_by_name(&self, name: &str) -> Result<Option<Target>> {
        let mut stmt = self.conn.prepare(
            "SELECT id, name, kind, host, port, status, created_at, last_seen_at, agent_version, nats_agent_id, nats_enabled FROM targets WHERE name = ?1",
        )?;

        let mut rows = stmt.query_map([name], |row| {
            Ok(Target {
                id: row.get::<_, String>(0)?.parse().unwrap_or_default(),
                name: row.get(1)?,
                kind: parse_target_kind(&row.get::<_, String>(2)?),
                host: row.get(3)?,
                port: row.get::<_, i32>(4)? as u16,
                status: parse_target_status(&row.get::<_, String>(5)?),
                created_at: row.get::<_, String>(6)?.parse().unwrap_or_default(),
                last_seen_at: row
                    .get::<_, Option<String>>(7)?
                    .and_then(|s| s.parse().ok()),
                agent_version: row.get(8)?,
                nats_agent_id: row.get(9)?,
                nats_enabled: row.get::<_, i32>(10).unwrap_or(0) != 0,
            })
        })?;

        Ok(rows.next().transpose()?)
    }

    pub fn update_target_status(
        &self,
        id: Uuid,
        status: TargetStatus,
        agent_version: Option<&str>,
    ) -> Result<()> {
        let now = chrono::Utc::now().to_rfc3339();
        self.conn.execute(
            "UPDATE targets SET status = ?1, last_seen_at = ?2, agent_version = ?3 WHERE id = ?4",
            (
                format_target_status(status),
                &now,
                agent_version,
                id.to_string(),
            ),
        )?;
        Ok(())
    }

    pub fn update_target_nats(&self, id: Uuid, agent_id: Option<&str>, enabled: bool) -> Result<()> {
        self.conn.execute(
            "UPDATE targets SET nats_agent_id = ?1, nats_enabled = ?2 WHERE id = ?3",
            (agent_id, enabled as i32, id.to_string()),
        )?;
        Ok(())
    }

    pub fn delete_target(&self, id: Uuid) -> Result<()> {
        self.conn
            .execute("DELETE FROM targets WHERE id = ?1", [id.to_string()])?;
        Ok(())
    }

    // --- Execution log methods ---

    pub fn insert_execution_log(&self, log: &ExecutionLog) -> Result<()> {
        self.conn.execute(
            "INSERT INTO execution_logs (id, project_id, started_at, finished_at, exit_code, output, trigger)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
            (
                log.id.to_string(),
                log.project_id.to_string(),
                log.started_at.to_rfc3339(),
                log.finished_at.map(|t| t.to_rfc3339()),
                log.exit_code,
                &log.output,
                &log.trigger,
            ),
        )?;
        Ok(())
    }

    pub fn finish_execution_log(
        &self,
        id: Uuid,
        exit_code: i32,
        output: &str,
    ) -> Result<()> {
        let now = chrono::Utc::now().to_rfc3339();
        self.conn.execute(
            "UPDATE execution_logs SET finished_at = ?1, exit_code = ?2, output = ?3 WHERE id = ?4",
            (&now, exit_code, output, id.to_string()),
        )?;
        Ok(())
    }

    pub fn append_execution_output(&self, id: Uuid, text: &str) -> Result<()> {
        self.conn.execute(
            "UPDATE execution_logs SET output = output || ?1 WHERE id = ?2",
            (text, id.to_string()),
        )?;
        Ok(())
    }

    pub fn list_execution_logs(&self, project_id: Uuid, limit: u32) -> Result<Vec<ExecutionLog>> {
        let mut stmt = self.conn.prepare(
            "SELECT id, project_id, started_at, finished_at, exit_code, output, trigger
             FROM execution_logs WHERE project_id = ?1
             ORDER BY started_at DESC LIMIT ?2",
        )?;

        let logs = stmt
            .query_map(rusqlite::params![project_id.to_string(), limit], |row| {
                Ok(ExecutionLog {
                    id: row.get::<_, String>(0)?.parse().unwrap_or_default(),
                    project_id: row.get::<_, String>(1)?.parse().unwrap_or_default(),
                    started_at: row.get::<_, String>(2)?.parse().unwrap_or_default(),
                    finished_at: row
                        .get::<_, Option<String>>(3)?
                        .and_then(|s| s.parse().ok()),
                    exit_code: row.get(4)?,
                    output: row.get(5)?,
                    trigger: row.get(6)?,
                })
            })?
            .collect::<Result<Vec<_>, _>>()?;

        Ok(logs)
    }

    // --- Settings methods ---

    pub fn get_all_settings(&self) -> Result<HashMap<String, String>> {
        let mut stmt = self.conn.prepare("SELECT key, value FROM settings")?;
        let map = stmt
            .query_map([], |row| Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?)))?
            .collect::<Result<HashMap<_, _>, _>>()?;
        Ok(map)
    }

    pub fn set_setting(&self, key: &str, value: &str) -> Result<()> {
        self.conn.execute(
            "INSERT OR REPLACE INTO settings (key, value) VALUES (?1, ?2)",
            (key, value),
        )?;
        Ok(())
    }

    // --- Deploy version methods ---

    pub fn increment_deploy_version(&self, project_id: Uuid) -> Result<u32> {
        self.conn.execute(
            "UPDATE projects SET deploy_version = deploy_version + 1 WHERE id = ?1",
            [project_id.to_string()],
        )?;
        let version: u32 = self.conn.query_row(
            "SELECT deploy_version FROM projects WHERE id = ?1",
            [project_id.to_string()],
            |row| row.get(0),
        )?;
        Ok(version)
    }

    pub fn set_latest_image_tag(&self, project_id: Uuid, tag: &str) -> Result<()> {
        self.conn.execute(
            "UPDATE projects SET latest_image_tag = ?2 WHERE id = ?1",
            (project_id.to_string(), tag),
        )?;
        Ok(())
    }

    pub fn get_latest_image_tag(&self, project_id: Uuid) -> Result<Option<String>> {
        let tag = self.conn.query_row(
            "SELECT latest_image_tag FROM projects WHERE id = ?1",
            [project_id.to_string()],
            |row| row.get(0),
        )?;
        Ok(tag)
    }

    // --- Environment variable methods ---

    pub fn set_env_var(&self, project_id: Uuid, key: &str, value: &str) -> Result<()> {
        self.conn.execute(
            "INSERT OR REPLACE INTO project_env_vars (project_id, key, value) VALUES (?1, ?2, ?3)",
            (project_id.to_string(), key, value),
        )?;
        Ok(())
    }

    pub fn get_env_vars(&self, project_id: Uuid) -> Result<HashMap<String, String>> {
        let mut stmt = self.conn.prepare(
            "SELECT key, value FROM project_env_vars WHERE project_id = ?1",
        )?;
        let map = stmt
            .query_map([project_id.to_string()], |row| {
                Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
            })?
            .collect::<Result<HashMap<_, _>, _>>()?;
        Ok(map)
    }

    pub fn delete_env_var(&self, project_id: Uuid, key: &str) -> Result<()> {
        self.conn.execute(
            "DELETE FROM project_env_vars WHERE project_id = ?1 AND key = ?2",
            (project_id.to_string(), key),
        )?;
        Ok(())
    }
}

fn parse_runtime(s: &str) -> Runtime {
    match s {
        "python" => Runtime::Python,
        "node" => Runtime::Node,
        "go" => Runtime::Go,
        "rust" => Runtime::Rust,
        "shell" => Runtime::Shell,
        _ => Runtime::Unknown,
    }
}

fn format_runtime(r: Runtime) -> &'static str {
    match r {
        Runtime::Python => "python",
        Runtime::Node => "node",
        Runtime::Go => "go",
        Runtime::Rust => "rust",
        Runtime::Shell => "shell",
        Runtime::Unknown => "unknown",
    }
}

fn parse_status(s: &str) -> ProjectStatus {
    match s {
        "idle" => ProjectStatus::Idle,
        "running" => ProjectStatus::Running,
        "stopped" => ProjectStatus::Stopped,
        "failed" => ProjectStatus::Failed,
        _ => ProjectStatus::Idle,
    }
}

fn format_status(s: ProjectStatus) -> &'static str {
    match s {
        ProjectStatus::Idle => "idle",
        ProjectStatus::Running => "running",
        ProjectStatus::Stopped => "stopped",
        ProjectStatus::Failed => "failed",
    }
}

fn parse_target_kind(s: &str) -> TargetKind {
    match s {
        "local" => TargetKind::Local,
        "remote" => TargetKind::Remote,
        "lan" => TargetKind::Lan,
        _ => TargetKind::Remote,
    }
}

fn format_target_kind(k: TargetKind) -> &'static str {
    match k {
        TargetKind::Local => "local",
        TargetKind::Remote => "remote",
        TargetKind::Lan => "lan",
    }
}

fn parse_target_status(s: &str) -> TargetStatus {
    match s {
        "online" => TargetStatus::Online,
        "offline" => TargetStatus::Offline,
        _ => TargetStatus::Unknown,
    }
}

fn format_target_status(s: TargetStatus) -> &'static str {
    match s {
        TargetStatus::Online => "online",
        TargetStatus::Offline => "offline",
        TargetStatus::Unknown => "unknown",
    }
}

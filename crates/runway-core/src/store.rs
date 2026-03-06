use anyhow::Result;
use chrono::{DateTime, Utc};
use rusqlite::Connection;
use uuid::Uuid;

use crate::project::{Project, ProjectStatus};
use crate::runtime::Runtime;
use crate::scheduler::Schedule;
use crate::target::{Target, TargetKind, TargetStatus};

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

        Ok(())
    }

    pub fn list(&self) -> Result<Vec<Project>> {
        let mut stmt = self.conn.prepare(
            "SELECT id, name, path, runtime, entrypoint, status, created_at, updated_at, last_run_at, last_exit_code, run_count FROM projects ORDER BY updated_at DESC",
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
                })
            })?
            .collect::<Result<Vec<_>, _>>()?;

        Ok(projects)
    }

    pub fn insert(&self, project: &Project) -> Result<()> {
        self.conn.execute(
            "INSERT INTO projects (id, name, path, runtime, entrypoint, status, created_at, updated_at, last_run_at, last_exit_code, run_count)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11)",
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
            ),
        )?;
        Ok(())
    }

    pub fn get(&self, id: Uuid) -> Result<Option<Project>> {
        let mut stmt = self.conn.prepare(
            "SELECT id, name, path, runtime, entrypoint, status, created_at, updated_at, last_run_at, last_exit_code, run_count FROM projects WHERE id = ?1",
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
            "INSERT INTO targets (id, name, kind, host, port, status, created_at, last_seen_at, agent_version)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)",
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
            ),
        )?;
        Ok(())
    }

    pub fn list_targets(&self) -> Result<Vec<Target>> {
        let mut stmt = self.conn.prepare(
            "SELECT id, name, kind, host, port, status, created_at, last_seen_at, agent_version FROM targets ORDER BY name ASC",
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
                })
            })?
            .collect::<Result<Vec<_>, _>>()?;

        Ok(targets)
    }

    pub fn get_target_by_name(&self, name: &str) -> Result<Option<Target>> {
        let mut stmt = self.conn.prepare(
            "SELECT id, name, kind, host, port, status, created_at, last_seen_at, agent_version FROM targets WHERE name = ?1",
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

    pub fn delete_target(&self, id: Uuid) -> Result<()> {
        self.conn
            .execute("DELETE FROM targets WHERE id = ?1", [id.to_string()])?;
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

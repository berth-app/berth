use anyhow::Result;
use rusqlite::Connection;
use uuid::Uuid;

use crate::project::{Project, ProjectStatus};
use crate::runtime::Runtime;

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
        Ok(())
    }

    pub fn list(&self) -> Result<Vec<Project>> {
        let mut stmt = self.conn.prepare(
            "SELECT id, name, path, runtime, entrypoint, status, created_at, updated_at FROM projects ORDER BY updated_at DESC",
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
                })
            })?
            .collect::<Result<Vec<_>, _>>()?;

        Ok(projects)
    }

    pub fn insert(&self, project: &Project) -> Result<()> {
        self.conn.execute(
            "INSERT INTO projects (id, name, path, runtime, entrypoint, status, created_at, updated_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
            (
                project.id.to_string(),
                &project.name,
                &project.path,
                format_runtime(project.runtime),
                &project.entrypoint,
                format_status(project.status),
                project.created_at.to_rfc3339(),
                project.updated_at.to_rfc3339(),
            ),
        )?;
        Ok(())
    }

    pub fn get(&self, id: Uuid) -> Result<Option<Project>> {
        let mut stmt = self.conn.prepare(
            "SELECT id, name, path, runtime, entrypoint, status, created_at, updated_at FROM projects WHERE id = ?1",
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

    pub fn delete(&self, id: Uuid) -> Result<()> {
        self.conn
            .execute("DELETE FROM projects WHERE id = ?1", [id.to_string()])?;
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

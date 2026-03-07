use anyhow::Result;
use chrono::{DateTime, Utc};
use rusqlite::Connection;

pub struct AgentStore {
    conn: Connection,
}

#[derive(Debug, Clone)]
pub struct Deployment {
    pub id: String,
    pub project_id: String,
    pub version: u32,
    pub runtime: String,
    pub entrypoint: String,
    pub working_dir: String,
    pub image_tag: Option<String>,
    pub status: String,
    pub deployed_at: DateTime<Utc>,
}

#[derive(Debug, Clone)]
pub struct Execution {
    pub id: String,
    pub project_id: String,
    pub deployment_id: Option<String>,
    pub started_at: DateTime<Utc>,
    pub finished_at: Option<DateTime<Utc>>,
    pub exit_code: Option<i32>,
    pub trigger: String,
    pub status: String,
}

#[derive(Debug, Clone)]
pub struct ExecLogLine {
    pub id: i64,
    pub execution_id: String,
    pub seq: i64,
    pub stream: String,
    pub text: String,
    pub timestamp: DateTime<Utc>,
}

#[derive(Debug, Clone)]
pub struct AgentEvent {
    pub id: i64,
    pub event_type: String,
    pub project_id: Option<String>,
    pub execution_id: Option<String>,
    pub data: String,
    pub created_at: DateTime<Utc>,
}

#[derive(Debug, Clone)]
pub struct AgentSchedule {
    pub id: String,
    pub project_id: String,
    pub cron_expr: String,
    pub enabled: bool,
    pub created_at: DateTime<Utc>,
    pub last_triggered_at: Option<DateTime<Utc>>,
    pub next_run_at: Option<DateTime<Utc>>,
}

impl AgentStore {
    pub fn open(path: &str) -> Result<Self> {
        let conn = Connection::open(path)?;
        conn.execute_batch("PRAGMA journal_mode=WAL; PRAGMA busy_timeout=5000;")?;
        let store = Self { conn };
        store.migrate()?;
        Ok(store)
    }

    fn migrate(&self) -> Result<()> {
        self.conn.execute_batch(
            "CREATE TABLE IF NOT EXISTS deployments (
                id TEXT PRIMARY KEY,
                project_id TEXT NOT NULL,
                version INTEGER NOT NULL,
                runtime TEXT NOT NULL,
                entrypoint TEXT NOT NULL,
                working_dir TEXT NOT NULL,
                image_tag TEXT,
                status TEXT NOT NULL DEFAULT 'deployed',
                deployed_at TEXT NOT NULL,
                UNIQUE(project_id, version)
            );

            CREATE TABLE IF NOT EXISTS executions (
                id TEXT PRIMARY KEY,
                project_id TEXT NOT NULL,
                deployment_id TEXT,
                started_at TEXT NOT NULL,
                finished_at TEXT,
                exit_code INTEGER,
                trigger TEXT NOT NULL DEFAULT 'manual',
                status TEXT NOT NULL DEFAULT 'running'
            );

            CREATE TABLE IF NOT EXISTS execution_logs (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                execution_id TEXT NOT NULL,
                seq INTEGER NOT NULL,
                stream TEXT NOT NULL,
                text TEXT NOT NULL,
                timestamp TEXT NOT NULL
            );
            CREATE INDEX IF NOT EXISTS idx_exec_logs_exec_seq ON execution_logs(execution_id, seq);

            CREATE TABLE IF NOT EXISTS events (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                event_type TEXT NOT NULL,
                project_id TEXT,
                execution_id TEXT,
                data TEXT NOT NULL,
                created_at TEXT NOT NULL
            );

            CREATE TABLE IF NOT EXISTS schedules (
                id TEXT PRIMARY KEY,
                project_id TEXT NOT NULL,
                cron_expr TEXT NOT NULL,
                enabled INTEGER NOT NULL DEFAULT 1,
                created_at TEXT NOT NULL,
                last_triggered_at TEXT,
                next_run_at TEXT
            );",
        )?;
        Ok(())
    }

    pub fn vacuum(&self) -> Result<()> {
        self.conn.execute_batch("VACUUM;")?;
        Ok(())
    }

    // --- Deployments ---

    pub fn insert_deployment(&self, d: &Deployment) -> Result<()> {
        self.conn.execute(
            "INSERT OR REPLACE INTO deployments (id, project_id, version, runtime, entrypoint, working_dir, image_tag, status, deployed_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)",
            (
                &d.id,
                &d.project_id,
                d.version,
                &d.runtime,
                &d.entrypoint,
                &d.working_dir,
                &d.image_tag,
                &d.status,
                d.deployed_at.to_rfc3339(),
            ),
        )?;
        Ok(())
    }

    pub fn get_latest_deployment(&self, project_id: &str) -> Result<Option<Deployment>> {
        let mut stmt = self.conn.prepare(
            "SELECT id, project_id, version, runtime, entrypoint, working_dir, image_tag, status, deployed_at
             FROM deployments WHERE project_id = ?1 ORDER BY version DESC LIMIT 1",
        )?;

        let mut rows = stmt.query_map([project_id], |row| {
            Ok(Deployment {
                id: row.get(0)?,
                project_id: row.get(1)?,
                version: row.get(2)?,
                runtime: row.get(3)?,
                entrypoint: row.get(4)?,
                working_dir: row.get(5)?,
                image_tag: row.get(6)?,
                status: row.get(7)?,
                deployed_at: row
                    .get::<_, String>(8)?
                    .parse()
                    .unwrap_or_else(|_| Utc::now()),
            })
        })?;

        Ok(rows.next().transpose()?)
    }

    pub fn prune_old_deployments(&self, project_id: &str, keep: u32) -> Result<u32> {
        let count: u32 = self.conn.query_row(
            "SELECT COUNT(*) FROM deployments WHERE project_id = ?1",
            [project_id],
            |row| row.get(0),
        )?;
        if count <= keep {
            return Ok(0);
        }
        let to_delete = count - keep;
        let deleted = self.conn.execute(
            "DELETE FROM deployments WHERE id IN (
                SELECT id FROM deployments WHERE project_id = ?1 ORDER BY version ASC LIMIT ?2
            )",
            rusqlite::params![project_id, to_delete],
        )?;
        Ok(deleted as u32)
    }

    // --- Executions ---

    pub fn insert_execution(&self, e: &Execution) -> Result<()> {
        self.conn.execute(
            "INSERT INTO executions (id, project_id, deployment_id, started_at, finished_at, exit_code, trigger, status)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
            (
                &e.id,
                &e.project_id,
                &e.deployment_id,
                e.started_at.to_rfc3339(),
                e.finished_at.map(|t| t.to_rfc3339()),
                e.exit_code,
                &e.trigger,
                &e.status,
            ),
        )?;
        Ok(())
    }

    pub fn finish_execution(&self, id: &str, exit_code: i32, status: &str) -> Result<()> {
        let now = Utc::now().to_rfc3339();
        self.conn.execute(
            "UPDATE executions SET finished_at = ?1, exit_code = ?2, status = ?3 WHERE id = ?4",
            (&now, exit_code, status, id),
        )?;
        Ok(())
    }

    pub fn list_executions(&self, project_id: &str, limit: u32) -> Result<Vec<Execution>> {
        let limit = if limit == 0 { 50 } else { limit };
        let (sql, params): (&str, Vec<Box<dyn rusqlite::types::ToSql>>) = if project_id.is_empty() {
            (
                "SELECT id, project_id, deployment_id, started_at, finished_at, exit_code, trigger, status
                 FROM executions ORDER BY started_at DESC LIMIT ?1",
                vec![Box::new(limit)],
            )
        } else {
            (
                "SELECT id, project_id, deployment_id, started_at, finished_at, exit_code, trigger, status
                 FROM executions WHERE project_id = ?1 ORDER BY started_at DESC LIMIT ?2",
                vec![Box::new(project_id.to_string()), Box::new(limit)],
            )
        };

        let mut stmt = self.conn.prepare(sql)?;
        let params_refs: Vec<&dyn rusqlite::types::ToSql> = params.iter().map(|p| p.as_ref()).collect();
        let executions = stmt
            .query_map(params_refs.as_slice(), |row| {
                Ok(Execution {
                    id: row.get(0)?,
                    project_id: row.get(1)?,
                    deployment_id: row.get(2)?,
                    started_at: row
                        .get::<_, String>(3)?
                        .parse()
                        .unwrap_or_else(|_| Utc::now()),
                    finished_at: row
                        .get::<_, Option<String>>(4)?
                        .and_then(|s| s.parse().ok()),
                    exit_code: row.get(5)?,
                    trigger: row.get(6)?,
                    status: row.get(7)?,
                })
            })?
            .collect::<Result<Vec<_>, _>>()?;

        Ok(executions)
    }

    // --- Execution Logs ---

    pub fn append_log_line(
        &self,
        execution_id: &str,
        seq: i64,
        stream: &str,
        text: &str,
        timestamp: &DateTime<Utc>,
    ) -> Result<()> {
        self.conn.execute(
            "INSERT INTO execution_logs (execution_id, seq, stream, text, timestamp) VALUES (?1, ?2, ?3, ?4, ?5)",
            (execution_id, seq, stream, text, timestamp.to_rfc3339()),
        )?;
        Ok(())
    }

    pub fn get_logs(&self, execution_id: &str, since_seq: i64) -> Result<Vec<ExecLogLine>> {
        let mut stmt = self.conn.prepare(
            "SELECT id, execution_id, seq, stream, text, timestamp
             FROM execution_logs WHERE execution_id = ?1 AND seq > ?2
             ORDER BY seq ASC LIMIT 50000",
        )?;

        let logs = stmt
            .query_map(rusqlite::params![execution_id, since_seq], |row| {
                Ok(ExecLogLine {
                    id: row.get(0)?,
                    execution_id: row.get(1)?,
                    seq: row.get(2)?,
                    stream: row.get(3)?,
                    text: row.get(4)?,
                    timestamp: row
                        .get::<_, String>(5)?
                        .parse()
                        .unwrap_or_else(|_| Utc::now()),
                })
            })?
            .collect::<Result<Vec<_>, _>>()?;

        Ok(logs)
    }

    // --- Events ---

    pub fn insert_event(
        &self,
        event_type: &str,
        project_id: Option<&str>,
        execution_id: Option<&str>,
        data: &str,
    ) -> Result<i64> {
        let now = Utc::now().to_rfc3339();
        self.conn.execute(
            "INSERT INTO events (event_type, project_id, execution_id, data, created_at) VALUES (?1, ?2, ?3, ?4, ?5)",
            (event_type, project_id, execution_id, data, &now),
        )?;
        Ok(self.conn.last_insert_rowid())
    }

    pub fn get_events_since(&self, since_id: i64, limit: u32) -> Result<Vec<AgentEvent>> {
        let limit = if limit == 0 { 100 } else { limit };
        let mut stmt = self.conn.prepare(
            "SELECT id, event_type, project_id, execution_id, data, created_at
             FROM events WHERE id > ?1 ORDER BY id ASC LIMIT ?2",
        )?;

        let events = stmt
            .query_map(rusqlite::params![since_id, limit], |row| {
                Ok(AgentEvent {
                    id: row.get(0)?,
                    event_type: row.get(1)?,
                    project_id: row.get(2)?,
                    execution_id: row.get(3)?,
                    data: row.get(4)?,
                    created_at: row
                        .get::<_, String>(5)?
                        .parse()
                        .unwrap_or_else(|_| Utc::now()),
                })
            })?
            .collect::<Result<Vec<_>, _>>()?;

        Ok(events)
    }

    pub fn prune_events(&self, up_to_id: i64) -> Result<i64> {
        let seven_days_ago = (Utc::now() - chrono::Duration::days(7)).to_rfc3339();
        let deleted = self.conn.execute(
            "DELETE FROM events WHERE id <= ?1 AND created_at < ?2",
            rusqlite::params![up_to_id, seven_days_ago],
        )?;
        Ok(deleted as i64)
    }

    pub fn prune_events_hard_cap(&self, max_events: i64) -> Result<()> {
        self.conn.execute(
            "DELETE FROM events WHERE id NOT IN (SELECT id FROM events ORDER BY id DESC LIMIT ?1)",
            [max_events],
        )?;
        Ok(())
    }

    // --- Schedules ---

    pub fn insert_schedule(&self, s: &AgentSchedule) -> Result<()> {
        self.conn.execute(
            "INSERT INTO schedules (id, project_id, cron_expr, enabled, created_at, last_triggered_at, next_run_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
            (
                &s.id,
                &s.project_id,
                &s.cron_expr,
                s.enabled as i32,
                s.created_at.to_rfc3339(),
                s.last_triggered_at.map(|t| t.to_rfc3339()),
                s.next_run_at.map(|t| t.to_rfc3339()),
            ),
        )?;
        Ok(())
    }

    pub fn list_schedules(&self, project_id: &str) -> Result<Vec<AgentSchedule>> {
        let (sql, params): (&str, Vec<Box<dyn rusqlite::types::ToSql>>) = if project_id.is_empty() {
            (
                "SELECT id, project_id, cron_expr, enabled, created_at, last_triggered_at, next_run_at FROM schedules ORDER BY created_at DESC",
                vec![],
            )
        } else {
            (
                "SELECT id, project_id, cron_expr, enabled, created_at, last_triggered_at, next_run_at FROM schedules WHERE project_id = ?1 ORDER BY created_at DESC",
                vec![Box::new(project_id.to_string())],
            )
        };

        let mut stmt = self.conn.prepare(sql)?;
        let params_refs: Vec<&dyn rusqlite::types::ToSql> = params.iter().map(|p| p.as_ref()).collect();
        let schedules = stmt
            .query_map(params_refs.as_slice(), |row| {
                Ok(AgentSchedule {
                    id: row.get(0)?,
                    project_id: row.get(1)?,
                    cron_expr: row.get(2)?,
                    enabled: row.get::<_, i32>(3)? != 0,
                    created_at: row
                        .get::<_, String>(4)?
                        .parse()
                        .unwrap_or_else(|_| Utc::now()),
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

    pub fn list_due_schedules(&self) -> Result<Vec<AgentSchedule>> {
        let now = Utc::now().to_rfc3339();
        let mut stmt = self.conn.prepare(
            "SELECT id, project_id, cron_expr, enabled, created_at, last_triggered_at, next_run_at
             FROM schedules WHERE enabled = 1 AND next_run_at IS NOT NULL AND next_run_at <= ?1",
        )?;

        let schedules = stmt
            .query_map([&now], |row| {
                Ok(AgentSchedule {
                    id: row.get(0)?,
                    project_id: row.get(1)?,
                    cron_expr: row.get(2)?,
                    enabled: true,
                    created_at: row
                        .get::<_, String>(4)?
                        .parse()
                        .unwrap_or_else(|_| Utc::now()),
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

    pub fn delete_schedule(&self, id: &str) -> Result<bool> {
        let deleted = self.conn.execute("DELETE FROM schedules WHERE id = ?1", [id])?;
        Ok(deleted > 0)
    }

    pub fn update_schedule_after_run(
        &self,
        id: &str,
        triggered_at: DateTime<Utc>,
        next_run_at: Option<DateTime<Utc>>,
    ) -> Result<()> {
        self.conn.execute(
            "UPDATE schedules SET last_triggered_at = ?1, next_run_at = ?2 WHERE id = ?3",
            (
                triggered_at.to_rfc3339(),
                next_run_at.map(|t| t.to_rfc3339()),
                id,
            ),
        )?;
        Ok(())
    }
}

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::agent_transport::AgentTransport;
use crate::store::{ExecutionLog, ProjectStore};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Schedule {
    pub id: Uuid,
    pub project_id: Uuid,
    pub cron_expr: String,
    pub enabled: bool,
    pub created_at: DateTime<Utc>,
    pub last_triggered_at: Option<DateTime<Utc>>,
    pub next_run_at: Option<DateTime<Utc>>,
}

impl Schedule {
    pub fn new(project_id: Uuid, cron_expr: String) -> Self {
        let now = Utc::now();
        let next = parse_next_run(&cron_expr, now);
        Self {
            id: Uuid::new_v4(),
            project_id,
            cron_expr,
            enabled: true,
            created_at: now,
            last_triggered_at: None,
            next_run_at: next,
        }
    }
}

/// Simple cron-like expression parser.
/// Supports: "@every <N>s|m|h", "@hourly", "@daily", "@weekly", or "M H * * *" (minute hour).
pub fn parse_next_run(expr: &str, from: DateTime<Utc>) -> Option<DateTime<Utc>> {
    let expr = expr.trim();

    if let Some(interval) = expr.strip_prefix("@every ") {
        let interval = interval.trim();
        let secs = parse_duration_secs(interval)?;
        return Some(from + chrono::Duration::seconds(secs));
    }

    match expr {
        "@hourly" => Some(from + chrono::Duration::hours(1)),
        "@daily" => Some(from + chrono::Duration::days(1)),
        "@weekly" => Some(from + chrono::Duration::weeks(1)),
        _ => {
            // Simple "M H * * *" parsing (minute, hour)
            let parts: Vec<&str> = expr.split_whitespace().collect();
            if parts.len() >= 2 {
                let min: u32 = parts[0].parse().ok()?;
                let hour: u32 = parts[1].parse().ok()?;
                if min < 60 && hour < 24 {
                    let today = from.date_naive();
                    let time = chrono::NaiveTime::from_hms_opt(hour, min, 0)?;
                    let candidate = today.and_time(time).and_utc();
                    if candidate > from {
                        return Some(candidate);
                    }
                    // Next day
                    let tomorrow = today + chrono::Duration::days(1);
                    return Some(tomorrow.and_time(time).and_utc());
                }
            }
            None
        }
    }
}

fn parse_duration_secs(s: &str) -> Option<i64> {
    if let Some(n) = s.strip_suffix('s') {
        return n.trim().parse().ok();
    }
    if let Some(n) = s.strip_suffix('m') {
        return n.trim().parse::<i64>().ok().map(|n| n * 60);
    }
    if let Some(n) = s.strip_suffix('h') {
        return n.trim().parse::<i64>().ok().map(|n| n * 3600);
    }
    // Bare number = seconds
    s.parse().ok()
}

/// Run the scheduler tick: check all enabled schedules, execute any that are due.
pub async fn tick(store: &ProjectStore) -> Vec<(Uuid, Result<i32, String>)> {
    let now = Utc::now();
    let schedules = match store.list_schedules() {
        Ok(s) => s,
        Err(_) => return vec![],
    };

    let mut results = vec![];

    for sched in schedules {
        if !sched.enabled {
            continue;
        }
        let due = match sched.next_run_at {
            Some(next) if next <= now => true,
            _ => false,
        };
        if !due {
            continue;
        }

        let project = match store.get(sched.project_id) {
            Ok(Some(p)) => p,
            _ => continue,
        };

        let entrypoint = match &project.entrypoint {
            Some(ep) => ep.clone(),
            None => continue,
        };

        let _ = store.record_run_start(project.id);

        // Create execution log for scheduled run
        let exec_log = ExecutionLog::new(project.id, "schedule");
        let exec_log_id = exec_log.id;
        let _ = store.insert_execution_log(&exec_log);

        let runtime_str = format!("{:?}", project.runtime).to_lowercase();
        let exit_code = run_with_timeout(
            &project.id.to_string(),
            &runtime_str,
            &entrypoint,
            &project.path,
            300,
        )
        .await;

        let code = match &exit_code {
            Ok(c) => Some(*c),
            Err(_) => Some(-1),
        };
        let _ = store.record_run_end(project.id, code);

        // Finalize execution log
        let output = match &exit_code {
            Ok(c) => format!("Exited with code {c}"),
            Err(e) => format!("Error: {e}"),
        };
        let _ = store.finish_execution_log(exec_log_id, code.unwrap_or(-1), &output);

        // Advance schedule
        let next = parse_next_run(&sched.cron_expr, now);
        let _ = store.update_schedule_after_run(sched.id, now, next);

        results.push((project.id, exit_code));
    }

    results
}

async fn run_with_timeout(
    project_id: &str,
    runtime: &str,
    entrypoint: &str,
    working_dir: &str,
    timeout_secs: u64,
) -> Result<i32, String> {
    let client = crate::local_agent::get_or_start_local_agent()
        .await
        .map_err(|e| e.to_string())?;

    let result = tokio::time::timeout(
        tokio::time::Duration::from_secs(timeout_secs),
        client.execute(project_id, runtime, entrypoint, working_dir, None, None, std::collections::HashMap::new()),
    )
    .await;

    match result {
        Ok(Ok(exec_result)) => Ok(exec_result.exit_code),
        Ok(Err(e)) => Err(e.to_string()),
        Err(_) => {
            let _ = client.stop(project_id).await;
            Err("Timed out".into())
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::Utc;

    #[test]
    fn test_parse_every_seconds() {
        let now = Utc::now();
        let next = parse_next_run("@every 30s", now).unwrap();
        assert!((next - now).num_seconds() == 30);
    }

    #[test]
    fn test_parse_every_minutes() {
        let now = Utc::now();
        let next = parse_next_run("@every 5m", now).unwrap();
        assert!((next - now).num_seconds() == 300);
    }

    #[test]
    fn test_parse_every_hours() {
        let now = Utc::now();
        let next = parse_next_run("@every 2h", now).unwrap();
        assert!((next - now).num_seconds() == 7200);
    }

    #[test]
    fn test_parse_hourly() {
        let now = Utc::now();
        let next = parse_next_run("@hourly", now).unwrap();
        assert!((next - now).num_seconds() == 3600);
    }

    #[test]
    fn test_parse_daily() {
        let now = Utc::now();
        let next = parse_next_run("@daily", now).unwrap();
        assert!((next - now).num_seconds() == 86400);
    }
}

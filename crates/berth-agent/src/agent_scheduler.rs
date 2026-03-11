use std::collections::HashMap;
use std::sync::Arc;
use std::sync::atomic::{AtomicI64, Ordering};

use tokio::sync::Mutex;

use crate::executor::{self, LogStream};
use berth_proto::runtime::Runtime;
use crate::container;

use crate::agent_store::AgentStore;
use crate::nats_publisher::{self, NatsPublisher};

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

/// Agent-side scheduler tick. Checks for due schedules, executes them using the latest deployment.
pub async fn tick(store: &Arc<Mutex<AgentStore>>, nats: &Option<Arc<NatsPublisher>>) {
    let due_schedules = {
        let store = store.lock().await;
        match store.list_due_schedules() {
            Ok(s) => s,
            Err(e) => {
                tracing::error!("Failed to list due schedules: {e}");
                return;
            }
        }
    };

    for sched in due_schedules {
        let deployment = {
            let store = store.lock().await;
            match store.get_latest_deployment(&sched.project_id) {
                Ok(Some(d)) => d,
                Ok(None) => {
                    tracing::warn!(
                        "Schedule {} has no deployment for project {}",
                        sched.id,
                        sched.project_id
                    );
                    continue;
                }
                Err(e) => {
                    tracing::error!("Failed to get deployment for schedule {}: {e}", sched.id);
                    continue;
                }
            }
        };

        tracing::info!(
            "Scheduler triggering project {} (schedule {})",
            sched.project_id,
            sched.id
        );

        let execution_id = uuid::Uuid::new_v4().to_string();
        let now = chrono::Utc::now();

        // Record execution start
        {
            let store = store.lock().await;
            let _ = store.insert_execution(&crate::agent_store::Execution {
                id: execution_id.clone(),
                project_id: sched.project_id.clone(),
                deployment_id: Some(deployment.id.clone()),
                started_at: now,
                finished_at: None,
                exit_code: None,
                trigger: "schedule".into(),
                status: "running".into(),
            });
        }

        // Execute using deployment info
        let use_container = deployment.image_tag.as_ref().is_some_and(|t| !t.is_empty());

        let exit_code = if use_container {
            let tag = deployment.image_tag.as_ref().unwrap();
            let container_name = format!("berth-sched-{}", sched.project_id);
            let env_vars: HashMap<String, String> = HashMap::new();

            let caps = container::detect_capabilities();
            let rt = caps.runtime_cmd().unwrap_or("podman");
            match container::run_container(rt, tag, &container_name, &env_vars).await {
                Ok((mut child, mut rx)) => {
                    let seq = AtomicI64::new(0);
                    while let Some(line) = rx.recv().await {
                        let s = seq.fetch_add(1, Ordering::Relaxed);
                        let stream_str = match line.stream {
                            LogStream::Stdout => "stdout",
                            LogStream::Stderr => "stderr",
                        };
                        let st = store.lock().await;
                        let _ = st.append_log_line(&execution_id, s, stream_str, &line.text, &line.timestamp);
                        drop(st);
                        nats_publisher::maybe_publish_log_line(nats, &sched.project_id, &execution_id, stream_str, &line.text, s).await;
                    }
                    match child.wait().await {
                        Ok(status) => Ok(status.code().unwrap_or(-1)),
                        Err(e) => Err(e.to_string()),
                    }
                }
                Err(e) => Err(e.to_string()),
            }
        } else {
            let runtime = parse_runtime(&deployment.runtime);
            match executor::spawn_and_stream(runtime, &deployment.entrypoint, &deployment.working_dir, None).await {
                Ok((mut child, mut rx)) => {
                    let seq = AtomicI64::new(0);
                    while let Some(line) = rx.recv().await {
                        let s = seq.fetch_add(1, Ordering::Relaxed);
                        let stream_str = match line.stream {
                            LogStream::Stdout => "stdout",
                            LogStream::Stderr => "stderr",
                        };
                        let st = store.lock().await;
                        let _ = st.append_log_line(&execution_id, s, stream_str, &line.text, &line.timestamp);
                        drop(st);
                        nats_publisher::maybe_publish_log_line(nats, &sched.project_id, &execution_id, stream_str, &line.text, s).await;
                    }
                    match child.wait().await {
                        Ok(status) => Ok(status.code().unwrap_or(-1)),
                        Err(e) => Err(e.to_string()),
                    }
                }
                Err(e) => Err(e.to_string()),
            }
        };

        let (status, code) = match exit_code {
            Ok(c) => {
                let s = if c == 0 { "completed" } else { "failed" };
                (s, c)
            }
            Err(e) => {
                tracing::error!(
                    "Scheduled execution failed for project {}: {e}",
                    sched.project_id
                );
                ("failed", -1)
            }
        };

        // Record execution end and emit event
        let data = serde_json::json!({
            "schedule_id": sched.id,
            "exit_code": code,
            "status": status,
        })
        .to_string();
        {
            let store = store.lock().await;
            let _ = store.finish_execution(&execution_id, code, status);
            let _ = store.insert_event(
                "schedule_triggered",
                Some(&sched.project_id),
                Some(&execution_id),
                &data,
            );
        }
        nats_publisher::maybe_publish_event(nats, "schedule_triggered", Some(&sched.project_id), Some(&execution_id), &data).await;

        // Advance schedule
        let next = berth_proto::schedule::parse_next_run(&sched.cron_expr, now);
        {
            let store = store.lock().await;
            let _ = store.update_schedule_after_run(&sched.id, now, next);
        }
    }
}

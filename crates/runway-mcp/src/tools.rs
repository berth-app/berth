use std::path::Path;

use runway_core::executor;
use runway_core::project::Project;
use runway_core::runtime;
use runway_core::scheduler::Schedule;
use runway_core::store::ProjectStore;
use serde_json::{json, Value};

use crate::protocol::{CallToolResult, Tool};

fn get_store() -> Result<ProjectStore, String> {
    let data_dir = dirs_next::data_dir()
        .unwrap_or_else(|| std::path::PathBuf::from("."))
        .join("com.runway.app");
    std::fs::create_dir_all(&data_dir).map_err(|e| e.to_string())?;
    let db_path = data_dir.join("runway.db");
    ProjectStore::open(db_path.to_str().unwrap_or("runway.db")).map_err(|e| e.to_string())
}

pub fn list_tools() -> Vec<Tool> {
    vec![
        Tool {
            name: "runway_list_projects".into(),
            description: "List all Runway projects with their status, runtime, and metadata".into(),
            input_schema: json!({
                "type": "object",
                "properties": {},
                "required": []
            }),
        },
        Tool {
            name: "runway_project_status".into(),
            description: "Get detailed status of a specific project including run history".into(),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "project_id": {
                        "type": "string",
                        "description": "Project UUID or name"
                    }
                },
                "required": ["project_id"]
            }),
        },
        Tool {
            name: "runway_deploy".into(),
            description: "Deploy code to a target. Can deploy from a directory path or inline code.".into(),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "name": {
                        "type": "string",
                        "description": "Project name"
                    },
                    "path": {
                        "type": "string",
                        "description": "Path to code directory or file"
                    },
                    "code": {
                        "type": "string",
                        "description": "Inline code to deploy (alternative to path)"
                    },
                    "target": {
                        "type": "string",
                        "description": "Deploy target (default: local)",
                        "default": "local"
                    }
                },
                "required": ["name"]
            }),
        },
        Tool {
            name: "runway_run".into(),
            description: "Run a project and return its output".into(),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "project_id": {
                        "type": "string",
                        "description": "Project UUID or name"
                    },
                    "timeout_secs": {
                        "type": "integer",
                        "description": "Max seconds to wait for output (default: 30)",
                        "default": 30
                    }
                },
                "required": ["project_id"]
            }),
        },
        Tool {
            name: "runway_stop".into(),
            description: "Stop a running project".into(),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "project_id": {
                        "type": "string",
                        "description": "Project UUID or name"
                    }
                },
                "required": ["project_id"]
            }),
        },
        Tool {
            name: "runway_logs".into(),
            description: "Fetch recent logs from a project run".into(),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "project_id": {
                        "type": "string",
                        "description": "Project UUID or name"
                    },
                    "tail": {
                        "type": "integer",
                        "description": "Number of recent lines to return (default: 50)",
                        "default": 50
                    }
                },
                "required": ["project_id"]
            }),
        },
        Tool {
            name: "runway_import_code".into(),
            description: "Import code from a path or inline string, creating a new project".into(),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "name": {
                        "type": "string",
                        "description": "Project name"
                    },
                    "path": {
                        "type": "string",
                        "description": "Path to existing code directory or file"
                    },
                    "code": {
                        "type": "string",
                        "description": "Inline code to save"
                    },
                    "filename": {
                        "type": "string",
                        "description": "Filename for inline code (e.g. main.py)"
                    }
                },
                "required": ["name"]
            }),
        },
        Tool {
            name: "runway_detect_runtime".into(),
            description: "Auto-detect the runtime, entrypoint, and dependencies for a path".into(),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "path": {
                        "type": "string",
                        "description": "Path to code directory or file"
                    }
                },
                "required": ["path"]
            }),
        },
        Tool {
            name: "runway_delete".into(),
            description: "Delete a project".into(),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "project_id": {
                        "type": "string",
                        "description": "Project UUID or name"
                    }
                },
                "required": ["project_id"]
            }),
        },
        Tool {
            name: "runway_health".into(),
            description: "Check Runway system health and version".into(),
            input_schema: json!({
                "type": "object",
                "properties": {},
                "required": []
            }),
        },
        Tool {
            name: "runway_schedule_add".into(),
            description: "Add a cron-like schedule to a project. Supports: @every 5m, @hourly, @daily, @weekly, or 'M H * * *'".into(),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "project_id": {
                        "type": "string",
                        "description": "Project UUID or name"
                    },
                    "cron": {
                        "type": "string",
                        "description": "Cron expression (e.g. '@every 5m', '@hourly', '@daily', '30 9 * * *')"
                    }
                },
                "required": ["project_id", "cron"]
            }),
        },
        Tool {
            name: "runway_schedule_list".into(),
            description: "List all schedules".into(),
            input_schema: json!({
                "type": "object",
                "properties": {},
                "required": []
            }),
        },
        Tool {
            name: "runway_schedule_remove".into(),
            description: "Remove a schedule by UUID".into(),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "schedule_id": {
                        "type": "string",
                        "description": "Schedule UUID"
                    }
                },
                "required": ["schedule_id"]
            }),
        },
    ]
}

fn find_project(identifier: &str) -> Result<Project, String> {
    let store = get_store()?;

    // Try as UUID first
    if let Ok(uuid) = identifier.parse::<uuid::Uuid>() {
        if let Some(p) = store.get(uuid).map_err(|e| e.to_string())? {
            return Ok(p);
        }
    }

    // Fall back to name match
    let projects = store.list().map_err(|e| e.to_string())?;
    projects
        .into_iter()
        .find(|p| p.name == identifier)
        .ok_or_else(|| format!("Project '{identifier}' not found"))
}

pub async fn call_tool(name: &str, args: &Value) -> CallToolResult {
    match name {
        "runway_list_projects" => handle_list_projects().await,
        "runway_project_status" => handle_project_status(args).await,
        "runway_deploy" => handle_deploy(args).await,
        "runway_run" => handle_run(args).await,
        "runway_stop" => handle_stop(args).await,
        "runway_logs" => handle_logs(args).await,
        "runway_import_code" => handle_import_code(args).await,
        "runway_detect_runtime" => handle_detect_runtime(args).await,
        "runway_delete" => handle_delete(args).await,
        "runway_health" => handle_health().await,
        "runway_schedule_add" => handle_schedule_add(args).await,
        "runway_schedule_list" => handle_schedule_list().await,
        "runway_schedule_remove" => handle_schedule_remove(args).await,
        _ => CallToolResult::error(format!("Unknown tool: {name}")),
    }
}

async fn handle_list_projects() -> CallToolResult {
    let store = match get_store() {
        Ok(s) => s,
        Err(e) => return CallToolResult::error(e),
    };
    let projects = match store.list() {
        Ok(p) => p,
        Err(e) => return CallToolResult::error(e.to_string()),
    };

    let list: Vec<Value> = projects
        .iter()
        .map(|p| {
            json!({
                "id": p.id.to_string(),
                "name": p.name,
                "path": p.path,
                "runtime": format!("{:?}", p.runtime).to_lowercase(),
                "entrypoint": p.entrypoint,
                "status": format!("{:?}", p.status).to_lowercase(),
                "run_count": p.run_count,
                "last_run_at": p.last_run_at.map(|t| t.to_rfc3339()),
                "last_exit_code": p.last_exit_code,
            })
        })
        .collect();

    CallToolResult::text(serde_json::to_string_pretty(&list).unwrap_or_default())
}

async fn handle_project_status(args: &Value) -> CallToolResult {
    let project_id = match args.get("project_id").and_then(|v| v.as_str()) {
        Some(id) => id,
        None => return CallToolResult::error("Missing required parameter: project_id".into()),
    };

    match find_project(project_id) {
        Ok(p) => {
            let info = json!({
                "id": p.id.to_string(),
                "name": p.name,
                "path": p.path,
                "runtime": format!("{:?}", p.runtime).to_lowercase(),
                "entrypoint": p.entrypoint,
                "status": format!("{:?}", p.status).to_lowercase(),
                "run_count": p.run_count,
                "last_run_at": p.last_run_at.map(|t| t.to_rfc3339()),
                "last_exit_code": p.last_exit_code,
                "created_at": p.created_at.to_rfc3339(),
                "updated_at": p.updated_at.to_rfc3339(),
            });
            CallToolResult::text(serde_json::to_string_pretty(&info).unwrap_or_default())
        }
        Err(e) => CallToolResult::error(e),
    }
}

async fn handle_deploy(args: &Value) -> CallToolResult {
    let name = match args.get("name").and_then(|v| v.as_str()) {
        Some(n) => n,
        None => return CallToolResult::error("Missing required parameter: name".into()),
    };

    let path = if let Some(code) = args.get("code").and_then(|v| v.as_str()) {
        // Save inline code
        let filename = args
            .get("filename")
            .and_then(|v| v.as_str())
            .unwrap_or("main.py");
        let base = dirs_next::data_dir()
            .unwrap_or_else(|| std::path::PathBuf::from("/tmp"))
            .join("com.runway.app")
            .join("projects")
            .join(name);
        if let Err(e) = std::fs::create_dir_all(&base) {
            return CallToolResult::error(format!("Failed to create directory: {e}"));
        }
        if let Err(e) = std::fs::write(base.join(filename), code) {
            return CallToolResult::error(format!("Failed to write code: {e}"));
        }
        base.to_string_lossy().to_string()
    } else if let Some(p) = args.get("path").and_then(|v| v.as_str()) {
        p.to_string()
    } else {
        return CallToolResult::error("Either 'path' or 'code' must be provided".into());
    };

    let store = match get_store() {
        Ok(s) => s,
        Err(e) => return CallToolResult::error(e),
    };

    let info = runtime::detect_runtime(Path::new(&path));
    let mut project = Project::new(name.to_string(), path.clone(), info.runtime);
    project.entrypoint = info.entrypoint;

    if let Err(e) = store.insert(&project) {
        return CallToolResult::error(format!("Failed to create project: {e}"));
    }

    // Run it
    let entrypoint = match &project.entrypoint {
        Some(ep) => ep.clone(),
        None => return CallToolResult::text(format!(
            "Project '{}' created at {} but no entrypoint detected. Set one manually.",
            name, path
        )),
    };

    let _ = store.record_run_start(project.id);

    match executor::spawn_and_stream(project.runtime, &entrypoint, &project.path).await {
        Ok((mut child, mut rx)) => {
            let mut output = String::new();
            let timeout = tokio::time::Duration::from_secs(
                args.get("timeout_secs")
                    .and_then(|v| v.as_u64())
                    .unwrap_or(30),
            );

            let result = tokio::time::timeout(timeout, async {
                while let Some(line) = rx.recv().await {
                    output.push_str(&line.text);
                    output.push('\n');
                }
            })
            .await;

            let exit_code = if result.is_err() {
                let _ = child.kill().await;
                Some(-1)
            } else {
                child.wait().await.ok().and_then(|s| s.code())
            };

            let _ = store.record_run_end(project.id, exit_code);

            CallToolResult::text(format!(
                "Deployed and ran '{}' (exit code: {})\n\n{}",
                name,
                exit_code.map(|c| c.to_string()).unwrap_or("unknown".into()),
                output
            ))
        }
        Err(e) => {
            let _ = store.record_run_end(project.id, Some(-1));
            CallToolResult::error(format!("Failed to start: {e}"))
        }
    }
}

async fn handle_run(args: &Value) -> CallToolResult {
    let project_id = match args.get("project_id").and_then(|v| v.as_str()) {
        Some(id) => id,
        None => return CallToolResult::error("Missing required parameter: project_id".into()),
    };

    let project = match find_project(project_id) {
        Ok(p) => p,
        Err(e) => return CallToolResult::error(e),
    };

    let entrypoint = match &project.entrypoint {
        Some(ep) => ep.clone(),
        None => return CallToolResult::error("Project has no entrypoint".into()),
    };

    let store = match get_store() {
        Ok(s) => s,
        Err(e) => return CallToolResult::error(e),
    };
    let _ = store.record_run_start(project.id);

    let timeout_secs = args
        .get("timeout_secs")
        .and_then(|v| v.as_u64())
        .unwrap_or(30);

    match executor::spawn_and_stream(project.runtime, &entrypoint, &project.path).await {
        Ok((mut child, mut rx)) => {
            let mut output = String::new();
            let timeout = tokio::time::Duration::from_secs(timeout_secs);

            let result = tokio::time::timeout(timeout, async {
                while let Some(line) = rx.recv().await {
                    output.push_str(&line.text);
                    output.push('\n');
                }
            })
            .await;

            let exit_code = if result.is_err() {
                let _ = child.kill().await;
                output.push_str(&format!("\n[Timed out after {}s]\n", timeout_secs));
                Some(-1)
            } else {
                child.wait().await.ok().and_then(|s| s.code())
            };

            let _ = store.record_run_end(project.id, exit_code);

            CallToolResult::text(format!(
                "Exit code: {}\n\n{}",
                exit_code.map(|c| c.to_string()).unwrap_or("unknown".into()),
                output
            ))
        }
        Err(e) => {
            let _ = store.record_run_end(project.id, Some(-1));
            CallToolResult::error(format!("Failed to start: {e}"))
        }
    }
}

async fn handle_stop(args: &Value) -> CallToolResult {
    let project_id = match args.get("project_id").and_then(|v| v.as_str()) {
        Some(id) => id,
        None => return CallToolResult::error("Missing required parameter: project_id".into()),
    };

    let project = match find_project(project_id) {
        Ok(p) => p,
        Err(e) => return CallToolResult::error(e),
    };

    let store = match get_store() {
        Ok(s) => s,
        Err(e) => return CallToolResult::error(e),
    };

    if let Err(e) = store.update_status(
        project.id,
        runway_core::project::ProjectStatus::Stopped,
    ) {
        return CallToolResult::error(format!("Failed to update status: {e}"));
    }

    CallToolResult::text(format!("Project '{}' marked as stopped", project.name))
}

async fn handle_logs(_args: &Value) -> CallToolResult {
    // Logs are currently only streamed via Tauri events.
    // For MCP, we'd need a log store. For now, return a helpful message.
    CallToolResult::text(
        "Log storage for MCP access is not yet implemented. \
         Use runway_run to execute a project and capture output directly."
            .into(),
    )
}

async fn handle_import_code(args: &Value) -> CallToolResult {
    let name = match args.get("name").and_then(|v| v.as_str()) {
        Some(n) => n,
        None => return CallToolResult::error("Missing required parameter: name".into()),
    };

    let path = if let Some(code) = args.get("code").and_then(|v| v.as_str()) {
        let filename = args
            .get("filename")
            .and_then(|v| v.as_str())
            .unwrap_or("main.py");
        let base = dirs_next::data_dir()
            .unwrap_or_else(|| std::path::PathBuf::from("/tmp"))
            .join("com.runway.app")
            .join("projects")
            .join(name);
        if let Err(e) = std::fs::create_dir_all(&base) {
            return CallToolResult::error(format!("Failed to create directory: {e}"));
        }
        if let Err(e) = std::fs::write(base.join(filename), code) {
            return CallToolResult::error(format!("Failed to write code: {e}"));
        }
        base.to_string_lossy().to_string()
    } else if let Some(p) = args.get("path").and_then(|v| v.as_str()) {
        p.to_string()
    } else {
        return CallToolResult::error("Either 'path' or 'code' must be provided".into());
    };

    let store = match get_store() {
        Ok(s) => s,
        Err(e) => return CallToolResult::error(e),
    };

    let info = runtime::detect_runtime(Path::new(&path));
    let mut project = Project::new(name.to_string(), path, info.runtime);
    project.entrypoint = info.entrypoint.clone();

    if let Err(e) = store.insert(&project) {
        return CallToolResult::error(format!("Failed to create project: {e}"));
    }

    CallToolResult::text(format!(
        "Project '{}' created (runtime: {:?}, entrypoint: {})",
        name,
        info.runtime,
        info.entrypoint.unwrap_or("none".into())
    ))
}

async fn handle_detect_runtime(args: &Value) -> CallToolResult {
    let path = match args.get("path").and_then(|v| v.as_str()) {
        Some(p) => p,
        None => return CallToolResult::error("Missing required parameter: path".into()),
    };

    let info = runtime::detect_runtime(Path::new(path));
    let result = json!({
        "runtime": format!("{:?}", info.runtime).to_lowercase(),
        "entrypoint": info.entrypoint,
        "version_file": info.version_file,
        "confidence": info.confidence,
        "dependencies": info.dependencies,
        "scripts": info.scripts,
    });

    CallToolResult::text(serde_json::to_string_pretty(&result).unwrap_or_default())
}

async fn handle_delete(args: &Value) -> CallToolResult {
    let project_id = match args.get("project_id").and_then(|v| v.as_str()) {
        Some(id) => id,
        None => return CallToolResult::error("Missing required parameter: project_id".into()),
    };

    let project = match find_project(project_id) {
        Ok(p) => p,
        Err(e) => return CallToolResult::error(e),
    };

    let store = match get_store() {
        Ok(s) => s,
        Err(e) => return CallToolResult::error(e),
    };

    if let Err(e) = store.delete(project.id) {
        return CallToolResult::error(format!("Failed to delete: {e}"));
    }

    CallToolResult::text(format!("Project '{}' deleted", project.name))
}

async fn handle_health() -> CallToolResult {
    let (project_count, schedule_count) = match get_store() {
        Ok(s) => {
            let pc = s.list().map(|p| p.len()).unwrap_or(0);
            let sc = s.list_schedules().map(|s| s.len()).unwrap_or(0);
            (pc, sc)
        }
        Err(_) => (0, 0),
    };

    let info = json!({
        "status": "healthy",
        "version": env!("CARGO_PKG_VERSION"),
        "projects": project_count,
        "schedules": schedule_count,
        "platform": std::env::consts::OS,
    });

    CallToolResult::text(serde_json::to_string_pretty(&info).unwrap_or_default())
}

async fn handle_schedule_add(args: &Value) -> CallToolResult {
    let project_id = match args.get("project_id").and_then(|v| v.as_str()) {
        Some(id) => id,
        None => return CallToolResult::error("Missing required parameter: project_id".into()),
    };
    let cron = match args.get("cron").and_then(|v| v.as_str()) {
        Some(c) => c,
        None => return CallToolResult::error("Missing required parameter: cron".into()),
    };

    let project = match find_project(project_id) {
        Ok(p) => p,
        Err(e) => return CallToolResult::error(e),
    };

    let store = match get_store() {
        Ok(s) => s,
        Err(e) => return CallToolResult::error(e),
    };

    let sched = Schedule::new(project.id, cron.to_string());
    if let Err(e) = store.insert_schedule(&sched) {
        return CallToolResult::error(format!("Failed to create schedule: {e}"));
    }

    let result = json!({
        "schedule_id": sched.id.to_string(),
        "project": project.name,
        "cron": cron,
        "next_run_at": sched.next_run_at.map(|t| t.to_rfc3339()),
    });
    CallToolResult::text(serde_json::to_string_pretty(&result).unwrap_or_default())
}

async fn handle_schedule_list() -> CallToolResult {
    let store = match get_store() {
        Ok(s) => s,
        Err(e) => return CallToolResult::error(e),
    };

    let schedules = match store.list_schedules() {
        Ok(s) => s,
        Err(e) => return CallToolResult::error(e.to_string()),
    };

    let list: Vec<Value> = schedules
        .iter()
        .map(|s| {
            json!({
                "id": s.id.to_string(),
                "project_id": s.project_id.to_string(),
                "cron": s.cron_expr,
                "enabled": s.enabled,
                "next_run_at": s.next_run_at.map(|t| t.to_rfc3339()),
                "last_triggered_at": s.last_triggered_at.map(|t| t.to_rfc3339()),
            })
        })
        .collect();

    CallToolResult::text(serde_json::to_string_pretty(&list).unwrap_or_default())
}

async fn handle_schedule_remove(args: &Value) -> CallToolResult {
    let schedule_id = match args.get("schedule_id").and_then(|v| v.as_str()) {
        Some(id) => id,
        None => return CallToolResult::error("Missing required parameter: schedule_id".into()),
    };

    let uuid: uuid::Uuid = match schedule_id.parse() {
        Ok(u) => u,
        Err(_) => return CallToolResult::error(format!("Invalid UUID: {schedule_id}")),
    };

    let store = match get_store() {
        Ok(s) => s,
        Err(e) => return CallToolResult::error(e),
    };

    if let Err(e) = store.delete_schedule(uuid) {
        return CallToolResult::error(format!("Failed to delete schedule: {e}"));
    }

    CallToolResult::text("Schedule deleted".into())
}

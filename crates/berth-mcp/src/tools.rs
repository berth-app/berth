use std::path::Path;

use berth_core::project::Project;
use berth_core::path_safety;
use berth_core::runtime;
use berth_core::agent_client::AgentClient;
use berth_core::agent_transport::AgentTransport;
use berth_core::scheduler::Schedule;
use berth_core::target::Target;
use berth_core::store::ProjectStore;
use serde_json::{json, Value};

use crate::protocol::{CallToolResult, Tool};

fn get_store() -> Result<ProjectStore, String> {
    let data_dir = dirs_next::data_dir()
        .unwrap_or_else(|| std::path::PathBuf::from("."))
        .join("com.berth.app");
    std::fs::create_dir_all(&data_dir).map_err(|e| e.to_string())?;
    let db_path = data_dir.join("berth.db");
    ProjectStore::open(db_path.to_str().unwrap_or("berth.db")).map_err(|e| e.to_string())
}

pub fn list_tools() -> Vec<Tool> {
    vec![
        Tool {
            name: "berth_list_projects".into(),
            description: "List all Berth projects with their status, runtime, and metadata".into(),
            input_schema: json!({
                "type": "object",
                "properties": {},
                "required": []
            }),
        },
        Tool {
            name: "berth_project_status".into(),
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
            name: "berth_deploy".into(),
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
            name: "berth_run".into(),
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
            name: "berth_stop".into(),
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
            name: "berth_logs".into(),
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
            name: "berth_import_code".into(),
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
            name: "berth_detect_runtime".into(),
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
            name: "berth_delete".into(),
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
            name: "berth_health".into(),
            description: "Check Berth system health and version".into(),
            input_schema: json!({
                "type": "object",
                "properties": {},
                "required": []
            }),
        },
        Tool {
            name: "berth_schedule_add".into(),
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
            name: "berth_schedule_list".into(),
            description: "List all schedules".into(),
            input_schema: json!({
                "type": "object",
                "properties": {},
                "required": []
            }),
        },
        Tool {
            name: "berth_schedule_remove".into(),
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
        Tool {
            name: "berth_list_targets".into(),
            description: "List configured deploy targets (local + remote agents)".into(),
            input_schema: json!({
                "type": "object",
                "properties": {},
                "required": []
            }),
        },
        Tool {
            name: "berth_add_target".into(),
            description: "Add a new remote deploy target (agent endpoint)".into(),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "name": {
                        "type": "string",
                        "description": "Target name (e.g. 'my-vps', 'staging')"
                    },
                    "host": {
                        "type": "string",
                        "description": "Agent host address (IP or hostname)"
                    },
                    "port": {
                        "type": "integer",
                        "description": "Agent port (default: 50051)",
                        "default": 50051
                    }
                },
                "required": ["name", "host"]
            }),
        },
        Tool {
            name: "berth_remove_target".into(),
            description: "Remove a deploy target by name".into(),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "name": {
                        "type": "string",
                        "description": "Target name"
                    }
                },
                "required": ["name"]
            }),
        },
        Tool {
            name: "berth_list_agents".into(),
            description: "List connected agents and their health status. Pings each target.".into(),
            input_schema: json!({
                "type": "object",
                "properties": {},
                "required": []
            }),
        },
        Tool {
            name: "berth_publish".into(),
            description: "Publish a running project to a public URL via tunnel (cloudflared). The project must be running and listening on a port.".into(),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "project_id": {
                        "type": "string",
                        "description": "Project UUID or name"
                    },
                    "port": {
                        "type": "integer",
                        "description": "Local port the service listens on"
                    },
                    "provider": {
                        "type": "string",
                        "description": "Tunnel provider (default: cloudflared)",
                        "default": "cloudflared",
                        "enum": ["cloudflared"]
                    }
                },
                "required": ["project_id", "port"]
            }),
        },
        Tool {
            name: "berth_unpublish".into(),
            description: "Stop the public URL tunnel for a project.".into(),
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
            name: "berth_env_set".into(),
            description: "Set an environment variable for a project. Variables are passed to the process at runtime.".into(),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "project_id": {
                        "type": "string",
                        "description": "Project UUID or name"
                    },
                    "key": {
                        "type": "string",
                        "description": "Variable name (e.g. API_KEY)"
                    },
                    "value": {
                        "type": "string",
                        "description": "Variable value"
                    }
                },
                "required": ["project_id", "key", "value"]
            }),
        },
        Tool {
            name: "berth_env_get".into(),
            description: "Get all environment variables for a project.".into(),
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
            name: "berth_env_delete".into(),
            description: "Delete an environment variable from a project.".into(),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "project_id": {
                        "type": "string",
                        "description": "Project UUID or name"
                    },
                    "key": {
                        "type": "string",
                        "description": "Variable name to delete"
                    }
                },
                "required": ["project_id", "key"]
            }),
        },
        Tool {
            name: "berth_env_import".into(),
            description: "Import environment variables from .env format. Merges with existing variables.".into(),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "project_id": {
                        "type": "string",
                        "description": "Project UUID or name"
                    },
                    "content": {
                        "type": "string",
                        "description": ".env file content (KEY=VALUE lines)"
                    }
                },
                "required": ["project_id", "content"]
            }),
        },
        // Template Store
        Tool {
            name: "berth_store_list".into(),
            description: "List available templates from the Berth Template Store. Optionally filter by category (scrapers, api-servers, bots, ai-ml).".into(),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "category": {
                        "type": "string",
                        "description": "Filter by category: scrapers, api-servers, bots, ai-ml"
                    }
                },
                "required": []
            }),
        },
        Tool {
            name: "berth_store_search".into(),
            description: "Search the Berth Template Store by keyword.".into(),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "query": {
                        "type": "string",
                        "description": "Search query"
                    }
                },
                "required": ["query"]
            }),
        },
        Tool {
            name: "berth_store_install".into(),
            description: "Install a template from the Berth Template Store, creating a new project ready to run.".into(),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "template_id": {
                        "type": "string",
                        "description": "Template ID (e.g. hn-scraper, fastapi-starter)"
                    }
                },
                "required": ["template_id"]
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
        "berth_list_projects" => handle_list_projects().await,
        "berth_project_status" => handle_project_status(args).await,
        "berth_deploy" => handle_deploy(args).await,
        "berth_run" => handle_run(args).await,
        "berth_stop" => handle_stop(args).await,
        "berth_logs" => handle_logs(args).await,
        "berth_import_code" => handle_import_code(args).await,
        "berth_detect_runtime" => handle_detect_runtime(args).await,
        "berth_delete" => handle_delete(args).await,
        "berth_health" => handle_health().await,
        "berth_schedule_add" => handle_schedule_add(args).await,
        "berth_schedule_list" => handle_schedule_list().await,
        "berth_schedule_remove" => handle_schedule_remove(args).await,
        "berth_list_targets" => handle_list_targets().await,
        "berth_add_target" => handle_add_target(args).await,
        "berth_remove_target" => handle_remove_target(args).await,
        "berth_list_agents" => handle_list_agents().await,
        "berth_publish" => handle_publish(args).await,
        "berth_unpublish" => handle_unpublish(args).await,
        "berth_env_set" => handle_env_set(args).await,
        "berth_env_get" => handle_env_get(args).await,
        "berth_env_delete" => handle_env_delete(args).await,
        "berth_env_import" => handle_env_import(args).await,
        "berth_store_list" => handle_store_list(args).await,
        "berth_store_search" => handle_store_search(args).await,
        "berth_store_install" => handle_store_install(args).await,
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
            .join("com.berth.app")
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
    let runtime_str = format!("{:?}", project.runtime).to_lowercase();

    let client = match berth_core::local_agent::get_or_start_local_agent().await {
        Ok(c) => c,
        Err(e) => return CallToolResult::error(format!("Failed to start local agent: {e}")),
    };

    let timeout_secs = args
        .get("timeout_secs")
        .and_then(|v| v.as_u64())
        .unwrap_or(30);

    let env_vars = store.get_env_vars(project.id).unwrap_or_default();

    let result = tokio::time::timeout(
        tokio::time::Duration::from_secs(timeout_secs),
        client.execute(
            &project.id.to_string(),
            &runtime_str,
            &entrypoint,
            &project.path,
            None,
            None,
            env_vars,
        ),
    )
    .await;

    match result {
        Ok(Ok(exec_result)) => {
            let output: String = exec_result.logs.iter().map(|l| format!("{}\n", l.text)).collect();
            let _ = store.record_run_end(project.id, Some(exec_result.exit_code));
            CallToolResult::text(format!(
                "Deployed and ran '{}' (exit code: {})\n\n{}",
                name, exec_result.exit_code, output
            ))
        }
        Ok(Err(e)) => {
            let _ = store.record_run_end(project.id, Some(-1));
            CallToolResult::error(format!("Failed to execute: {e}"))
        }
        Err(_) => {
            // Timeout — stop the process
            let _ = client.stop(&project.id.to_string()).await;
            let _ = store.record_run_end(project.id, Some(-1));
            CallToolResult::text(format!(
                "Project '{}' timed out after {}s",
                name, timeout_secs
            ))
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
    let runtime_str = format!("{:?}", project.runtime).to_lowercase();

    let client = match berth_core::local_agent::get_or_start_local_agent().await {
        Ok(c) => c,
        Err(e) => return CallToolResult::error(format!("Failed to start local agent: {e}")),
    };

    let timeout_secs = args
        .get("timeout_secs")
        .and_then(|v| v.as_u64())
        .unwrap_or(30);

    let env_vars = store.get_env_vars(project.id).unwrap_or_default();

    let result = tokio::time::timeout(
        tokio::time::Duration::from_secs(timeout_secs),
        client.execute(
            &project.id.to_string(),
            &runtime_str,
            &entrypoint,
            &project.path,
            None,
            None,
            env_vars,
        ),
    )
    .await;

    match result {
        Ok(Ok(exec_result)) => {
            let output: String = exec_result.logs.iter().map(|l| format!("{}\n", l.text)).collect();
            let _ = store.record_run_end(project.id, Some(exec_result.exit_code));
            CallToolResult::text(format!(
                "Exit code: {}\n\n{}",
                exec_result.exit_code, output
            ))
        }
        Ok(Err(e)) => {
            let _ = store.record_run_end(project.id, Some(-1));
            CallToolResult::error(format!("Failed to execute: {e}"))
        }
        Err(_) => {
            let _ = client.stop(&project.id.to_string()).await;
            let _ = store.record_run_end(project.id, Some(-1));
            CallToolResult::text(format!("[Timed out after {}s]", timeout_secs))
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

    let client = match berth_core::local_agent::get_or_start_local_agent().await {
        Ok(c) => c,
        Err(e) => return CallToolResult::error(format!("Failed to connect to agent: {e}")),
    };

    match client.stop(&project.id.to_string()).await {
        Ok(true) => {
            let store = match get_store() {
                Ok(s) => s,
                Err(e) => return CallToolResult::error(e),
            };
            let _ = store.update_status(
                project.id,
                berth_core::project::ProjectStatus::Stopped,
            );
            CallToolResult::text(format!("Project '{}' stopped", project.name))
        }
        Ok(false) => {
            CallToolResult::text(format!("Project '{}' is not running", project.name))
        }
        Err(e) => CallToolResult::error(format!("Failed to stop: {e}")),
    }
}

async fn handle_logs(_args: &Value) -> CallToolResult {
    // Logs are currently only streamed via Tauri events.
    // For MCP, we'd need a log store. For now, return a helpful message.
    CallToolResult::text(
        "Log storage for MCP access is not yet implemented. \
         Use berth_run to execute a project and capture output directly."
            .into(),
    )
}

async fn handle_import_code(args: &Value) -> CallToolResult {
    let name = match args.get("name").and_then(|v| v.as_str()) {
        Some(n) => n,
        None => return CallToolResult::error("Missing required parameter: name".into()),
    };

    // Sanitize project name to prevent directory traversal
    let name = match path_safety::sanitize_project_name(name) {
        Ok(n) => n,
        Err(e) => return CallToolResult::error(format!("Invalid project name: {e}")),
    };

    let path = if let Some(code) = args.get("code").and_then(|v| v.as_str()) {
        let filename = args
            .get("filename")
            .and_then(|v| v.as_str())
            .unwrap_or("main.py");
        // Sanitize filename
        let filename = match path_safety::sanitize_filename(filename) {
            Ok(f) => f,
            Err(e) => return CallToolResult::error(format!("Invalid filename: {e}")),
        };
        let base = dirs_next::data_dir()
            .unwrap_or_else(|| std::path::PathBuf::from("/tmp"))
            .join("com.berth.app")
            .join("projects")
            .join(&name);
        if let Err(e) = std::fs::create_dir_all(&base) {
            return CallToolResult::error(format!("Failed to create directory: {e}"));
        }
        if let Err(e) = std::fs::write(base.join(&filename), code) {
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
    let mut project = Project::new(name.clone(), path, info.runtime);
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

    // Restrict to paths under user's home directory
    if let Some(home) = dirs_next::home_dir() {
        let target = Path::new(path);
        if let (Ok(canonical_home), Ok(canonical_target)) = (home.canonicalize(), target.canonicalize()) {
            if !canonical_target.starts_with(&canonical_home) {
                return CallToolResult::error(
                    "Path must be within the user's home directory".into(),
                );
            }
        }
    }

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

async fn handle_list_targets() -> CallToolResult {
    let store = match get_store() {
        Ok(s) => s,
        Err(e) => return CallToolResult::error(e),
    };

    let targets = match store.list_targets() {
        Ok(t) => t,
        Err(e) => return CallToolResult::error(e.to_string()),
    };

    let mut list: Vec<Value> = vec![json!({
        "name": "local",
        "kind": "local",
        "host": "127.0.0.1",
        "port": 50051,
        "status": "online",
    })];

    for t in &targets {
        list.push(json!({
            "name": t.name,
            "kind": format!("{:?}", t.kind).to_lowercase(),
            "host": t.host,
            "port": t.port,
            "status": format!("{:?}", t.status).to_lowercase(),
            "agent_version": t.agent_version,
            "last_seen_at": t.last_seen_at.map(|ts| ts.to_rfc3339()),
        }));
    }

    CallToolResult::text(serde_json::to_string_pretty(&list).unwrap_or_default())
}

async fn handle_add_target(args: &Value) -> CallToolResult {
    let name = match args.get("name").and_then(|v| v.as_str()) {
        Some(n) => n,
        None => return CallToolResult::error("Missing required parameter: name".into()),
    };
    let host = match args.get("host").and_then(|v| v.as_str()) {
        Some(h) => h,
        None => return CallToolResult::error("Missing required parameter: host".into()),
    };
    let port = args.get("port").and_then(|v| v.as_u64()).unwrap_or(50051) as u16;

    let store = match get_store() {
        Ok(s) => s,
        Err(e) => return CallToolResult::error(e),
    };

    let target = Target::new_remote(name.to_string(), host.to_string(), port);
    if let Err(e) = store.insert_target(&target) {
        return CallToolResult::error(format!("Failed to add target: {e}"));
    }

    CallToolResult::text(format!(
        "Target '{}' added ({}:{})",
        name, host, port
    ))
}

async fn handle_remove_target(args: &Value) -> CallToolResult {
    let name = match args.get("name").and_then(|v| v.as_str()) {
        Some(n) => n,
        None => return CallToolResult::error("Missing required parameter: name".into()),
    };

    let store = match get_store() {
        Ok(s) => s,
        Err(e) => return CallToolResult::error(e),
    };

    let target = match store.get_target_by_name(name) {
        Ok(Some(t)) => t,
        Ok(None) => return CallToolResult::error(format!("Target '{name}' not found")),
        Err(e) => return CallToolResult::error(e.to_string()),
    };

    if let Err(e) = store.delete_target(target.id) {
        return CallToolResult::error(format!("Failed to remove target: {e}"));
    }

    CallToolResult::text(format!("Target '{}' removed", name))
}

async fn handle_list_agents() -> CallToolResult {
    let store = match get_store() {
        Ok(s) => s,
        Err(e) => return CallToolResult::error(e),
    };

    let targets = match store.list_targets() {
        Ok(t) => t,
        Err(e) => return CallToolResult::error(e.to_string()),
    };

    let mut agents = Vec::new();

    for target in &targets {
        let endpoint = target.grpc_endpoint();
        let health = match AgentClient::connect(&endpoint).await {
            Ok(client) => match client.health().await {
                Ok(h) => {
                    let _ = store.update_target_status(
                        target.id,
                        berth_core::target::TargetStatus::Online,
                        Some(&h.version),
                    );
                    json!({
                        "name": target.name,
                        "host": target.host,
                        "port": target.port,
                        "status": "online",
                        "version": h.version,
                        "uptime_seconds": h.uptime_seconds,
                    })
                }
                Err(e) => {
                    let _ = store.update_target_status(
                        target.id,
                        berth_core::target::TargetStatus::Offline,
                        None,
                    );
                    json!({
                        "name": target.name,
                        "host": target.host,
                        "port": target.port,
                        "status": "offline",
                        "error": e.to_string(),
                    })
                }
            },
            Err(e) => {
                let _ = store.update_target_status(
                    target.id,
                    berth_core::target::TargetStatus::Offline,
                    None,
                );
                json!({
                    "name": target.name,
                    "host": target.host,
                    "port": target.port,
                    "status": "unreachable",
                    "error": e.to_string(),
                })
            }
        };
        agents.push(health);
    }

    CallToolResult::text(serde_json::to_string_pretty(&agents).unwrap_or_default())
}

async fn handle_publish(args: &Value) -> CallToolResult {
    let project_id = match args.get("project_id").and_then(|v| v.as_str()) {
        Some(id) => id,
        None => return CallToolResult::error("Missing required parameter: project_id".into()),
    };
    let port = match args.get("port").and_then(|v| v.as_u64()) {
        Some(p) => p as u16,
        None => return CallToolResult::error("Missing required parameter: port".into()),
    };
    let provider = args.get("provider").and_then(|v| v.as_str()).unwrap_or("cloudflared");

    let project = match find_project(project_id) {
        Ok(p) => p,
        Err(e) => return CallToolResult::error(e),
    };

    let client = match berth_core::local_agent::get_or_start_local_agent().await {
        Ok(c) => c,
        Err(e) => return CallToolResult::error(format!("Failed to connect to agent: {e}")),
    };

    match client.publish(&project.id.to_string(), port, provider, "").await {
        Ok((true, url, used_provider, _message)) => {
            if let Ok(store) = get_store() {
                let _ = store.set_tunnel_url(project.id, &url, &used_provider);
            }
            let result = json!({
                "success": true,
                "url": url,
                "provider": used_provider,
                "project": project.name,
            });
            CallToolResult::text(serde_json::to_string_pretty(&result).unwrap_or_default())
        }
        Ok((false, _, _, message)) => {
            CallToolResult::error(format!("Publish failed: {message}"))
        }
        Err(e) => CallToolResult::error(format!("Publish failed: {e}")),
    }
}

async fn handle_unpublish(args: &Value) -> CallToolResult {
    let project_id = match args.get("project_id").and_then(|v| v.as_str()) {
        Some(id) => id,
        None => return CallToolResult::error("Missing required parameter: project_id".into()),
    };

    let project = match find_project(project_id) {
        Ok(p) => p,
        Err(e) => return CallToolResult::error(e),
    };

    let client = match berth_core::local_agent::get_or_start_local_agent().await {
        Ok(c) => c,
        Err(e) => return CallToolResult::error(format!("Failed to connect to agent: {e}")),
    };

    match client.unpublish(&project.id.to_string()).await {
        Ok((true, _message)) => {
            if let Ok(store) = get_store() {
                let _ = store.clear_tunnel_url(project.id);
            }
            CallToolResult::text(format!("Project '{}' unpublished", project.name))
        }
        Ok((false, message)) => CallToolResult::error(format!("Unpublish failed: {message}")),
        Err(e) => CallToolResult::error(format!("Unpublish failed: {e}")),
    }
}

// --- Environment variable handlers ---

async fn handle_env_set(args: &Value) -> CallToolResult {
    let project_id = match args.get("project_id").and_then(|v| v.as_str()) {
        Some(id) => id,
        None => return CallToolResult::error("Missing required parameter: project_id".into()),
    };
    let key = match args.get("key").and_then(|v| v.as_str()) {
        Some(k) => k,
        None => return CallToolResult::error("Missing required parameter: key".into()),
    };
    let value = match args.get("value").and_then(|v| v.as_str()) {
        Some(v) => v,
        None => return CallToolResult::error("Missing required parameter: value".into()),
    };

    let project = match find_project(project_id) {
        Ok(p) => p,
        Err(e) => return CallToolResult::error(e),
    };

    let store = match get_store() {
        Ok(s) => s,
        Err(e) => return CallToolResult::error(e),
    };

    if let Err(e) = store.set_env_var(project.id, key, value) {
        return CallToolResult::error(format!("Failed to set env var: {e}"));
    }

    CallToolResult::text(format!("Set {}=*** on '{}'", key, project.name))
}

async fn handle_env_get(args: &Value) -> CallToolResult {
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

    let vars = match store.get_env_vars(project.id) {
        Ok(v) => v,
        Err(e) => return CallToolResult::error(e.to_string()),
    };

    let result = json!({
        "project": project.name,
        "env_vars": vars,
    });
    CallToolResult::text(serde_json::to_string_pretty(&result).unwrap_or_default())
}

async fn handle_env_delete(args: &Value) -> CallToolResult {
    let project_id = match args.get("project_id").and_then(|v| v.as_str()) {
        Some(id) => id,
        None => return CallToolResult::error("Missing required parameter: project_id".into()),
    };
    let key = match args.get("key").and_then(|v| v.as_str()) {
        Some(k) => k,
        None => return CallToolResult::error("Missing required parameter: key".into()),
    };

    let project = match find_project(project_id) {
        Ok(p) => p,
        Err(e) => return CallToolResult::error(e),
    };

    let store = match get_store() {
        Ok(s) => s,
        Err(e) => return CallToolResult::error(e),
    };

    if let Err(e) = store.delete_env_var(project.id, key) {
        return CallToolResult::error(format!("Failed to delete env var: {e}"));
    }

    CallToolResult::text(format!("Deleted {} from '{}'", key, project.name))
}

async fn handle_env_import(args: &Value) -> CallToolResult {
    let project_id = match args.get("project_id").and_then(|v| v.as_str()) {
        Some(id) => id,
        None => return CallToolResult::error("Missing required parameter: project_id".into()),
    };
    let content = match args.get("content").and_then(|v| v.as_str()) {
        Some(c) => c,
        None => return CallToolResult::error("Missing required parameter: content".into()),
    };

    let project = match find_project(project_id) {
        Ok(p) => p,
        Err(e) => return CallToolResult::error(e),
    };

    let store = match get_store() {
        Ok(s) => s,
        Err(e) => return CallToolResult::error(e),
    };

    let vars = berth_core::env::parse_dotenv(content);
    let count = vars.len();
    for (key, value) in &vars {
        if let Err(e) = store.set_env_var(project.id, key, value) {
            return CallToolResult::error(format!("Failed to import: {e}"));
        }
    }

    CallToolResult::text(format!(
        "Imported {} variable{} into '{}'",
        count,
        if count != 1 { "s" } else { "" },
        project.name
    ))
}

// --- Template Store handlers ---

async fn handle_store_list(args: &Value) -> CallToolResult {
    let category = args.get("category").and_then(|v| v.as_str());

    let catalog = match berth_core::template_store::get_catalog(false).await {
        Ok(c) => c,
        Err(e) => return CallToolResult::error(format!("Failed to load store: {e}")),
    };

    let templates: Vec<&berth_core::template_store::TemplateMeta> = match category {
        Some(cat) => berth_core::template_store::filter_by_category(&catalog, cat),
        None => catalog.templates.iter().collect(),
    };

    let list: Vec<Value> = templates
        .iter()
        .map(|t| {
            json!({
                "id": t.id,
                "name": t.name,
                "description": t.description,
                "category": t.category,
                "runtime": t.runtime,
                "pro_only": t.pro_only,
                "featured": t.featured,
                "version": t.version,
                "tags": t.tags,
            })
        })
        .collect();

    CallToolResult::text(serde_json::to_string_pretty(&json!({
        "categories": catalog.categories.iter().map(|c| json!({"id": c.id, "name": c.name})).collect::<Vec<_>>(),
        "templates": list,
    })).unwrap_or_default())
}

async fn handle_store_search(args: &Value) -> CallToolResult {
    let query = match args.get("query").and_then(|v| v.as_str()) {
        Some(q) => q,
        None => return CallToolResult::error("Missing required parameter: query".into()),
    };

    let catalog = match berth_core::template_store::get_catalog(false).await {
        Ok(c) => c,
        Err(e) => return CallToolResult::error(format!("Failed to load store: {e}")),
    };

    let templates = berth_core::template_store::search_templates(&catalog, query);

    let list: Vec<Value> = templates
        .iter()
        .map(|t| {
            json!({
                "id": t.id,
                "name": t.name,
                "description": t.description,
                "category": t.category,
                "runtime": t.runtime,
                "pro_only": t.pro_only,
                "version": t.version,
            })
        })
        .collect();

    CallToolResult::text(serde_json::to_string_pretty(&list).unwrap_or_default())
}

async fn handle_store_install(args: &Value) -> CallToolResult {
    let template_id = match args.get("template_id").and_then(|v| v.as_str()) {
        Some(id) => id,
        None => return CallToolResult::error("Missing required parameter: template_id".into()),
    };

    let catalog = match berth_core::template_store::get_catalog(false).await {
        Ok(c) => c,
        Err(e) => return CallToolResult::error(format!("Failed to load store: {e}")),
    };

    let template = match catalog.templates.iter().find(|t| t.id == template_id) {
        Some(t) => t,
        None => return CallToolResult::error(format!("Template '{template_id}' not found in store")),
    };

    let store = match get_store() {
        Ok(s) => s,
        Err(e) => return CallToolResult::error(e),
    };

    // MCP skips Pro tier check (pass None)
    match berth_core::template_store::install_template(&store, template, None).await {
        Ok(project) => {
            let result = json!({
                "project_id": project.id.to_string(),
                "name": project.name,
                "path": project.path,
                "runtime": format!("{:?}", project.runtime).to_lowercase(),
                "entrypoint": project.entrypoint,
            });
            CallToolResult::text(serde_json::to_string_pretty(&result).unwrap_or_default())
        }
        Err(e) => CallToolResult::error(format!("Failed to install template: {e}")),
    }
}

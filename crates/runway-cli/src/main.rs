use std::path::Path;

use clap::{Parser, Subcommand};
use runway_core::agent_client::AgentClient;
use runway_core::project::Project;
use runway_core::runtime;
use runway_core::scheduler::{self, Schedule};
use runway_core::store::ProjectStore;
use runway_core::target::Target;

#[derive(Parser)]
#[command(name = "runway", about = "Deploy and manage code from your terminal")]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    /// List all projects
    List,
    /// Deploy code to a target (create + run)
    Deploy {
        /// Path to code directory or file
        path: String,
        /// Project name (defaults to directory name)
        #[arg(long)]
        name: Option<String>,
        /// Target to deploy to
        #[arg(long, default_value = "local")]
        target: String,
    },
    /// Run a project
    Run {
        /// Project name or UUID
        project: String,
        /// Target to run on (default: local)
        #[arg(long, default_value = "local")]
        target: String,
    },
    /// Stop a running project
    Stop {
        /// Project name or UUID
        project: String,
        /// Target to stop on (default: local)
        #[arg(long, default_value = "local")]
        target: String,
    },
    /// View logs for a project (run and capture)
    Logs {
        /// Project name or UUID
        project: String,
        /// Follow log output
        #[arg(long, short)]
        follow: bool,
        /// Target to run on (default: local)
        #[arg(long, default_value = "local")]
        target: String,
    },
    /// Check status of a project
    Status {
        /// Project name or UUID
        project: String,
    },
    /// Import code as a new project
    Import {
        /// Path to code
        path: String,
        /// Project name
        #[arg(long)]
        name: Option<String>,
    },
    /// Detect runtime for a path
    Detect {
        /// Path to code
        path: String,
    },
    /// Delete a project
    Delete {
        /// Project name or UUID
        project: String,
    },
    /// System health check
    Health,
    /// Manage deploy targets
    Targets {
        #[command(subcommand)]
        action: TargetActions,
    },
    /// Manage schedules
    Schedule {
        #[command(subcommand)]
        action: ScheduleActions,
    },
}

#[derive(Subcommand)]
enum TargetActions {
    /// List configured targets
    List,
    /// Add a new remote target
    Add {
        /// Target name
        name: String,
        /// Agent host address (IP or hostname)
        #[arg(long)]
        host: String,
        /// Agent port (default: 50051)
        #[arg(long, default_value_t = 50051)]
        port: u16,
    },
    /// Remove a target
    Remove {
        /// Target name
        name: String,
    },
    /// Check health of a target's agent
    Ping {
        /// Target name
        name: String,
    },
}

#[derive(Subcommand)]
enum ScheduleActions {
    /// Add a schedule to a project
    Add {
        /// Project name or UUID
        project: String,
        /// Cron expression (e.g. "@every 5m", "@hourly", "@daily", "30 9 * * *")
        #[arg(long)]
        cron: String,
    },
    /// List all schedules
    List,
    /// Remove a schedule
    Remove {
        /// Schedule UUID
        id: String,
    },
    /// Enable a schedule
    Enable {
        /// Schedule UUID
        id: String,
    },
    /// Disable a schedule
    Disable {
        /// Schedule UUID
        id: String,
    },
    /// Run one scheduler tick (execute any due schedules)
    Tick,
}

fn get_store() -> anyhow::Result<ProjectStore> {
    let data_dir = dirs_next::data_dir()
        .unwrap_or_else(|| std::path::PathBuf::from("."))
        .join("com.runway.app");
    std::fs::create_dir_all(&data_dir)?;
    let db_path = data_dir.join("runway.db");
    ProjectStore::open(db_path.to_str().unwrap_or("runway.db"))
}

async fn get_agent_client(target: &str) -> anyhow::Result<AgentClient> {
    if target == "local" {
        runway_core::local_agent::get_or_start_local_agent().await
    } else {
        let store = get_store()?;
        let t = store
            .get_target_by_name(target)?
            .ok_or_else(|| anyhow::anyhow!("Target '{}' not found", target))?;
        AgentClient::connect(&t.grpc_endpoint()).await
    }
}

fn find_project(store: &ProjectStore, identifier: &str) -> anyhow::Result<Project> {
    if let Ok(uuid) = identifier.parse::<uuid::Uuid>() {
        if let Some(p) = store.get(uuid)? {
            return Ok(p);
        }
    }
    let projects = store.list()?;
    projects
        .into_iter()
        .find(|p| p.name == identifier)
        .ok_or_else(|| anyhow::anyhow!("Project '{}' not found", identifier))
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let cli = Cli::parse();

    match cli.command {
        Commands::List => {
            let store = get_store()?;
            let projects = store.list()?;
            if projects.is_empty() {
                println!("No projects. Use 'runway deploy <path>' to create one.");
                return Ok(());
            }
            println!(
                "{:<36}  {:<20}  {:<10}  {:<8}  {}",
                "ID", "NAME", "RUNTIME", "STATUS", "RUNS"
            );
            for p in &projects {
                println!(
                    "{:<36}  {:<20}  {:<10}  {:<8}  {}",
                    p.id,
                    p.name,
                    format!("{:?}", p.runtime).to_lowercase(),
                    format!("{:?}", p.status).to_lowercase(),
                    p.run_count,
                );
            }
        }

        Commands::Deploy { path, name, target } => {
            let project_name = name.unwrap_or_else(|| {
                Path::new(&path)
                    .file_name()
                    .map(|n| n.to_string_lossy().to_string())
                    .unwrap_or("unnamed".into())
            });
            println!("Deploying '{}' to {} ...", project_name, target);

            let store = get_store()?;
            let info = runtime::detect_runtime(Path::new(&path));
            let mut project = Project::new(project_name.clone(), path, info.runtime);
            project.entrypoint = info.entrypoint;
            store.insert(&project)?;

            let entrypoint = match &project.entrypoint {
                Some(ep) => ep.clone(),
                None => {
                    println!(
                        "Project '{}' created but no entrypoint detected. Use 'runway detect' to check.",
                        project_name
                    );
                    return Ok(());
                }
            };

            store.record_run_start(project.id)?;
            let runtime_str = format!("{:?}", project.runtime).to_lowercase();
            println!("Runtime: {} | Entry: {}", runtime_str, entrypoint);

            let is_remote = target != "local";
            let code = if is_remote {
                let code_path = Path::new(&project.path).join(&entrypoint);
                Some(std::fs::read(&code_path)?)
            } else {
                None
            };
            let working_dir = if is_remote { "/tmp" } else { &project.path };

            let mut client = get_agent_client(&target).await?;
            let mut stream = client
                .execute_streaming(
                    &project.id.to_string(),
                    &runtime_str,
                    &entrypoint,
                    working_dir,
                    code.as_deref(),
                    None,
                    std::collections::HashMap::new(),
                )
                .await?;

            let mut exit_code = 0i32;
            while let Some(msg) = stream.message().await? {
                if msg.is_final {
                    exit_code = msg.exit_code;
                    continue;
                }
                match msg.stream.as_str() {
                    "stderr" => eprintln!("\x1b[31m{}\x1b[0m", msg.text),
                    _ => println!("{}", msg.text),
                }
            }

            store.record_run_end(project.id, Some(exit_code))?;
            println!("\nExit code: {}", exit_code);
        }

        Commands::Run { project, target } => {
            let store = get_store()?;
            let p = find_project(&store, &project)?;
            let entrypoint = p
                .entrypoint
                .as_deref()
                .ok_or_else(|| anyhow::anyhow!("No entrypoint"))?;

            store.record_run_start(p.id)?;
            let runtime_str = format!("{:?}", p.runtime).to_lowercase();

            let is_remote = target != "local";
            let code = if is_remote {
                let code_path = Path::new(&p.path).join(entrypoint);
                Some(std::fs::read(&code_path)?)
            } else {
                None
            };
            let working_dir = if is_remote { "/tmp" } else { &p.path };

            let mut client = get_agent_client(&target).await?;
            let mut stream = client
                .execute_streaming(
                    &p.id.to_string(),
                    &runtime_str,
                    entrypoint,
                    working_dir,
                    code.as_deref(),
                    None,
                    std::collections::HashMap::new(),
                )
                .await?;

            let mut exit_code = 0i32;
            while let Some(msg) = stream.message().await? {
                if msg.is_final {
                    exit_code = msg.exit_code;
                    continue;
                }
                match msg.stream.as_str() {
                    "stderr" => eprintln!("\x1b[31m{}\x1b[0m", msg.text),
                    _ => println!("{}", msg.text),
                }
            }

            store.record_run_end(p.id, Some(exit_code))?;
        }

        Commands::Stop { project, target } => {
            let store = get_store()?;
            let p = find_project(&store, &project)?;

            let mut client = get_agent_client(&target).await?;
            let stopped = client.stop(&p.id.to_string()).await?;

            if stopped {
                store.update_status(p.id, runway_core::project::ProjectStatus::Stopped)?;
                println!("Project '{}' stopped", p.name);
            } else {
                println!("Project '{}' is not running", p.name);
            }
        }

        Commands::Logs { project, target, .. } => {
            let store = get_store()?;
            let p = find_project(&store, &project)?;
            println!("Running '{}' to capture logs...\n", p.name);

            let entrypoint = p
                .entrypoint
                .as_deref()
                .ok_or_else(|| anyhow::anyhow!("No entrypoint"))?;

            let runtime_str = format!("{:?}", p.runtime).to_lowercase();

            let mut client = get_agent_client(&target).await?;
            let mut stream = client
                .execute_streaming(
                    &p.id.to_string(),
                    &runtime_str,
                    entrypoint,
                    &p.path,
                    None,
                    None,
                    std::collections::HashMap::new(),
                )
                .await?;

            while let Some(msg) = stream.message().await? {
                if !msg.is_final {
                    println!("{}", msg.text);
                }
            }
        }

        Commands::Status { project } => {
            let store = get_store()?;
            let p = find_project(&store, &project)?;
            println!("Project: {}", p.name);
            println!("ID:      {}", p.id);
            println!("Path:    {}", p.path);
            println!("Runtime: {:?}", p.runtime);
            println!("Entry:   {}", p.entrypoint.unwrap_or("none".into()));
            println!("Status:  {:?}", p.status);
            println!("Runs:    {}", p.run_count);
            if let Some(t) = p.last_run_at {
                println!("Last run: {}", t.to_rfc3339());
            }
            if let Some(c) = p.last_exit_code {
                println!("Exit code: {}", c);
            }
        }

        Commands::Import { path, name } => {
            let project_name = name.unwrap_or_else(|| {
                Path::new(&path)
                    .file_name()
                    .map(|n| n.to_string_lossy().to_string())
                    .unwrap_or("unnamed".into())
            });

            let store = get_store()?;
            let info = runtime::detect_runtime(Path::new(&path));
            let mut project = Project::new(project_name.clone(), path, info.runtime);
            project.entrypoint = info.entrypoint.clone();
            store.insert(&project)?;

            println!(
                "Imported '{}' (runtime: {:?}, entry: {})",
                project_name,
                info.runtime,
                info.entrypoint.unwrap_or("none".into())
            );
        }

        Commands::Detect { path } => {
            let info = runtime::detect_runtime(Path::new(&path));
            println!("Runtime:    {:?}", info.runtime);
            println!(
                "Entrypoint: {}",
                info.entrypoint.unwrap_or("none".into())
            );
            println!("Confidence: {:.0}%", info.confidence * 100.0);
            if let Some(vf) = info.version_file {
                println!("Marker:     {}", vf);
            }
            if !info.dependencies.is_empty() {
                println!(
                    "Deps:       {} ({})",
                    info.dependencies.len(),
                    info.dependencies
                        .iter()
                        .take(10)
                        .cloned()
                        .collect::<Vec<_>>()
                        .join(", ")
                );
            }
            if !info.scripts.is_empty() {
                println!("Scripts:    {}", info.scripts.keys().cloned().collect::<Vec<_>>().join(", "));
            }
        }

        Commands::Delete { project } => {
            let store = get_store()?;
            let p = find_project(&store, &project)?;
            store.delete(p.id)?;
            println!("Deleted project '{}'", p.name);
        }

        Commands::Health => {
            let store = get_store()?;
            let count = store.list().map(|p| p.len()).unwrap_or(0);
            println!("Runway v{}", env!("CARGO_PKG_VERSION"));
            println!("Status:   healthy");
            println!("Projects: {}", count);
            println!("Platform: {}", std::env::consts::OS);
        }

        Commands::Targets { action } => {
            let store = get_store()?;
            match action {
                TargetActions::List => {
                    println!("local  (built-in, always available)");
                    let targets = store.list_targets()?;
                    if !targets.is_empty() {
                        println!(
                            "\n{:<20}  {:<20}  {:<6}  {:<8}  {}",
                            "NAME", "HOST", "PORT", "STATUS", "AGENT"
                        );
                        for t in &targets {
                            println!(
                                "{:<20}  {:<20}  {:<6}  {:<8}  {}",
                                t.name,
                                t.host.as_deref().unwrap_or("-"),
                                t.port,
                                format!("{:?}", t.status).to_lowercase(),
                                t.agent_version.as_deref().unwrap_or("-"),
                            );
                        }
                    }
                }
                TargetActions::Add { name, host, port } => {
                    let target = Target::new_remote(name.clone(), host, port);
                    store.insert_target(&target)?;
                    println!("Target '{}' added ({}:{})", name, target.host.as_deref().unwrap_or("?"), port);
                }
                TargetActions::Remove { name } => {
                    let target = store
                        .get_target_by_name(&name)?
                        .ok_or_else(|| anyhow::anyhow!("Target '{}' not found", name))?;
                    store.delete_target(target.id)?;
                    println!("Target '{}' removed", name);
                }
                TargetActions::Ping { name } => {
                    let target = store
                        .get_target_by_name(&name)?
                        .ok_or_else(|| anyhow::anyhow!("Target '{}' not found", name))?;
                    let endpoint = target.grpc_endpoint();
                    println!("Pinging {} ...", endpoint);
                    match AgentClient::connect(&endpoint).await {
                        Ok(mut client) => match client.health().await {
                            Ok(health) => {
                                store.update_target_status(
                                    target.id,
                                    runway_core::target::TargetStatus::Online,
                                    Some(&health.version),
                                )?;
                                println!("Agent: {} v{}", health.status, health.version);
                                println!("Uptime: {}s", health.uptime_seconds);
                            }
                            Err(e) => {
                                store.update_target_status(
                                    target.id,
                                    runway_core::target::TargetStatus::Offline,
                                    None,
                                )?;
                                println!("Agent unhealthy: {}", e);
                            }
                        },
                        Err(e) => {
                            store.update_target_status(
                                target.id,
                                runway_core::target::TargetStatus::Offline,
                                None,
                            )?;
                            println!("Connection failed: {}", e);
                        }
                    }
                }
            }
        }

        Commands::Schedule { action } => {
            let store = get_store()?;
            match action {
                ScheduleActions::Add { project, cron } => {
                    let p = find_project(&store, &project)?;
                    let sched = Schedule::new(p.id, cron.clone());
                    store.insert_schedule(&sched)?;
                    println!(
                        "Schedule created: {} (next run: {})",
                        sched.id,
                        sched
                            .next_run_at
                            .map(|t| t.to_rfc3339())
                            .unwrap_or("unknown".into())
                    );
                }
                ScheduleActions::List => {
                    let schedules = store.list_schedules()?;
                    if schedules.is_empty() {
                        println!("No schedules. Use 'runway schedule add <project> --cron \"@every 5m\"'");
                        return Ok(());
                    }
                    println!(
                        "{:<36}  {:<36}  {:<16}  {:<8}  {}",
                        "SCHEDULE ID", "PROJECT ID", "CRON", "ENABLED", "NEXT RUN"
                    );
                    for s in &schedules {
                        println!(
                            "{:<36}  {:<36}  {:<16}  {:<8}  {}",
                            s.id,
                            s.project_id,
                            s.cron_expr,
                            if s.enabled { "yes" } else { "no" },
                            s.next_run_at
                                .map(|t| t.to_rfc3339())
                                .unwrap_or("-".into()),
                        );
                    }
                }
                ScheduleActions::Remove { id } => {
                    let uuid: uuid::Uuid = id.parse().map_err(|_| anyhow::anyhow!("Invalid UUID: {}", id))?;
                    store.delete_schedule(uuid)?;
                    println!("Schedule deleted");
                }
                ScheduleActions::Enable { id } => {
                    let uuid: uuid::Uuid = id.parse().map_err(|_| anyhow::anyhow!("Invalid UUID: {}", id))?;
                    store.set_schedule_enabled(uuid, true)?;
                    println!("Schedule enabled");
                }
                ScheduleActions::Disable { id } => {
                    let uuid: uuid::Uuid = id.parse().map_err(|_| anyhow::anyhow!("Invalid UUID: {}", id))?;
                    store.set_schedule_enabled(uuid, false)?;
                    println!("Schedule disabled");
                }
                ScheduleActions::Tick => {
                    let results = scheduler::tick(&store).await;
                    if results.is_empty() {
                        println!("No schedules due");
                    } else {
                        for (project_id, result) in &results {
                            match result {
                                Ok(code) => println!("Project {} ran (exit code: {})", project_id, code),
                                Err(e) => println!("Project {} failed: {}", project_id, e),
                            }
                        }
                    }
                }
            }
        },
    }

    Ok(())
}

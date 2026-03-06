use std::path::Path;

use clap::{Parser, Subcommand};
use runway_core::executor;
use runway_core::project::Project;
use runway_core::runtime;
use runway_core::store::ProjectStore;

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
    },
    /// Stop a running project
    Stop {
        /// Project name or UUID
        project: String,
    },
    /// View logs for a project (run and capture)
    Logs {
        /// Project name or UUID
        project: String,
        /// Follow log output
        #[arg(long, short)]
        follow: bool,
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
}

#[derive(Subcommand)]
enum TargetActions {
    /// List configured targets
    List,
    /// Add a new target
    Add {
        name: String,
        #[arg(long)]
        host: Option<String>,
    },
}

fn get_store() -> anyhow::Result<ProjectStore> {
    let data_dir = dirs_next::data_dir()
        .unwrap_or_else(|| std::path::PathBuf::from("."))
        .join("com.runway.app");
    std::fs::create_dir_all(&data_dir)?;
    let db_path = data_dir.join("runway.db");
    ProjectStore::open(db_path.to_str().unwrap_or("runway.db"))
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
            println!("Runtime: {:?} | Entry: {}", project.runtime, entrypoint);

            let (mut child, mut rx) =
                executor::spawn_and_stream(project.runtime, &entrypoint, &project.path).await?;

            while let Some(line) = rx.recv().await {
                match line.stream {
                    executor::LogStream::Stdout => println!("{}", line.text),
                    executor::LogStream::Stderr => eprintln!("\x1b[31m{}\x1b[0m", line.text),
                }
            }

            let exit_code = child.wait().await.ok().and_then(|s| s.code());
            store.record_run_end(project.id, exit_code)?;
            println!(
                "\nExit code: {}",
                exit_code.map(|c| c.to_string()).unwrap_or("unknown".into())
            );
        }

        Commands::Run { project } => {
            let store = get_store()?;
            let p = find_project(&store, &project)?;
            let entrypoint = p
                .entrypoint
                .as_deref()
                .ok_or_else(|| anyhow::anyhow!("No entrypoint"))?;

            store.record_run_start(p.id)?;

            let (mut child, mut rx) =
                executor::spawn_and_stream(p.runtime, entrypoint, &p.path).await?;

            while let Some(line) = rx.recv().await {
                match line.stream {
                    executor::LogStream::Stdout => println!("{}", line.text),
                    executor::LogStream::Stderr => eprintln!("\x1b[31m{}\x1b[0m", line.text),
                }
            }

            let exit_code = child.wait().await.ok().and_then(|s| s.code());
            store.record_run_end(p.id, exit_code)?;
        }

        Commands::Stop { project } => {
            let store = get_store()?;
            let p = find_project(&store, &project)?;
            store.update_status(p.id, runway_core::project::ProjectStatus::Stopped)?;
            println!("Project '{}' marked as stopped", p.name);
        }

        Commands::Logs { project, .. } => {
            let store = get_store()?;
            let p = find_project(&store, &project)?;
            println!("Running '{}' to capture logs...\n", p.name);

            let entrypoint = p
                .entrypoint
                .as_deref()
                .ok_or_else(|| anyhow::anyhow!("No entrypoint"))?;

            let (_child, mut rx) =
                executor::spawn_and_stream(p.runtime, entrypoint, &p.path).await?;

            while let Some(line) = rx.recv().await {
                println!("{}", line.text);
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

        Commands::Targets { action } => match action {
            TargetActions::List => {
                println!("local  (built-in, always available)");
                println!("\nRemote targets will be available in Phase 3.");
            }
            TargetActions::Add { name, .. } => {
                println!("Target '{}' — remote targets available in Phase 3.", name);
            }
        },
    }

    Ok(())
}

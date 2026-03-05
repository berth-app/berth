use clap::{Parser, Subcommand};

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
    /// Deploy code to a target
    Deploy {
        /// Path to code directory or file
        path: String,
        /// Target to deploy to
        #[arg(long)]
        target: Option<String>,
    },
    /// View logs for a project
    Logs {
        /// Project name or ID
        project: String,
        /// Follow log output
        #[arg(long, short)]
        follow: bool,
    },
    /// Check status of a project
    Status {
        /// Project name or ID
        project: String,
    },
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
        /// Target name
        name: String,
        /// Host address
        #[arg(long)]
        host: Option<String>,
    },
}

fn main() {
    let cli = Cli::parse();

    match cli.command {
        Commands::List => println!("runway list — Phase 2"),
        Commands::Deploy { path, target } => {
            println!("runway deploy {path} --target {} — Phase 2", target.unwrap_or("local".into()))
        }
        Commands::Logs { project, follow } => {
            println!("runway logs {project} --follow={follow} — Phase 2")
        }
        Commands::Status { project } => println!("runway status {project} — Phase 2"),
        Commands::Targets { action } => match action {
            TargetActions::List => println!("runway targets list — Phase 3"),
            TargetActions::Add { name, .. } => println!("runway targets add {name} — Phase 3"),
        },
    }
}

mod agent_scheduler;
mod agent_store;
mod persistent_service;
mod service;

use std::sync::Arc;

use clap::Parser;
use tokio::sync::Mutex;
use tonic::transport::Server;

use runway_core::agent_client::proto::agent_service_server::AgentServiceServer;

use agent_store::AgentStore;
use persistent_service::PersistentAgentService;

#[derive(Parser)]
#[command(name = "runway-agent", version, about = "Runway deployment agent")]
struct Cli {
    /// Bind to 0.0.0.0 instead of 127.0.0.1
    #[arg(long)]
    listen_all: bool,

    /// Port for gRPC server
    #[arg(long, default_value_t = 50051)]
    port: u16,

    /// Print version and exit (alias for clap's --version)
    #[arg(long = "show-version")]
    show_version: bool,
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt::init();

    let cli = Cli::parse();

    if cli.show_version {
        println!("runway-agent {}", env!("CARGO_PKG_VERSION"));
        return Ok(());
    }

    // Open agent-side SQLite store
    let db_dir = dirs_next::home_dir()
        .unwrap_or_else(|| std::path::PathBuf::from("/tmp"))
        .join(".runway");
    std::fs::create_dir_all(&db_dir)?;
    let db_path = db_dir.join("agent.db");

    let store = AgentStore::open(db_path.to_str().unwrap_or("/tmp/runway-agent.db"))?;

    // Vacuum on startup
    if let Err(e) = store.vacuum() {
        tracing::warn!("VACUUM failed (non-fatal): {e}");
    }

    let store = Arc::new(Mutex::new(store));

    let service = PersistentAgentService::new(store.clone());

    // Start agent-side scheduler
    let sched_store = store.clone();
    tokio::spawn(async move {
        // Wait for service to be ready
        tokio::time::sleep(tokio::time::Duration::from_secs(5)).await;
        loop {
            agent_scheduler::tick(&sched_store).await;
            tokio::time::sleep(tokio::time::Duration::from_secs(30)).await;
        }
    });

    let addr = if cli.listen_all {
        format!("0.0.0.0:{}", cli.port)
    } else {
        format!("127.0.0.1:{}", cli.port)
    };

    let addr = addr.parse()?;

    tracing::info!("Runway agent listening on {} (persistent mode)", addr);

    Server::builder()
        .add_service(AgentServiceServer::new(service))
        .serve(addr)
        .await?;

    Ok(())
}

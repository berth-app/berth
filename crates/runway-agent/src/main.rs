mod agent_scheduler;
mod agent_store;
mod nats_cmd_handler;
mod nats_publisher;
mod persistent_service;
mod service;

use std::sync::Arc;

use clap::Parser;
use tokio::sync::Mutex;
use tonic::transport::Server;

use runway_core::agent_client::proto::agent_service_server::AgentServiceServer;
use runway_core::nats_relay::NatsConfig;

use agent_store::AgentStore;
use nats_publisher::NatsPublisher;
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

    /// NATS server URL (enables NATS relay when set)
    #[arg(long, env = "RUNWAY_NATS_URL")]
    nats_url: Option<String>,

    /// Path to NATS credentials file
    #[arg(long, env = "RUNWAY_NATS_CREDS")]
    nats_creds: Option<String>,

    /// Agent ID for NATS subjects (defaults to hostname)
    #[arg(long, env = "RUNWAY_NATS_AGENT_ID")]
    nats_agent_id: Option<String>,
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

    // Initialize NATS publisher if configured
    let nats_publisher: Option<Arc<NatsPublisher>> = if let Some(nats_url) = &cli.nats_url {
        let agent_id = cli
            .nats_agent_id
            .clone()
            .or_else(|| sysinfo::System::host_name())
            .unwrap_or_else(|| "unknown-agent".to_string());

        let config = NatsConfig {
            url: nats_url.clone(),
            creds_path: cli.nats_creds.clone(),
            agent_id,
        };

        match NatsPublisher::connect(&config).await {
            Ok(publisher) => {
                let publisher = Arc::new(publisher);

                // Spawn heartbeat loop
                let hb_publisher = publisher.clone();
                let start_time = std::time::Instant::now();
                let version = env!("CARGO_PKG_VERSION").to_string();
                tokio::spawn(async move {
                    loop {
                        let sys = sysinfo::System::new_all();
                        let cpu_usage = sys.global_cpu_usage();
                        let memory_bytes = sys.used_memory();
                        let uptime = start_time.elapsed().as_secs();

                        hb_publisher
                            .publish_heartbeat(&version, uptime, cpu_usage, memory_bytes)
                            .await;

                        tokio::time::sleep(tokio::time::Duration::from_secs(30)).await;
                    }
                });

                Some(publisher)
            }
            Err(e) => {
                tracing::warn!("Failed to connect to NATS (continuing without relay): {e}");
                None
            }
        }
    } else {
        None
    };

    let service = Arc::new(PersistentAgentService::new(store.clone(), nats_publisher.clone()));

    // Start NATS command handler if configured
    if let Some(ref publisher) = nats_publisher {
        let agent_id = cli
            .nats_agent_id
            .clone()
            .or_else(|| sysinfo::System::host_name())
            .unwrap_or_else(|| "unknown-agent".to_string());

        let handler = nats_cmd_handler::NatsCommandHandler::new(
            publisher.client().clone(),
            agent_id,
            service.clone(),
        );
        tokio::spawn(handler.run());
    }

    // Start agent-side scheduler
    let sched_store = store.clone();
    let sched_nats = nats_publisher.clone();
    tokio::spawn(async move {
        // Wait for service to be ready
        tokio::time::sleep(tokio::time::Duration::from_secs(5)).await;
        loop {
            agent_scheduler::tick(&sched_store, &sched_nats).await;
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
    if nats_publisher.is_some() {
        tracing::info!("NATS relay enabled");
    }

    Server::builder()
        .add_service(AgentServiceServer::from_arc(service))
        .serve(addr)
        .await?;

    Ok(())
}

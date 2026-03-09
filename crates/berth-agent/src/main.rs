mod agent_scheduler;
mod agent_store;
mod archive;
mod container;
mod containerfile;
mod executor;
mod nats_cmd_handler;
mod nats_publisher;
mod pairing;
mod persistent_service;
mod setup;
#[allow(dead_code)]
mod tunnel;
mod update;

use std::net::SocketAddr;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use clap::{Parser, Subcommand};
use tokio::sync::Mutex;
use tonic::transport::Server;

use berth_proto::proto::agent_service_server::AgentServiceServer;
use berth_proto::nats_relay::NatsConfig;

use agent_store::AgentStore;
use nats_publisher::NatsPublisher;
use persistent_service::PersistentAgentService;

/// Exit code that tells systemd "I upgraded/rolled back, just restart me."
/// systemd SuccessExitStatus=42 ensures this doesn't count toward rate limiting.
const EXIT_CODE_UPGRADE: i32 = 42;

#[derive(Parser)]
#[command(name = "berth-agent", version, about = "Berth deployment agent")]
struct Cli {
    #[command(subcommand)]
    command: Option<Commands>,

    /// Bind to 0.0.0.0 instead of 127.0.0.1
    #[arg(long)]
    listen_all: bool,

    /// Port for gRPC server
    #[arg(long, default_value_t = 50051, env = "BERTH_PORT")]
    port: u16,

    /// Print version and exit (alias for clap's --version)
    #[arg(long = "show-version")]
    show_version: bool,

    /// NATS server URL (enables NATS relay when set)
    #[arg(long, env = "BERTH_NATS_URL")]
    nats_url: Option<String>,

    /// Path to NATS credentials file
    #[arg(long, env = "BERTH_NATS_CREDS")]
    nats_creds: Option<String>,

    /// Agent ID for NATS subjects (defaults to hostname)
    #[arg(long, env = "BERTH_NATS_AGENT_ID")]
    nats_agent_id: Option<String>,

    /// Owner ID (set during pairing, stored in agent.db)
    #[arg(long, env = "BERTH_OWNER_ID")]
    owner_id: Option<String>,
}

#[derive(Subcommand)]
enum Commands {
    /// Check for updates and self-upgrade
    Update {
        /// Specific version to update to (default: latest)
        #[arg(long)]
        version: Option<String>,
        /// Skip confirmation prompt
        #[arg(long, short)]
        yes: bool,
    },
}

/// Probation health check: TCP-connect to our own gRPC port repeatedly
/// to verify the server is actually accepting connections.
/// Returns true if probation passed, false if it should rollback.
async fn run_probation(addr: SocketAddr, berth_dir: &Path) -> bool {
    // Wait for the gRPC server to start
    tokio::time::sleep(tokio::time::Duration::from_secs(3)).await;

    let mut successes = 0;
    let required = 3;
    let max_checks = 6; // 6 checks × 5s = 30s window

    for i in 0..max_checks {
        match tokio::net::TcpStream::connect(addr).await {
            Ok(_) => {
                successes += 1;
                tracing::info!("Probation check {}/{}: OK ({}/{})", i + 1, max_checks, successes, required);
                if successes >= required {
                    // Probation passed
                    let _ = std::fs::remove_file(berth_dir.join(".probation"));
                    let _ = std::fs::remove_file(berth_dir.join(".rollback-count"));
                    let _ = std::fs::write(
                        berth_dir.join(".probation-passed"),
                        serde_json::json!({
                            "passed_at": chrono::Utc::now().to_rfc3339(),
                            "version": env!("CARGO_PKG_VERSION"),
                            "checks": successes,
                        }).to_string(),
                    );
                    tracing::info!("Probation passed after {} successful checks", successes);
                    return true;
                }
            }
            Err(e) => {
                tracing::warn!("Probation check {}/{}: FAIL ({})", i + 1, max_checks, e);
            }
        }
        tokio::time::sleep(tokio::time::Duration::from_secs(5)).await;
    }

    tracing::error!("Probation failed: only {}/{} checks passed", successes, required);
    false
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let cli = Cli::parse();

    if cli.show_version {
        println!("berth-agent {}", env!("CARGO_PKG_VERSION"));
        return Ok(());
    }

    // Handle subcommands before initializing the full agent
    if let Some(Commands::Update { version, yes }) = cli.command {
        return update::run_update(version.as_deref(), yes).await;
    }

    tracing_subscriber::fmt::init();

    // Open agent-side SQLite store
    let db_dir = dirs_next::home_dir()
        .unwrap_or_else(|| PathBuf::from("/tmp"))
        .join(".berth");
    std::fs::create_dir_all(&db_dir)?;
    let db_path = db_dir.join("agent.db");

    let store = AgentStore::open(db_path.to_str().unwrap_or("/tmp/berth-agent.db"))?;

    // Vacuum on startup
    if let Err(e) = store.vacuum() {
        tracing::warn!("VACUUM failed (non-fatal): {e}");
    }

    let store = Arc::new(Mutex::new(store));

    // Resolve agent_id
    let agent_id = cli
        .nats_agent_id
        .clone()
        .or_else(|| sysinfo::System::host_name())
        .unwrap_or_else(|| "unknown-agent".to_string());

    // Resolve owner_id: CLI/env → SQLite config → pairing mode
    let owner_id: Option<String> = cli.owner_id.clone().or_else(|| {
        let s = store.blocking_lock();
        s.get_config("owner_id").ok().flatten()
    });

    let env_file = db_dir.join("agent.env");

    // Initialize NATS publisher if configured
    let nats_publisher: Option<Arc<NatsPublisher>> = if let Some(nats_url) = &cli.nats_url {
        // If no owner_id, we need to pair first. Connect to NATS for pairing.
        let owner_id = if let Some(oid) = owner_id.clone() {
            oid
        } else {
            tracing::info!("No owner_id configured — entering pairing mode");

            // Connect to NATS just for pairing
            let mut opts = async_nats::ConnectOptions::new();
            if let Some(ref creds_path) = cli.nats_creds {
                match opts.credentials_file(creds_path).await {
                    Ok(new_opts) => opts = new_opts,
                    Err(e) => {
                        tracing::warn!("Failed to load NATS credentials for pairing: {e}");
                        opts = async_nats::ConnectOptions::new();
                    }
                }
            }
            let pairing_client = match opts.connect(nats_url).await {
                Ok(c) => c,
                Err(e) => {
                    tracing::error!("Cannot connect to NATS for pairing: {e}");
                    tracing::error!("Agent cannot start without pairing. Configure BERTH_OWNER_ID or fix NATS connection.");
                    std::process::exit(1);
                }
            };

            let pairing_service = pairing::PairingService::new(
                pairing_client.clone(),
                agent_id.clone(),
                store.clone(),
                env_file.clone(),
            );

            match pairing_service.run().await {
                Some(result) => {
                    tracing::info!("Pairing complete. Owner: {}", result.owner_id);
                    result.owner_id
                }
                None => {
                    tracing::error!("Pairing failed — agent cannot start without an owner");
                    std::process::exit(1);
                }
            }
        };

        let config = NatsConfig {
            url: nats_url.clone(),
            creds_path: cli.nats_creds.clone(),
            agent_id: agent_id.clone(),
            owner_id: owner_id.clone(),
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
        // owner_id is guaranteed to exist here (pairing blocks until set)
        let handler_owner_id = cli.owner_id.clone().or_else(|| {
            let s = store.blocking_lock();
            s.get_config("owner_id").ok().flatten()
        }).unwrap_or_default();

        let handler = nats_cmd_handler::NatsCommandHandler::new(
            publisher.client().clone(),
            agent_id.clone(),
            handler_owner_id,
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

    let addr: SocketAddr = if cli.listen_all {
        format!("0.0.0.0:{}", cli.port)
    } else {
        format!("127.0.0.1:{}", cli.port)
    }
    .parse()?;

    tracing::info!("Berth agent listening on {} (persistent mode)", addr);
    if nats_publisher.is_some() {
        tracing::info!("NATS relay enabled");
    }

    // Check probation state
    let berth_dir = db_dir.clone(); // ~/.berth
    let probation_file = berth_dir.join(".probation");
    let backup_path = berth_dir.join("bin/berth-agent.old");
    let in_probation = probation_file.exists() && backup_path.exists();

    // Clean up .upgrading marker from a previous upgrade/rollback exit
    let _ = std::fs::remove_file(berth_dir.join(".upgrading"));

    if in_probation {
        tracing::info!("Probation mode active — running health checks for 30s");
    } else {
        // Not in probation — check for post-upgrade/rollback status from previous run
        service.check_post_upgrade_status().await;
    }

    // Spawn gRPC server as a task (non-blocking)
    let server_service = service.clone();
    let server_handle = tokio::spawn(async move {
        Server::builder()
            .add_service(AgentServiceServer::from_arc(server_service))
            .serve(addr)
            .await
    });

    if in_probation {
        let probation_dir = berth_dir.clone();
        let probation_handle = tokio::spawn(async move {
            run_probation(addr, &probation_dir).await
        });

        tokio::select! {
            server_result = server_handle => {
                // Server crashed during probation — exit(1) so ExecStopPost
                // handles the rollback (swaps backup binary back)
                tracing::error!("gRPC server exited during probation: {:?}", server_result);
                std::process::exit(1);
            }
            probation_result = probation_handle => {
                match probation_result {
                    Ok(true) => {
                        tracing::info!("Probation passed, running normally");
                        tokio::signal::ctrl_c().await.ok();
                    }
                    Ok(false) => {
                        // Probation failed — exit(1) so ExecStopPost rolls back
                        tracing::error!("Probation failed, exiting for rollback");
                        std::process::exit(1);
                    }
                    Err(e) => {
                        tracing::error!("Probation task panicked: {e}");
                        std::process::exit(1);
                    }
                }
            }
        }
    } else {
        // Normal mode — just await the server
        server_handle.await??;
    }

    Ok(())
}

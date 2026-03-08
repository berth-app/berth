mod agent_scheduler;
mod agent_store;
mod nats_cmd_handler;
mod nats_publisher;
mod persistent_service;
mod service;

use std::net::SocketAddr;
use std::path::{Path, PathBuf};
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

/// Probation health check: TCP-connect to our own gRPC port repeatedly
/// to verify the server is actually accepting connections.
/// Returns true if probation passed, false if it should rollback.
async fn run_probation(addr: SocketAddr, runway_dir: &Path) -> bool {
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
                    let _ = std::fs::remove_file(runway_dir.join(".probation"));
                    let _ = std::fs::remove_file(runway_dir.join(".rollback-count"));
                    let _ = std::fs::write(
                        runway_dir.join(".probation-passed"),
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

/// Self-rollback: copy the backup binary over the current exe and clean up.
fn do_self_rollback(runway_dir: &Path) {
    let backup_path = runway_dir.join("runway-agent.old");
    let count_file = runway_dir.join(".rollback-count");
    let probation_file = runway_dir.join(".probation");

    let count: u32 = std::fs::read_to_string(&count_file)
        .ok()
        .and_then(|s| s.trim().parse().ok())
        .unwrap_or(0);

    if count >= 2 {
        tracing::error!("Rollback loop detected (count={}), not rolling back again", count);
        let _ = std::fs::remove_file(&probation_file);
        let _ = std::fs::remove_file(&count_file);
        return;
    }

    let current_exe = match std::env::current_exe() {
        Ok(p) => p,
        Err(e) => {
            tracing::error!("Cannot determine current exe for rollback: {e}");
            return;
        }
    };

    if !backup_path.exists() {
        tracing::error!("No backup binary at {} — cannot rollback", backup_path.display());
        let _ = std::fs::remove_file(&probation_file);
        return;
    }

    match std::fs::copy(&backup_path, &current_exe) {
        Ok(_) => {
            tracing::warn!("Self-rollback: restored backup binary");
            let _ = std::fs::write(&count_file, format!("{}", count + 1));
            let _ = std::fs::remove_file(&probation_file);
        }
        Err(e) => {
            tracing::error!("Self-rollback failed: {e}");
        }
    }
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
        .unwrap_or_else(|| PathBuf::from("/tmp"))
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

    let addr: SocketAddr = if cli.listen_all {
        format!("0.0.0.0:{}", cli.port)
    } else {
        format!("127.0.0.1:{}", cli.port)
    }
    .parse()?;

    tracing::info!("Runway agent listening on {} (persistent mode)", addr);
    if nats_publisher.is_some() {
        tracing::info!("NATS relay enabled");
    }

    // Check probation state
    let runway_dir = db_dir.clone(); // ~/.runway
    let probation_file = runway_dir.join(".probation");
    let backup_path = runway_dir.join("runway-agent.old");
    let in_probation = probation_file.exists() && backup_path.exists();

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
        let probation_dir = runway_dir.clone();
        let probation_handle = tokio::spawn(async move {
            run_probation(addr, &probation_dir).await
        });

        tokio::select! {
            server_result = server_handle => {
                // Server crashed during probation
                tracing::error!("gRPC server exited during probation: {:?}", server_result);
                do_self_rollback(&runway_dir);
                std::process::exit(42);
            }
            probation_result = probation_handle => {
                match probation_result {
                    Ok(true) => {
                        tracing::info!("Probation passed, running normally");
                        // Server is already running in background, wait forever
                        // (server_handle was moved into select, so we just loop)
                        tokio::signal::ctrl_c().await.ok();
                    }
                    Ok(false) => {
                        tracing::error!("Probation failed, self-rolling back");
                        do_self_rollback(&runway_dir);
                        std::process::exit(42);
                    }
                    Err(e) => {
                        tracing::error!("Probation task panicked: {e}");
                        do_self_rollback(&runway_dir);
                        std::process::exit(42);
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

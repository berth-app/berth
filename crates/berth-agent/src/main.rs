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

    /// Path to TLS server certificate (PEM) for mTLS direct connections
    #[arg(long, env = "BERTH_TLS_CERT")]
    tls_cert: Option<String>,

    /// Path to TLS server private key (PEM)
    #[arg(long, env = "BERTH_TLS_KEY")]
    tls_key: Option<String>,

    /// Path to CA certificate (PEM) for client verification (mTLS)
    #[arg(long, env = "BERTH_TLS_CA")]
    tls_ca: Option<String>,
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
    /// Generate mTLS certificates for direct connections
    InitTls {
        /// Hostname for the server certificate SAN (defaults to system hostname)
        #[arg(long)]
        hostname: Option<String>,
    },
    /// Configure NATS credentials by pasting from Synadia Cloud
    SetupNats {
        /// NATS server URL (default: tls://connect.ngs.global)
        #[arg(long, default_value = "tls://connect.ngs.global")]
        url: String,
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

/// Generate mTLS certificates for direct connections.
async fn run_init_tls(hostname: Option<String>) -> anyhow::Result<()> {
    let berth_dir = dirs_next::home_dir()
        .unwrap_or_else(|| PathBuf::from("/tmp"))
        .join(".berth");
    let certs_dir = berth_dir.join("certs");
    std::fs::create_dir_all(&certs_dir)?;

    let hostname = hostname
        .or_else(|| sysinfo::System::host_name())
        .unwrap_or_else(|| "berth-agent".to_string());

    // Generate or load CA
    let ca = berth_core::tls::generate_ca()?;
    let ca_cert_pem = ca.pem();
    let ca_key_pem = ca.key().serialize_pem();

    // Save CA
    std::fs::write(certs_dir.join("ca.crt"), &ca_cert_pem)?;
    std::fs::write(certs_dir.join("ca.key"), &ca_key_pem)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(
            certs_dir.join("ca.key"),
            std::fs::Permissions::from_mode(0o600),
        )?;
    }

    // Generate server certificate
    let server_bundle = berth_core::tls::generate_server_cert(&ca, &hostname)?;
    std::fs::write(certs_dir.join("server.crt"), &server_bundle.cert_pem)?;
    std::fs::write(certs_dir.join("server.key"), &server_bundle.key_pem)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(
            certs_dir.join("server.key"),
            std::fs::Permissions::from_mode(0o600),
        )?;
    }

    // Generate client certificate for the desktop app
    let client_bundle = berth_core::tls::generate_client_cert(&ca, "berth-desktop")?;
    std::fs::write(certs_dir.join("client.crt"), &client_bundle.cert_pem)?;
    std::fs::write(certs_dir.join("client.key"), &client_bundle.key_pem)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(
            certs_dir.join("client.key"),
            std::fs::Permissions::from_mode(0o600),
        )?;
    }

    println!("mTLS certificates generated in {}", certs_dir.display());
    println!();
    println!("Server certificate hostname: {hostname}");
    println!();
    println!("Copy these files to your desktop machine for direct connection:");
    println!("  {}/ca.crt", certs_dir.display());
    println!("  {}/client.crt", certs_dir.display());
    println!("  {}/client.key", certs_dir.display());
    println!();
    println!("Add these to your agent.env:");
    println!("  BERTH_TLS_CERT={}/server.crt", certs_dir.display());
    println!("  BERTH_TLS_KEY={}/server.key", certs_dir.display());
    println!("  BERTH_TLS_CA={}/ca.crt", certs_dir.display());

    Ok(())
}

/// Configure NATS credentials interactively by reading from stdin.
async fn run_setup_nats(url: String) -> anyhow::Result<()> {
    let berth_dir = dirs_next::home_dir()
        .unwrap_or_else(|| PathBuf::from("/tmp"))
        .join(".berth");
    std::fs::create_dir_all(&berth_dir)?;

    println!("Paste your Synadia Cloud credentials below.");
    println!("The block should contain both a NATS USER JWT and USER NKEY SEED section.");
    println!("Press Ctrl+D (EOF) when done:\n");

    let mut input = String::new();
    std::io::Read::read_to_string(&mut std::io::stdin(), &mut input)?;
    let input = input.trim().to_string();

    if !input.contains("BEGIN NATS USER JWT") || !input.contains("BEGIN USER NKEY SEED") {
        anyhow::bail!(
            "Invalid credentials format. Expected a block containing both \
             '-----BEGIN NATS USER JWT-----' and '-----BEGIN USER NKEY SEED-----'."
        );
    }

    // Write credentials file
    let creds_path = berth_dir.join("nats.creds");
    std::fs::write(&creds_path, &input)?;

    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&creds_path, std::fs::Permissions::from_mode(0o600))?;
    }

    // Update agent.env
    let env_path = berth_dir.join("agent.env");
    let env_content = if env_path.exists() {
        std::fs::read_to_string(&env_path)?
    } else {
        String::new()
    };

    let mut lines: Vec<String> = env_content.lines().map(|l| l.to_string()).collect();

    // Update or add BERTH_NATS_CREDS
    let creds_line = format!("BERTH_NATS_CREDS={}", creds_path.display());
    if let Some(idx) = lines.iter().position(|l| l.starts_with("BERTH_NATS_CREDS=")) {
        lines[idx] = creds_line;
    } else {
        lines.push(creds_line);
    }

    // Update or add BERTH_NATS_URL
    let url_line = format!("BERTH_NATS_URL={url}");
    if let Some(idx) = lines.iter().position(|l| l.starts_with("BERTH_NATS_URL=")) {
        lines[idx] = url_line;
    } else {
        lines.push(url_line);
    }

    let new_content = lines.join("\n");
    std::fs::write(&env_path, &new_content)?;

    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&env_path, std::fs::Permissions::from_mode(0o600))?;
    }

    println!();
    println!("NATS credentials saved to {}", creds_path.display());
    println!("agent.env updated with BERTH_NATS_URL={url}");
    println!();
    println!("\x1b[31mThese credentials are sensitive. Never share them with anyone.\x1b[0m");
    println!();
    println!("Restart the agent to connect:");
    println!("  sudo systemctl restart berth-agent");

    Ok(())
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let cli = Cli::parse();

    if cli.show_version {
        println!("berth-agent {}", env!("CARGO_PKG_VERSION"));
        return Ok(());
    }

    // Handle subcommands before initializing the full agent
    match cli.command {
        Some(Commands::Update { version, yes }) => {
            return update::run_update(version.as_deref(), yes).await;
        }
        Some(Commands::InitTls { hostname }) => {
            return run_init_tls(hostname).await;
        }
        Some(Commands::SetupNats { url }) => {
            return run_setup_nats(url).await;
        }
        None => {}
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

    // Resolve agent_id: CLI/env → SQLite config → generate and persist
    let agent_id = if let Some(id) = cli.nats_agent_id.clone() {
        id
    } else {
        let s = store.lock().await;
        match s.get_config("agent_id").ok().flatten() {
            Some(id) => id,
            None => {
                // First start: generate a randomized agent_id to prevent prediction
                let hostname = sysinfo::System::host_name()
                    .unwrap_or_else(|| "agent".to_string());
                let suffix = &uuid::Uuid::new_v4().to_string()[..8];
                let new_id = format!("{hostname}-{suffix}");
                if let Err(e) = s.set_config("agent_id", &new_id) {
                    tracing::warn!("Failed to persist agent_id: {e}");
                }
                tracing::info!("Generated agent_id: {new_id}");
                new_id
            }
        }
    };

    // Resolve owner_id: CLI/env → SQLite config → pairing mode
    let owner_id: Option<String> = if let Some(oid) = cli.owner_id.clone() {
        Some(oid)
    } else {
        let s = store.lock().await;
        s.get_config("owner_id").ok().flatten()
    };

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
        let handler_owner_id = if let Some(oid) = cli.owner_id.clone() {
            oid
        } else {
            let s = store.lock().await;
            s.get_config("owner_id").ok().flatten().unwrap_or_default()
        };

        // Load shared secret for HMAC verification (established during pairing)
        let shared_secret: Option<Vec<u8>> = {
            let s = store.lock().await;
            s.get_config("shared_secret")
                .ok()
                .flatten()
                .and_then(|hex_str| hex::decode(&hex_str).ok())
        };

        let mut handler = nats_cmd_handler::NatsCommandHandler::new(
            publisher.client().clone(),
            agent_id.clone(),
            handler_owner_id,
            service.clone(),
        );
        if let Some(secret) = shared_secret {
            handler = handler.with_secret(secret);
        }
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

    // Determine connection mode and address binding
    let has_nats = nats_publisher.is_some();
    let has_tls = cli.tls_cert.is_some() && cli.tls_key.is_some() && cli.tls_ca.is_some();

    let addr: SocketAddr = if has_nats {
        // Synadia mode: gRPC restricted to localhost (NATS provides remote access)
        if cli.listen_all {
            tracing::info!("NATS relay mode — ignoring --listen-all, gRPC restricted to localhost");
        }
        format!("127.0.0.1:{}", cli.port).parse()?
    } else if cli.listen_all {
        // Direct mode: must have TLS to bind externally
        if !has_tls {
            tracing::error!("Cannot bind to 0.0.0.0 without TLS. Run `berth-agent init-tls` to generate certificates, then set BERTH_TLS_CERT, BERTH_TLS_KEY, and BERTH_TLS_CA.");
            std::process::exit(1);
        }
        format!("0.0.0.0:{}", cli.port).parse()?
    } else {
        format!("127.0.0.1:{}", cli.port).parse()?
    };

    if has_nats {
        tracing::info!("NATS relay enabled — gRPC on {} (localhost only)", addr);
    } else if has_tls {
        tracing::info!("Direct connection mode — mTLS enabled on {}", addr);
    } else {
        tracing::info!("Running in gRPC-only mode on {} (no NATS relay configured)", addr);
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
    let tls_cert = cli.tls_cert.clone();
    let tls_key = cli.tls_key.clone();
    let tls_ca = cli.tls_ca.clone();
    let server_handle = tokio::spawn(async move {
        if let (Some(cert_path), Some(key_path), Some(ca_path)) = (tls_cert, tls_key, tls_ca) {
            let cert_pem = std::fs::read_to_string(&cert_path)
                .unwrap_or_else(|e| panic!("Failed to read TLS cert {cert_path}: {e}"));
            let key_pem = std::fs::read_to_string(&key_path)
                .unwrap_or_else(|e| panic!("Failed to read TLS key {key_path}: {e}"));
            let ca_pem = std::fs::read_to_string(&ca_path)
                .unwrap_or_else(|e| panic!("Failed to read TLS CA {ca_path}: {e}"));

            let server_bundle = berth_core::tls::CertBundle { cert_pem, key_pem };
            let tls_config = berth_core::tls::server_tls_config(&server_bundle, &ca_pem)
                .expect("Failed to build TLS config");

            Server::builder()
                .tls_config(tls_config)
                .expect("Failed to configure TLS on gRPC server")
                .add_service(AgentServiceServer::from_arc(server_service))
                .serve(addr)
                .await
        } else {
            Server::builder()
                .add_service(AgentServiceServer::from_arc(server_service))
                .serve(addr)
                .await
        }
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

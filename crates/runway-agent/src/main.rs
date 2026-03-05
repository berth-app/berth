use clap::Parser;

#[derive(Parser)]
#[command(name = "runway-agent", about = "Runway deployment agent")]
struct Cli {
    /// Run in local mode (no gRPC server)
    #[arg(long, default_value_t = true)]
    local: bool,

    /// Port for gRPC server (Phase 3)
    #[arg(long, default_value_t = 50051)]
    port: u16,
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt::init();

    let cli = Cli::parse();

    if cli.local {
        tracing::info!("Runway agent running in local mode");
        // Phase 1: local mode — the Tauri app communicates directly via runway-core
        // Phase 3: will start a gRPC server here
        tokio::signal::ctrl_c().await?;
        tracing::info!("Shutting down");
    }

    Ok(())
}

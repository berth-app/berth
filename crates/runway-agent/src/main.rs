mod service;

use clap::Parser;
use tonic::transport::Server;

pub mod proto {
    tonic::include_proto!("runway");
}

use proto::agent_service_server::AgentServiceServer;
use service::AgentServiceImpl;

#[derive(Parser)]
#[command(name = "runway-agent", about = "Runway deployment agent")]
struct Cli {
    /// Bind to 0.0.0.0 instead of 127.0.0.1
    #[arg(long)]
    listen_all: bool,

    /// Port for gRPC server
    #[arg(long, default_value_t = 50051)]
    port: u16,
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt::init();

    let cli = Cli::parse();

    let addr = if cli.listen_all {
        format!("0.0.0.0:{}", cli.port)
    } else {
        format!("127.0.0.1:{}", cli.port)
    };

    let addr = addr.parse()?;
    let service = AgentServiceImpl::new();

    tracing::info!("Runway agent listening on {}", addr);

    Server::builder()
        .add_service(AgentServiceServer::new(service))
        .serve(addr)
        .await?;

    Ok(())
}

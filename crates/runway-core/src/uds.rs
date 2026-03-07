use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use tokio::net::UnixListener;
use tokio_stream::wrappers::UnixListenerStream;
use tonic::transport::{Channel, Endpoint, Server, Uri};
use tower::service_fn;

use crate::agent_client::proto::agent_service_server::AgentServiceServer;
use crate::agent_service::AgentServiceImpl;

/// Default path for the local agent Unix domain socket.
pub fn default_socket_path() -> PathBuf {
    dirs_next::home_dir()
        .unwrap_or_else(|| PathBuf::from("/tmp"))
        .join(".runway")
        .join("agent.sock")
}

/// Default path for the local agent lockfile.
pub fn default_lock_path() -> PathBuf {
    dirs_next::home_dir()
        .unwrap_or_else(|| PathBuf::from("/tmp"))
        .join(".runway")
        .join("agent.lock")
}

/// Start the agent gRPC server on a Unix domain socket.
pub async fn serve_uds(path: &Path, service: AgentServiceImpl) -> Result<()> {
    // Ensure parent directory exists
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)
            .context("Failed to create directory for agent socket")?;
    }

    // Remove stale socket file
    let _ = std::fs::remove_file(path);

    let listener = UnixListener::bind(path)
        .context("Failed to bind Unix domain socket — check permissions on ~/.runway/")?;

    tracing::info!("Local agent listening on {}", path.display());

    let stream = UnixListenerStream::new(listener);

    Server::builder()
        .add_service(AgentServiceServer::new(service))
        .serve_with_incoming(stream)
        .await
        .context("Agent gRPC server exited with error")?;

    Ok(())
}

/// Connect to an agent via Unix domain socket, returning a tonic Channel.
pub async fn connect_uds(path: &Path) -> Result<Channel> {
    let path = path.to_path_buf();

    // tonic requires a valid URI even for UDS; the actual connection is handled by the connector
    let channel = Endpoint::try_from("http://[::]:50051")
        .context("Failed to create endpoint for UDS")?
        .connect_with_connector(service_fn(move |_: Uri| {
            let path = path.clone();
            async move {
                let stream = tokio::net::UnixStream::connect(&path).await?;
                Ok::<_, std::io::Error>(hyper_util::rt::TokioIo::new(stream))
            }
        }))
        .await
        .context("Failed to connect to local agent via Unix socket — is the agent running?")?;

    Ok(channel)
}

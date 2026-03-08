use anyhow::{Context, Result};
use tokio::sync::OnceCell;

use crate::agent_client::AgentClient;
use crate::agent_service::AgentServiceImpl;
use crate::uds;

static LOCAL_AGENT: OnceCell<()> = OnceCell::const_new();

/// Get or start the local embedded agent, returning a connected client.
///
/// Uses a process-level OnceCell so the agent is started at most once.
/// Implements lockfile coordination: if another process already owns the socket,
/// connects to it instead of starting a new agent.
pub async fn get_or_start_local_agent() -> Result<AgentClient> {
    // Clone from the OnceCell — AgentClient needs to be cloneable or we return a new connection each time.
    // Since AgentClient wraps a tonic Channel (which is cheap to clone), we connect fresh each call
    // but only start the server once.
    let socket_path = uds::default_socket_path();
    let lock_path = uds::default_lock_path();

    // Try connecting to an existing agent first
    if lock_path.exists() {
        if let Ok(pid_str) = std::fs::read_to_string(&lock_path) {
            if let Ok(pid) = pid_str.trim().parse::<i32>() {
                // Check if the process is alive (kill with signal 0)
                if unsafe { libc::kill(pid, 0) } == 0 {
                    // Process is alive — try to connect
                    if let Ok(client) = AgentClient::connect_uds(&socket_path).await {
                        return Ok(client);
                    }
                }
            }
        }
        // Stale lockfile — clean up
        let _ = std::fs::remove_file(&lock_path);
    }

    // No existing agent — start one (only once per process)
    LOCAL_AGENT
        .get_or_try_init(|| async {
            start_local_agent(&socket_path, &lock_path).await
        })
        .await?;

    // Connect to the agent we just started
    AgentClient::connect_uds(&socket_path)
        .await
        .context("Failed to connect to local agent after starting it")
}

async fn start_local_agent(
    socket_path: &std::path::Path,
    lock_path: &std::path::Path,
) -> Result<()> {
    let service = AgentServiceImpl::new();
    let path = socket_path.to_path_buf();
    let lpath = lock_path.to_path_buf();

    // Write lockfile with our PID
    if let Some(parent) = lpath.parent() {
        std::fs::create_dir_all(parent)?;
    }
    std::fs::write(&lpath, format!("{}", std::process::id()))?;

    // Spawn the server as a background task
    tokio::spawn(async move {
        if let Err(e) = uds::serve_uds(&path, service).await {
            tracing::error!("Local agent exited with error: {}", e);
        }
        // Clean up lockfile on exit
        let _ = std::fs::remove_file(&lpath);
    });

    // Wait for the socket to become available
    let socket = socket_path.to_path_buf();
    for _ in 0..50 {
        if socket.exists() {
            // Try a quick connect to verify it's ready
            if tokio::net::UnixStream::connect(&socket).await.is_ok() {
                return Ok(());
            }
        }
        tokio::time::sleep(std::time::Duration::from_millis(50)).await;
    }

    anyhow::bail!(
        "Local agent failed to start within 2.5 seconds — check logs for errors"
    )
}

/// Clean up the local agent lockfile. Call on process exit.
pub fn cleanup_lockfile() {
    let lock_path = uds::default_lock_path();
    let socket_path = uds::default_socket_path();
    let _ = std::fs::remove_file(&lock_path);
    let _ = std::fs::remove_file(&socket_path);
}

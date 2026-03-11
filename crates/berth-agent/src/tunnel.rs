use std::collections::HashMap;
use std::process::Stdio;
use std::sync::Arc;

use anyhow::Result;
use chrono::{DateTime, Utc};
use tokio::process::{Child, Command};
use tokio::sync::Mutex;

/// Supported tunnel providers
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TunnelProvider {
    Cloudflared,
    // Future: Ngrok { auth_token: String },
    // Future: Custom { command_template: String },
}

/// Info about an active tunnel
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct TunnelInfo {
    pub project_id: String,
    pub provider: String,
    pub public_url: String,
    pub local_port: u16,
    pub started_at: DateTime<Utc>,
}

/// Manages active tunnel processes
pub struct TunnelManager {
    tunnels: Arc<Mutex<HashMap<String, TunnelHandle>>>,
}

struct TunnelHandle {
    child: Child,
    info: TunnelInfo,
}

impl TunnelManager {
    pub fn new() -> Self {
        Self {
            tunnels: Arc::new(Mutex::new(HashMap::new())),
        }
    }

    /// Start a tunnel for a project. Returns the public URL.
    pub async fn start(
        &self,
        project_id: &str,
        port: u16,
        provider: &TunnelProvider,
    ) -> Result<TunnelInfo> {
        // Stop existing tunnel for this project if any
        self.stop(project_id).await.ok();

        let (child, url) = match provider {
            TunnelProvider::Cloudflared => start_cloudflared(port).await?,
        };

        let info = TunnelInfo {
            project_id: project_id.to_string(),
            provider: provider_name(provider),
            public_url: url,
            local_port: port,
            started_at: Utc::now(),
        };

        self.tunnels.lock().await.insert(
            project_id.to_string(),
            TunnelHandle {
                child,
                info: info.clone(),
            },
        );

        Ok(info)
    }

    /// Stop a tunnel for a project.
    pub async fn stop(&self, project_id: &str) -> Result<()> {
        if let Some(mut handle) = self.tunnels.lock().await.remove(project_id) {
            handle.child.kill().await?;
        }
        Ok(())
    }

    /// Get info about an active tunnel.
    pub async fn get(&self, project_id: &str) -> Option<TunnelInfo> {
        self.tunnels
            .lock()
            .await
            .get(project_id)
            .map(|h| h.info.clone())
    }

    /// List all active tunnels.
    pub async fn list(&self) -> Vec<TunnelInfo> {
        self.tunnels
            .lock()
            .await
            .values()
            .map(|h| h.info.clone())
            .collect()
    }

    /// Detect which providers are available on this machine.
    pub fn available_providers() -> Vec<String> {
        let mut providers = Vec::new();
        if is_binary_available("cloudflared") {
            providers.push("cloudflared".to_string());
        }
        providers
    }
}

fn provider_name(p: &TunnelProvider) -> String {
    match p {
        TunnelProvider::Cloudflared => "cloudflared".to_string(),
    }
}

fn is_binary_available(name: &str) -> bool {
    std::process::Command::new(name)
        .arg("--version")
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .is_ok()
}

/// Spawn cloudflared and parse the public URL from stderr.
async fn start_cloudflared(port: u16) -> Result<(Child, String)> {
    let mut child = Command::new("cloudflared")
        .args(["tunnel", "--url", &format!("http://127.0.0.1:{port}")])
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .kill_on_drop(true)
        .spawn()
        .map_err(|e| {
            anyhow::anyhow!(
                "Failed to start cloudflared. Is it installed? \
                 Install: brew install cloudflared — Error: {e}"
            )
        })?;

    let url = parse_cloudflared_url(&mut child).await?;
    Ok((child, url))
}

/// Read stderr lines from cloudflared until we find the tunnel URL.
/// Timeout after 30 seconds.
/// IMPORTANT: After finding the URL, we spawn a background task to keep
/// draining stderr so cloudflared doesn't get SIGPIPE and die.
async fn parse_cloudflared_url(child: &mut Child) -> Result<String> {
    use tokio::io::{AsyncBufReadExt, BufReader};

    let stderr = child
        .stderr
        .take()
        .ok_or_else(|| anyhow::anyhow!("No stderr from cloudflared"))?;
    let mut reader = BufReader::new(stderr).lines();

    let url = tokio::time::timeout(std::time::Duration::from_secs(30), async {
        while let Some(line) = reader.next_line().await? {
            if let Some(url) = extract_url(&line, "trycloudflare.com") {
                return Ok(url);
            }
        }
        Err(anyhow::anyhow!(
            "cloudflared exited without providing a URL"
        ))
    })
    .await
    .map_err(|_| anyhow::anyhow!("Timed out waiting for cloudflared tunnel URL (30s)"))??;

    // Keep draining stderr in the background so cloudflared doesn't die from SIGPIPE
    tokio::spawn(async move {
        while let Ok(Some(_)) = reader.next_line().await {}
    });

    Ok(url)
}

/// Extract a URL containing the given domain from a log line.
fn extract_url(line: &str, domain: &str) -> Option<String> {
    if let Some(start) = line.find("https://") {
        let rest = &line[start..];
        let end = rest
            .find(|c: char| c.is_whitespace())
            .unwrap_or(rest.len());
        let url = &rest[..end];
        if url.contains(domain) {
            return Some(url.to_string());
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_extract_url() {
        let line = "2024-01-01 INF |  https://random-words-here.trycloudflare.com";
        assert_eq!(
            extract_url(line, "trycloudflare.com"),
            Some("https://random-words-here.trycloudflare.com".to_string())
        );

        let line = "no url here";
        assert_eq!(extract_url(line, "trycloudflare.com"), None);

        let line = "https://example.com not the right domain";
        assert_eq!(extract_url(line, "trycloudflare.com"), None);
    }
}

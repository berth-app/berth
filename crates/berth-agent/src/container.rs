use std::collections::HashMap;
use std::path::Path;
use std::process::Stdio;

use tokio::io::{AsyncBufReadExt, BufReader};
use tokio::process::{Child, Command};
use tokio::sync::mpsc;

use berth_proto::executor::{LogLine, LogStream};

/// Container runtime capabilities detected on this agent.
#[derive(Debug, Clone)]
pub struct ContainerCapabilities {
    pub podman_version: Option<String>,
    pub docker_version: Option<String>,
    pub compose_version: Option<String>,
    /// "docker compose" or "podman-compose"
    pub compose_tool: Option<String>,
    /// "podman", "docker", "both", or "none"
    pub preferred: String,
}

/// Parse version from `podman --version` output: "podman version 4.9.3"
fn parse_cli_version(output: &str) -> Option<String> {
    output
        .trim()
        .rsplit(' ')
        .next()
        .map(|v| v.trim().to_string())
        .filter(|v| !v.is_empty())
}

/// Check if Podman is installed and return its version.
/// Uses `podman --version` which doesn't require rootless namespaces.
pub fn check_podman_sync() -> Option<String> {
    std::process::Command::new("podman")
        .args(["--version"])
        .output()
        .ok()
        .filter(|o| o.status.success())
        .and_then(|o| parse_cli_version(&String::from_utf8_lossy(&o.stdout)))
}

/// Check if Docker is installed and return its version.
/// Uses `docker --version` output: "Docker version 24.0.7, build afdd53b"
pub fn check_docker_sync() -> Option<String> {
    std::process::Command::new("docker")
        .args(["--version"])
        .output()
        .ok()
        .filter(|o| o.status.success())
        .and_then(|o| {
            let out = String::from_utf8_lossy(&o.stdout);
            let trimmed = out.trim();
            // "Docker version 24.0.7, build afdd53b" → "24.0.7"
            trimmed
                .strip_prefix("Docker version ")
                .and_then(|rest| rest.split(',').next())
                .map(|v| v.trim().to_string())
                .or_else(|| parse_cli_version(trimmed))
        })
}

/// Check if Docker Compose or Podman Compose is installed.
/// Returns (version, tool_name) where tool_name is "docker compose" or "podman-compose".
pub fn check_compose_sync() -> Option<(String, String)> {
    // Try `docker compose version` first
    if let Some(v) = std::process::Command::new("docker")
        .args(["compose", "version", "--short"])
        .output()
        .ok()
        .filter(|o| o.status.success())
        .and_then(|o| {
            let v = String::from_utf8_lossy(&o.stdout).trim().to_string();
            if v.is_empty() { None } else { Some(v) }
        })
    {
        return Some((v, "docker compose".into()));
    }

    // Fallback: `podman-compose --version`
    if let Some(v) = std::process::Command::new("podman-compose")
        .args(["--version"])
        .output()
        .ok()
        .filter(|o| o.status.success())
        .and_then(|o| parse_cli_version(&String::from_utf8_lossy(&o.stdout)))
    {
        return Some((v, "podman-compose".into()));
    }

    None
}

/// Detect all container capabilities on this machine.
pub fn detect_capabilities() -> ContainerCapabilities {
    let podman_version = check_podman_sync();
    let docker_version = check_docker_sync();
    let compose = check_compose_sync();

    let preferred = match (&podman_version, &docker_version) {
        (Some(_), Some(_)) => "both".into(),
        (Some(_), None) => "podman".into(),
        (None, Some(_)) => "docker".into(),
        (None, None) => "none".into(),
    };

    ContainerCapabilities {
        podman_version,
        docker_version,
        compose_version: compose.as_ref().map(|(v, _)| v.clone()),
        compose_tool: compose.map(|(_, t)| t),
        preferred,
    }
}

impl ContainerCapabilities {
    /// The container runtime command to use ("podman" or "docker").
    /// Prefers podman when both are available.
    pub fn runtime_cmd(&self) -> Option<&str> {
        if self.podman_version.is_some() {
            Some("podman")
        } else if self.docker_version.is_some() {
            Some("docker")
        } else {
            None
        }
    }

    pub fn is_ready(&self) -> bool {
        self.podman_version.is_some() || self.docker_version.is_some()
    }
}

/// Check if Podman is installed (async version, for backward compat).
pub async fn check_podman() -> anyhow::Result<String> {
    check_podman_sync().ok_or_else(|| {
        anyhow::anyhow!(
            "Podman is not installed. Install it with: \
             apt-get install -y podman (Debian/Ubuntu) or \
             dnf install -y podman (Fedora/RHEL)"
        )
    })
}

/// Build an OCI image from a Containerfile and context directory.
/// Streams build output through the sender channel.
/// Returns the image tag on success.
pub async fn build_image(
    runtime: &str,
    project_id: &str,
    version: u32,
    containerfile: &str,
    context_dir: &Path,
    tx: mpsc::Sender<LogLine>,
) -> anyhow::Result<String> {
    let image_tag = format!("berth/{}:{}", project_id, version);

    // Write containerfile to context dir
    let cf_path = context_dir.join("Containerfile");
    tokio::fs::write(&cf_path, containerfile).await?;

    let mut child = Command::new(runtime)
        .args([
            "build",
            "-t",
            &image_tag,
            "-f",
            &cf_path.to_string_lossy(),
            &context_dir.to_string_lossy(),
        ])
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .kill_on_drop(true)
        .spawn()?;

    let stderr = child.stderr.take().expect("stderr piped");
    let tx_build = tx.clone();

    // Build output goes to stderr
    tokio::spawn(async move {
        let reader = BufReader::new(stderr);
        let mut lines = reader.lines();
        while let Ok(Some(line)) = lines.next_line().await {
            let _ = tx_build
                .send(LogLine {
                    stream: LogStream::Stderr,
                    text: line,
                    timestamp: chrono::Utc::now(),
                })
                .await;
        }
    });

    let stdout = child.stdout.take().expect("stdout piped");
    tokio::spawn(async move {
        let reader = BufReader::new(stdout);
        let mut lines = reader.lines();
        while let Ok(Some(line)) = lines.next_line().await {
            let _ = tx
                .send(LogLine {
                    stream: LogStream::Stdout,
                    text: line,
                    timestamp: chrono::Utc::now(),
                })
                .await;
        }
    });

    let status = child.wait().await?;
    if !status.success() {
        anyhow::bail!("{} build failed with exit code {}", runtime, status.code().unwrap_or(-1));
    }

    Ok(image_tag)
}

/// Run a container from a previously built image.
/// Returns the child process and a log receiver, matching the executor pattern.
pub async fn run_container(
    runtime: &str,
    image_tag: &str,
    container_name: &str,
    env_vars: &HashMap<String, String>,
) -> anyhow::Result<(Child, mpsc::Receiver<LogLine>)> {
    let mut args = vec![
        "run".to_string(),
        "--rm".to_string(),
        "--name".to_string(),
        container_name.to_string(),
    ];

    for (k, v) in env_vars {
        args.push("-e".to_string());
        args.push(format!("{k}={v}"));
    }

    args.push(image_tag.to_string());

    let mut child = Command::new(runtime)
        .args(&args)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .kill_on_drop(true)
        .spawn()?;

    let (tx, rx) = mpsc::channel::<LogLine>(256);

    let stdout = child.stdout.take().expect("stdout piped");
    let tx_out = tx.clone();
    tokio::spawn(async move {
        let reader = BufReader::new(stdout);
        let mut lines = reader.lines();
        while let Ok(Some(line)) = lines.next_line().await {
            let _ = tx_out
                .send(LogLine {
                    stream: LogStream::Stdout,
                    text: line,
                    timestamp: chrono::Utc::now(),
                })
                .await;
        }
    });

    let stderr = child.stderr.take().expect("stderr piped");
    tokio::spawn(async move {
        let reader = BufReader::new(stderr);
        let mut lines = reader.lines();
        while let Ok(Some(line)) = lines.next_line().await {
            let _ = tx
                .send(LogLine {
                    stream: LogStream::Stderr,
                    text: line,
                    timestamp: chrono::Utc::now(),
                })
                .await;
        }
    });

    Ok((child, rx))
}

/// Stop a running container by name.
pub async fn stop_container(runtime: &str, container_name: &str) -> anyhow::Result<()> {
    let _ = Command::new(runtime)
        .args(["stop", "-t", "5", container_name])
        .output()
        .await;

    let _ = Command::new(runtime)
        .args(["rm", "-f", container_name])
        .output()
        .await;

    Ok(())
}

/// List images for a project. Returns (tag, size_bytes, created).
pub async fn list_images(runtime: &str, project_id: &str) -> anyhow::Result<Vec<ImageInfo>> {
    let output = Command::new(runtime)
        .args([
            "images",
            &format!("berth/{project_id}"),
            "--format",
            "{{.Tag}} {{.Size}} {{.CreatedAt}}",
        ])
        .output()
        .await?;

    if !output.status.success() {
        return Ok(vec![]);
    }

    let stdout = String::from_utf8_lossy(&output.stdout);
    let images = stdout
        .lines()
        .filter(|l| !l.is_empty())
        .filter_map(|line| {
            let parts: Vec<&str> = line.splitn(3, ' ').collect();
            if parts.len() >= 2 {
                Some(ImageInfo {
                    tag: parts[0].to_string(),
                    size: parts.get(1).unwrap_or(&"").to_string(),
                    created: parts.get(2).unwrap_or(&"").to_string(),
                })
            } else {
                None
            }
        })
        .collect();

    Ok(images)
}

/// Remove old images, keeping the most recent `keep` versions.
pub async fn prune_images(runtime: &str, project_id: &str, keep: usize) -> anyhow::Result<u32> {
    let images = list_images(runtime, project_id).await?;
    if images.len() <= keep {
        return Ok(0);
    }

    // Sort by tag (version number) descending
    let mut versioned: Vec<(u32, &ImageInfo)> = images
        .iter()
        .filter_map(|img| img.tag.parse::<u32>().ok().map(|v| (v, img)))
        .collect();
    versioned.sort_by(|a, b| b.0.cmp(&a.0));

    let mut removed = 0u32;
    for (_, img) in versioned.iter().skip(keep) {
        let full_tag = format!("berth/{}:{}", project_id, img.tag);
        let output = Command::new(runtime)
            .args(["rmi", &full_tag])
            .output()
            .await;
        if output.is_ok() {
            removed += 1;
        }
    }

    Ok(removed)
}

#[derive(Debug, Clone)]
pub struct ImageInfo {
    pub tag: String,
    pub size: String,
    pub created: String,
}

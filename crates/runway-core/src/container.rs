use std::collections::HashMap;
use std::path::Path;
use std::process::Stdio;

use tokio::io::{AsyncBufReadExt, BufReader};
use tokio::process::{Child, Command};
use tokio::sync::mpsc;

use crate::executor::{LogLine, LogStream};

/// Check if Podman is installed and return its version.
pub async fn check_podman() -> anyhow::Result<String> {
    let output = Command::new("podman")
        .args(["version", "--format", "{{.Client.Version}}"])
        .output()
        .await
        .map_err(|_| {
            anyhow::anyhow!(
                "Podman is not installed. Install it with: \
                 apt-get install -y podman (Debian/Ubuntu) or \
                 dnf install -y podman (Fedora/RHEL)"
            )
        })?;

    if !output.status.success() {
        anyhow::bail!("Podman check failed: {}", String::from_utf8_lossy(&output.stderr));
    }

    Ok(String::from_utf8_lossy(&output.stdout).trim().to_string())
}

/// Build an OCI image from a Containerfile and context directory.
/// Streams build output through the sender channel.
/// Returns the image tag on success.
pub async fn build_image(
    project_id: &str,
    version: u32,
    containerfile: &str,
    context_dir: &Path,
    tx: mpsc::Sender<LogLine>,
) -> anyhow::Result<String> {
    let image_tag = format!("runway/{}:{}", project_id, version);

    // Write containerfile to context dir
    let cf_path = context_dir.join("Containerfile");
    tokio::fs::write(&cf_path, containerfile).await?;

    let mut child = Command::new("podman")
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

    // Podman build output goes to stderr
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
        anyhow::bail!("podman build failed with exit code {}", status.code().unwrap_or(-1));
    }

    Ok(image_tag)
}

/// Run a container from a previously built image.
/// Returns the child process and a log receiver, matching the executor pattern.
pub async fn run_container(
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

    let mut child = Command::new("podman")
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
pub async fn stop_container(container_name: &str) -> anyhow::Result<()> {
    let _ = Command::new("podman")
        .args(["stop", "-t", "5", container_name])
        .output()
        .await;

    let _ = Command::new("podman")
        .args(["rm", "-f", container_name])
        .output()
        .await;

    Ok(())
}

/// List images for a project. Returns (tag, size_bytes, created).
pub async fn list_images(project_id: &str) -> anyhow::Result<Vec<ImageInfo>> {
    let output = Command::new("podman")
        .args([
            "images",
            &format!("runway/{project_id}"),
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
pub async fn prune_images(project_id: &str, keep: usize) -> anyhow::Result<u32> {
    let images = list_images(project_id).await?;
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
        let full_tag = format!("runway/{}:{}", project_id, img.tag);
        let output = Command::new("podman")
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

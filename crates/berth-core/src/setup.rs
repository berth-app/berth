use std::path::Path;
use std::process::Stdio;

use tokio::io::{AsyncBufReadExt, BufReader};
use tokio::process::Command;
use tokio::sync::mpsc;

use crate::executor::{LogLine, LogStream};
use crate::runtime::RuntimeInfo;

/// Result of setting up a local environment.
pub struct SetupResult {
    /// Path to the venv python binary, if a venv was created.
    pub venv_python: Option<std::path::PathBuf>,
    /// The working directory for execution.
    pub working_dir: std::path::PathBuf,
}

/// Run setup commands in the project directory and stream output.
/// Used for bare-process execution (local agent without containers).
pub async fn run_setup_commands(
    working_dir: &Path,
    commands: &[String],
    tx: mpsc::Sender<LogLine>,
) -> anyhow::Result<()> {
    for cmd_str in commands {
        let _ = tx
            .send(LogLine {
                stream: LogStream::Stdout,
                text: format!("$ {cmd_str}"),
                timestamp: chrono::Utc::now(),
            })
            .await;

        let mut child = Command::new("sh")
            .args(["-c", cmd_str])
            .current_dir(working_dir)
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .kill_on_drop(true)
            .spawn()?;

        let stdout = child.stdout.take().expect("stdout piped");
        let stderr = child.stderr.take().expect("stderr piped");

        let tx_out = tx.clone();
        let out_task = tokio::spawn(async move {
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

        let tx_err = tx.clone();
        let err_task = tokio::spawn(async move {
            let reader = BufReader::new(stderr);
            let mut lines = reader.lines();
            while let Ok(Some(line)) = lines.next_line().await {
                let _ = tx_err
                    .send(LogLine {
                        stream: LogStream::Stderr,
                        text: line,
                        timestamp: chrono::Utc::now(),
                    })
                    .await;
            }
        });

        let _ = out_task.await;
        let _ = err_task.await;

        let status = child.wait().await?;
        if !status.success() {
            anyhow::bail!(
                "Setup command failed: '{}' (exit code {})",
                cmd_str,
                status.code().unwrap_or(-1)
            );
        }
    }
    Ok(())
}

/// Compute a hash of dependency lock files for caching.
/// Returns a hex-encoded SHA-256 hash, or empty string if no lock files exist.
pub fn compute_deps_hash(project_path: &Path) -> String {
    use std::io::Read;

    let lock_files = [
        "requirements.txt",
        "package-lock.json",
        "yarn.lock",
        "pnpm-lock.yaml",
        "go.sum",
        "Cargo.lock",
    ];

    let mut hasher = simple_hash::SimpleHasher::new();
    let mut found_any = false;

    for name in &lock_files {
        let path = project_path.join(name);
        if let Ok(mut f) = std::fs::File::open(&path) {
            let mut buf = Vec::new();
            if f.read_to_end(&mut buf).is_ok() {
                hasher.update(&buf);
                found_any = true;
            }
        }
    }

    if !found_any {
        return String::new();
    }

    hasher.finish_hex()
}

/// Check if the project's setup is cached (deps haven't changed).
pub fn is_setup_cached(project_id: &str, current_hash: &str) -> bool {
    if current_hash.is_empty() {
        return true; // No deps = always "cached"
    }

    let cache_dir = dirs_next::home_dir()
        .unwrap_or_default()
        .join(".berth/setup-cache");
    let cache_file = cache_dir.join(format!("{project_id}.hash"));

    match std::fs::read_to_string(&cache_file) {
        Ok(stored) => stored.trim() == current_hash,
        Err(_) => false,
    }
}

/// Store the deps hash after successful setup.
pub fn store_setup_hash(project_id: &str, hash: &str) -> anyhow::Result<()> {
    let cache_dir = dirs_next::home_dir()
        .unwrap_or_default()
        .join(".berth/setup-cache");
    std::fs::create_dir_all(&cache_dir)?;
    let cache_file = cache_dir.join(format!("{project_id}.hash"));
    std::fs::write(cache_file, hash)?;
    Ok(())
}

/// Determine the python binary path, preferring venv if it exists.
pub fn resolve_python_path(working_dir: &Path) -> Option<std::path::PathBuf> {
    let venv_python = working_dir.join(".venv/bin/python3");
    if venv_python.exists() {
        Some(venv_python)
    } else {
        let venv_python = working_dir.join(".venv/bin/python");
        if venv_python.exists() {
            Some(venv_python)
        } else {
            None
        }
    }
}

/// Generate setup commands from RuntimeInfo (convenience wrapper).
pub fn generate_commands(info: &RuntimeInfo) -> Vec<String> {
    crate::containerfile::setup_commands(info)
}

/// Simple hasher using basic byte accumulation.
/// We avoid pulling in sha2 crate by using a simple FNV-like hash for cache keys.
/// This doesn't need to be cryptographic — just detect changes.
mod simple_hash {
    pub struct SimpleHasher {
        state: u64,
        len: u64,
    }

    impl SimpleHasher {
        pub fn new() -> Self {
            Self {
                state: 0xcbf29ce484222325, // FNV offset basis
                len: 0,
            }
        }

        pub fn update(&mut self, data: &[u8]) {
            for &byte in data {
                self.state ^= byte as u64;
                self.state = self.state.wrapping_mul(0x100000001b3); // FNV prime
            }
            self.len += data.len() as u64;
        }

        pub fn finish_hex(&self) -> String {
            format!("{:016x}{:016x}", self.state, self.len)
        }
    }
}

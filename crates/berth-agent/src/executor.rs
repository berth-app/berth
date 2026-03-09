use std::collections::HashMap;
use std::path::Path;
use std::process::Stdio;
use tokio::io::{AsyncBufReadExt, BufReader};
use tokio::process::{Child, Command};
use tokio::sync::mpsc;

pub use berth_proto::executor::{LogLine, LogStream};

use berth_proto::runtime::Runtime;

/// Build the command to execute a project based on its runtime.
fn build_command(
    runtime: Runtime,
    entrypoint: &str,
    working_dir: &str,
    env_vars: Option<&HashMap<String, String>>,
) -> Command {
    let mut cmd = match runtime {
        Runtime::Python => {
            // Prefer venv python if it exists
            let venv_python = Path::new(working_dir).join(".venv/bin/python3");
            if venv_python.exists() {
                let mut c = Command::new(venv_python);
                c.args(["-u", entrypoint]);
                c
            } else {
                let mut c = Command::new("python3");
                c.args(["-u", entrypoint]);
                c
            }
        }
        Runtime::Node => {
            let mut c = Command::new("node");
            c.arg(entrypoint);
            c
        }
        Runtime::Go => {
            let mut c = Command::new("go");
            c.args(["run", entrypoint]);
            c
        }
        Runtime::Rust => {
            let mut c = Command::new("cargo");
            c.args(["run"]);
            c
        }
        Runtime::Shell | Runtime::Unknown => {
            let mut c = Command::new("sh");
            c.arg(entrypoint);
            c
        }
    };

    cmd.current_dir(working_dir)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .kill_on_drop(true);

    if let Some(vars) = env_vars {
        cmd.envs(vars);
    }

    cmd
}

/// Spawn a process and stream its output via a channel.
pub async fn spawn_and_stream(
    runtime: Runtime,
    entrypoint: &str,
    working_dir: &str,
    env_vars: Option<&HashMap<String, String>>,
) -> anyhow::Result<(Child, mpsc::Receiver<LogLine>)> {
    let mut child = build_command(runtime, entrypoint, working_dir, env_vars).spawn()?;

    let (tx, rx) = mpsc::channel::<LogLine>(256);

    let stdout = child.stdout.take().expect("stdout was piped");
    let stderr = child.stderr.take().expect("stderr was piped");

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

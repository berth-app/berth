use std::process::Stdio;
use tokio::io::{AsyncBufReadExt, BufReader};
use tokio::process::{Child, Command};
use tokio::sync::mpsc;

use crate::runtime::Runtime;

#[derive(Debug, Clone)]
pub struct LogLine {
    pub stream: LogStream,
    pub text: String,
    pub timestamp: chrono::DateTime<chrono::Utc>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LogStream {
    Stdout,
    Stderr,
}

impl serde::Serialize for LogStream {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        match self {
            LogStream::Stdout => serializer.serialize_str("stdout"),
            LogStream::Stderr => serializer.serialize_str("stderr"),
        }
    }
}

impl serde::Serialize for LogLine {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        use serde::ser::SerializeStruct;
        let mut s = serializer.serialize_struct("LogLine", 3)?;
        s.serialize_field("stream", &self.stream)?;
        s.serialize_field("text", &self.text)?;
        s.serialize_field("timestamp", &self.timestamp.to_rfc3339())?;
        s.end()
    }
}

/// Build the command to execute a project based on its runtime.
fn build_command(runtime: Runtime, entrypoint: &str, working_dir: &str) -> Command {
    let mut cmd = match runtime {
        Runtime::Python => {
            let mut c = Command::new("python3");
            c.arg(entrypoint);
            c
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

    cmd
}

/// Spawn a process and stream its output via a channel.
pub async fn spawn_and_stream(
    runtime: Runtime,
    entrypoint: &str,
    working_dir: &str,
) -> anyhow::Result<(Child, mpsc::Receiver<LogLine>)> {
    let mut child = build_command(runtime, entrypoint, working_dir).spawn()?;

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

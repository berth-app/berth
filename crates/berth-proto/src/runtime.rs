use std::collections::HashMap;

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Runtime {
    Python,
    Node,
    Go,
    Rust,
    Shell,
    Unknown,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RuntimeInfo {
    pub runtime: Runtime,
    pub version_file: Option<String>,
    pub entrypoint: Option<String>,
    pub confidence: f32,
    #[serde(default)]
    pub dependencies: Vec<String>,
    #[serde(default)]
    pub scripts: HashMap<String, String>,
}

/// Parse a runtime string (from proto/NATS) into a Runtime enum.
pub fn parse_runtime(s: &str) -> Runtime {
    match s {
        "python" => Runtime::Python,
        "node" => Runtime::Node,
        "go" => Runtime::Go,
        "rust" => Runtime::Rust,
        "shell" => Runtime::Shell,
        _ => Runtime::Unknown,
    }
}

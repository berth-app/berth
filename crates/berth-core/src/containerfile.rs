use std::path::Path;

use crate::runtime::{Runtime, RuntimeInfo};

/// Generate a Containerfile (Dockerfile) from runtime detection info.
pub fn generate(info: &RuntimeInfo) -> String {
    let entrypoint = info.entrypoint.as_deref().unwrap_or("main.py");

    match info.runtime {
        Runtime::Python => generate_python(entrypoint, info),
        Runtime::Node => generate_node(entrypoint, info),
        Runtime::Go => generate_go(entrypoint),
        Runtime::Rust => generate_rust(),
        Runtime::Shell => generate_shell(entrypoint),
        Runtime::Unknown => generate_shell(entrypoint),
    }
}

/// Check if the project has a user-provided Containerfile or Dockerfile.
pub fn has_custom(project_path: &Path) -> Option<std::path::PathBuf> {
    for name in &["Containerfile", "Dockerfile"] {
        let p = project_path.join(name);
        if p.exists() {
            return Some(p);
        }
    }
    None
}

/// Return the user's custom Containerfile if present, otherwise auto-generate.
pub fn get(project_path: &Path, info: &RuntimeInfo) -> String {
    if let Some(custom_path) = has_custom(project_path) {
        if let Ok(content) = std::fs::read_to_string(&custom_path) {
            return content;
        }
    }
    generate(info)
}

/// Generate setup commands for bare-process execution (no containers).
/// These commands are run in the project directory to install dependencies.
pub fn setup_commands(info: &RuntimeInfo) -> Vec<String> {
    let has_deps = !info.dependencies.is_empty();

    match info.runtime {
        Runtime::Python if has_deps => {
            vec![
                "python3 -m venv .venv".to_string(),
                ".venv/bin/pip install -r requirements.txt".to_string(),
            ]
        }
        Runtime::Node if has_deps => {
            vec!["npm install --production".to_string()]
        }
        Runtime::Go if has_deps => {
            vec!["go mod download".to_string()]
        }
        Runtime::Rust if has_deps => {
            vec!["cargo build --release".to_string()]
        }
        _ => vec![],
    }
}

fn generate_python(entrypoint: &str, info: &RuntimeInfo) -> String {
    let has_requirements = !info.dependencies.is_empty()
        || info.version_file.as_deref() == Some("requirements.txt")
        || info.version_file.as_deref() == Some("pyproject.toml");

    let mut lines = vec![
        "FROM python:3.12-slim".to_string(),
        "WORKDIR /app".to_string(),
    ];

    if has_requirements {
        lines.push("COPY requirements.txt* pyproject.toml* ./".to_string());
        lines.push(
            "RUN [ -f requirements.txt ] && pip install --no-cache-dir -r requirements.txt || true"
                .to_string(),
        );
    }

    lines.push("COPY . .".to_string());
    lines.push(format!("CMD [\"python\", \"-u\", \"{entrypoint}\"]"));

    lines.join("\n")
}

fn generate_node(entrypoint: &str, info: &RuntimeInfo) -> String {
    let has_package = !info.dependencies.is_empty()
        || info.version_file.as_deref() == Some("package.json");

    let mut lines = vec![
        "FROM node:20-slim".to_string(),
        "WORKDIR /app".to_string(),
    ];

    if has_package {
        lines.push("COPY package*.json ./".to_string());
        lines.push(
            "RUN [ -f package.json ] && npm install --production || true".to_string(),
        );
    }

    lines.push("COPY . .".to_string());
    lines.push(format!("CMD [\"node\", \"{entrypoint}\"]"));

    lines.join("\n")
}

fn generate_go(entrypoint: &str) -> String {
    [
        "FROM golang:1.22",
        "WORKDIR /app",
        "COPY go.mod go.sum* ./",
        "RUN [ -f go.mod ] && go mod download || true",
        "COPY . .",
        &format!("CMD [\"go\", \"run\", \"{entrypoint}\"]"),
    ]
    .join("\n")
}

fn generate_rust() -> String {
    [
        "FROM rust:1.77 AS builder",
        "WORKDIR /app",
        "COPY . .",
        "RUN cargo build --release",
        "",
        "FROM debian:bookworm-slim",
        "RUN apt-get update && apt-get install -y --no-install-recommends ca-certificates && rm -rf /var/lib/apt/lists/*",
        "COPY --from=builder /app/target/release/* /usr/local/bin/",
        "CMD [\"/usr/local/bin/app\"]",
    ]
    .join("\n")
}

fn generate_shell(entrypoint: &str) -> String {
    [
        "FROM alpine:3.19",
        "RUN apk add --no-cache bash curl",
        "WORKDIR /app",
        "COPY . .",
        &format!("RUN chmod +x {entrypoint}"),
        &format!("CMD [\"sh\", \"{entrypoint}\"]"),
    ]
    .join("\n")
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;

    fn make_info(runtime: Runtime, entrypoint: &str, deps: Vec<&str>) -> RuntimeInfo {
        RuntimeInfo {
            runtime,
            version_file: None,
            entrypoint: Some(entrypoint.to_string()),
            confidence: 0.9,
            dependencies: deps.into_iter().map(String::from).collect(),
            scripts: HashMap::new(),
        }
    }

    #[test]
    fn test_python_containerfile() {
        let info = make_info(Runtime::Python, "main.py", vec!["requests", "flask"]);
        let cf = generate(&info);
        assert!(cf.contains("FROM python:3.12-slim"));
        assert!(cf.contains("pip install"));
        assert!(cf.contains("CMD [\"python\", \"-u\", \"main.py\"]"));
    }

    #[test]
    fn test_node_containerfile() {
        let info = make_info(Runtime::Node, "index.js", vec!["express"]);
        let cf = generate(&info);
        assert!(cf.contains("FROM node:20-slim"));
        assert!(cf.contains("npm install"));
        assert!(cf.contains("CMD [\"node\", \"index.js\"]"));
    }

    #[test]
    fn test_go_containerfile() {
        let info = make_info(Runtime::Go, "main.go", vec![]);
        let cf = generate(&info);
        assert!(cf.contains("FROM golang:1.22"));
        assert!(cf.contains("CMD [\"go\", \"run\", \"main.go\"]"));
    }

    #[test]
    fn test_shell_containerfile() {
        let info = make_info(Runtime::Shell, "run.sh", vec![]);
        let cf = generate(&info);
        assert!(cf.contains("FROM alpine:3.19"));
        assert!(cf.contains("CMD [\"sh\", \"run.sh\"]"));
    }

    #[test]
    fn test_rust_containerfile() {
        let info = make_info(Runtime::Rust, "src/main.rs", vec!["serde"]);
        let cf = generate(&info);
        assert!(cf.contains("FROM rust:1.77 AS builder"));
        assert!(cf.contains("cargo build --release"));
    }

    #[test]
    fn test_custom_containerfile() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("Containerfile"), "FROM custom:latest").unwrap();
        let info = make_info(Runtime::Python, "main.py", vec![]);
        let cf = get(dir.path(), &info);
        assert_eq!(cf, "FROM custom:latest");
    }

    #[test]
    fn test_setup_commands_python() {
        let info = make_info(Runtime::Python, "main.py", vec!["requests"]);
        let cmds = setup_commands(&info);
        assert_eq!(cmds.len(), 2);
        assert!(cmds[0].contains("venv"));
        assert!(cmds[1].contains("pip install"));
    }

    #[test]
    fn test_setup_commands_no_deps() {
        let info = make_info(Runtime::Python, "main.py", vec![]);
        let cmds = setup_commands(&info);
        assert!(cmds.is_empty());
    }
}

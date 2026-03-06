use serde::{Deserialize, Serialize};
use std::path::Path;

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
}

/// Detect the runtime of a project by scanning its directory for marker files.
pub fn detect_runtime(path: &Path) -> RuntimeInfo {
    if path.is_file() {
        return detect_from_file(path);
    }

    // Check for language-specific marker files in priority order
    let checks: &[(&str, Runtime, &str)] = &[
        ("requirements.txt", Runtime::Python, "main.py"),
        ("pyproject.toml", Runtime::Python, "main.py"),
        ("setup.py", Runtime::Python, "main.py"),
        ("package.json", Runtime::Node, "index.js"),
        ("go.mod", Runtime::Go, "main.go"),
        ("Cargo.toml", Runtime::Rust, "src/main.rs"),
    ];

    for (marker, runtime, default_entry) in checks {
        if path.join(marker).exists() {
            let entrypoint = find_entrypoint(path, *runtime).unwrap_or(default_entry.to_string());
            return RuntimeInfo {
                runtime: *runtime,
                version_file: Some(marker.to_string()),
                entrypoint: Some(entrypoint),
                confidence: 0.9,
            };
        }
    }

    // Check for entrypoint files directly (no marker file present)
    let direct_checks: &[(&str, Runtime)] = &[
        ("main.py", Runtime::Python),
        ("app.py", Runtime::Python),
        ("run.py", Runtime::Python),
        ("index.js", Runtime::Node),
        ("index.ts", Runtime::Node),
        ("main.js", Runtime::Node),
        ("app.js", Runtime::Node),
        ("main.go", Runtime::Go),
        ("run.sh", Runtime::Shell),
        ("start.sh", Runtime::Shell),
        ("main.sh", Runtime::Shell),
    ];

    for (file, runtime) in direct_checks {
        if path.join(file).exists() {
            return RuntimeInfo {
                runtime: *runtime,
                version_file: None,
                entrypoint: Some(file.to_string()),
                confidence: 0.7,
            };
        }
    }

    // Check for files by extension as last resort
    for ext in &["sh", "bash", "zsh"] {
        if has_files_with_extension(path, ext) {
            return RuntimeInfo {
                runtime: Runtime::Shell,
                version_file: None,
                entrypoint: find_first_with_extension(path, ext),
                confidence: 0.5,
            };
        }
    }

    for (ext, runtime) in &[("py", Runtime::Python), ("js", Runtime::Node), ("go", Runtime::Go)] {
        if has_files_with_extension(path, ext) {
            return RuntimeInfo {
                runtime: *runtime,
                version_file: None,
                entrypoint: find_first_with_extension(path, ext),
                confidence: 0.5,
            };
        }
    }

    RuntimeInfo {
        runtime: Runtime::Unknown,
        version_file: None,
        entrypoint: None,
        confidence: 0.0,
    }
}

fn detect_from_file(path: &Path) -> RuntimeInfo {
    let ext = path.extension().and_then(|e| e.to_str()).unwrap_or("");
    let (runtime, confidence) = match ext {
        "py" => (Runtime::Python, 0.95),
        "js" | "ts" | "mjs" => (Runtime::Node, 0.95),
        "go" => (Runtime::Go, 0.95),
        "rs" => (Runtime::Rust, 0.95),
        "sh" | "bash" | "zsh" => (Runtime::Shell, 0.95),
        _ => (Runtime::Unknown, 0.0),
    };

    RuntimeInfo {
        runtime,
        version_file: None,
        entrypoint: Some(path.file_name().unwrap_or_default().to_string_lossy().into()),
        confidence,
    }
}

fn find_entrypoint(path: &Path, runtime: Runtime) -> Option<String> {
    let candidates: &[&str] = match runtime {
        Runtime::Python => &["main.py", "app.py", "run.py", "__main__.py"],
        Runtime::Node => &["index.js", "index.ts", "main.js", "main.ts", "app.js", "app.ts"],
        Runtime::Go => &["main.go", "cmd/main.go"],
        Runtime::Rust => &["src/main.rs"],
        Runtime::Shell => &["run.sh", "start.sh", "main.sh"],
        Runtime::Unknown => &[],
    };

    candidates
        .iter()
        .find(|c| path.join(c).exists())
        .map(|c| c.to_string())
}

fn find_first_with_extension(path: &Path, ext: &str) -> Option<String> {
    path.read_dir()
        .ok()?
        .filter_map(|e| e.ok())
        .find(|e| {
            e.path()
                .extension()
                .and_then(|e| e.to_str())
                .is_some_and(|e2| e2 == ext)
        })
        .map(|e| e.file_name().to_string_lossy().into())
}

fn has_files_with_extension(path: &Path, ext: &str) -> bool {
    path.read_dir()
        .map(|entries| {
            entries
                .filter_map(|e| e.ok())
                .any(|e| {
                    e.path()
                        .extension()
                        .and_then(|e| e.to_str())
                        .is_some_and(|e| e == ext)
                })
        })
        .unwrap_or(false)
}

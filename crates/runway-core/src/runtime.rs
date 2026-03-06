use serde::{Deserialize, Serialize};
use std::collections::HashMap;
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
    #[serde(default)]
    pub dependencies: Vec<String>,
    #[serde(default)]
    pub scripts: HashMap<String, String>,
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
            let (deps, scripts) = parse_deps(path, marker, *runtime);
            return RuntimeInfo {
                runtime: *runtime,
                version_file: Some(marker.to_string()),
                entrypoint: Some(entrypoint),
                confidence: 0.9,
                dependencies: deps,
                scripts,
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
                dependencies: vec![],
                scripts: HashMap::new(),
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
                dependencies: vec![],
                scripts: HashMap::new(),
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
                dependencies: vec![],
                scripts: HashMap::new(),
            };
        }
    }

    RuntimeInfo {
        runtime: Runtime::Unknown,
        version_file: None,
        entrypoint: None,
        confidence: 0.0,
        dependencies: vec![],
        scripts: HashMap::new(),
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
        dependencies: vec![],
        scripts: HashMap::new(),
    }
}

/// Parse dependencies and scripts from marker files.
fn parse_deps(path: &Path, marker: &str, runtime: Runtime) -> (Vec<String>, HashMap<String, String>) {
    match (marker, runtime) {
        ("requirements.txt", Runtime::Python) => {
            let deps = parse_requirements_txt(path);
            (deps, HashMap::new())
        }
        ("pyproject.toml", Runtime::Python) => {
            let deps = parse_pyproject_toml(path);
            (deps, HashMap::new())
        }
        ("package.json", Runtime::Node) => parse_package_json(path),
        ("go.mod", Runtime::Go) => {
            let deps = parse_go_mod(path);
            (deps, HashMap::new())
        }
        ("Cargo.toml", Runtime::Rust) => {
            let deps = parse_cargo_toml(path);
            (deps, HashMap::new())
        }
        _ => (vec![], HashMap::new()),
    }
}

/// Parse requirements.txt: one package per line, ignore comments and options.
fn parse_requirements_txt(dir: &Path) -> Vec<String> {
    let content = match std::fs::read_to_string(dir.join("requirements.txt")) {
        Ok(c) => c,
        Err(_) => return vec![],
    };

    content
        .lines()
        .map(|l| l.trim())
        .filter(|l| !l.is_empty() && !l.starts_with('#') && !l.starts_with('-'))
        .map(|l| {
            // Strip version specifiers: "requests>=2.28" -> "requests"
            l.split(&['=', '>', '<', '!', '~', ';', '['][..])
                .next()
                .unwrap_or(l)
                .trim()
                .to_string()
        })
        .collect()
}

/// Parse pyproject.toml: extract [project].dependencies list.
fn parse_pyproject_toml(dir: &Path) -> Vec<String> {
    let content = match std::fs::read_to_string(dir.join("pyproject.toml")) {
        Ok(c) => c,
        Err(_) => return vec![],
    };

    // Simple line-based extraction from dependencies array
    let mut in_deps = false;
    let mut deps = vec![];

    for line in content.lines() {
        let trimmed = line.trim();
        if trimmed == "dependencies = [" || trimmed.starts_with("dependencies = [") {
            in_deps = true;
            // Check for inline: dependencies = ["foo", "bar"]
            if let Some(rest) = trimmed.strip_prefix("dependencies = [") {
                for item in rest.trim_end_matches(']').split(',') {
                    let dep = item.trim().trim_matches('"').trim_matches('\'');
                    if !dep.is_empty() {
                        let name = dep
                            .split(&['=', '>', '<', '!', '~', ';', '['][..])
                            .next()
                            .unwrap_or(dep)
                            .trim();
                        if !name.is_empty() {
                            deps.push(name.to_string());
                        }
                    }
                }
                if rest.contains(']') {
                    in_deps = false;
                }
            }
            continue;
        }
        if in_deps {
            if trimmed == "]" {
                in_deps = false;
                continue;
            }
            let dep = trimmed.trim_matches(',').trim_matches('"').trim_matches('\'');
            if !dep.is_empty() {
                let name = dep
                    .split(&['=', '>', '<', '!', '~', ';', '['][..])
                    .next()
                    .unwrap_or(dep)
                    .trim();
                if !name.is_empty() {
                    deps.push(name.to_string());
                }
            }
        }
    }

    deps
}

/// Parse package.json: extract dependencies keys and scripts.
fn parse_package_json(dir: &Path) -> (Vec<String>, HashMap<String, String>) {
    let content = match std::fs::read_to_string(dir.join("package.json")) {
        Ok(c) => c,
        Err(_) => return (vec![], HashMap::new()),
    };

    let json: serde_json::Value = match serde_json::from_str(&content) {
        Ok(v) => v,
        Err(_) => return (vec![], HashMap::new()),
    };

    let mut deps = vec![];
    if let Some(obj) = json.get("dependencies").and_then(|v| v.as_object()) {
        deps.extend(obj.keys().cloned());
    }
    if let Some(obj) = json.get("devDependencies").and_then(|v| v.as_object()) {
        deps.extend(obj.keys().cloned());
    }

    let mut scripts = HashMap::new();
    if let Some(obj) = json.get("scripts").and_then(|v| v.as_object()) {
        for (k, v) in obj {
            if let Some(s) = v.as_str() {
                scripts.insert(k.clone(), s.to_string());
            }
        }
    }

    (deps, scripts)
}

/// Parse go.mod: extract require directives.
fn parse_go_mod(dir: &Path) -> Vec<String> {
    let content = match std::fs::read_to_string(dir.join("go.mod")) {
        Ok(c) => c,
        Err(_) => return vec![],
    };

    let mut deps = vec![];
    let mut in_require = false;

    for line in content.lines() {
        let trimmed = line.trim();
        if trimmed == "require (" {
            in_require = true;
            continue;
        }
        if trimmed == ")" {
            in_require = false;
            continue;
        }
        if in_require {
            // "github.com/foo/bar v1.2.3"
            if let Some(module) = trimmed.split_whitespace().next() {
                if !module.starts_with("//") {
                    deps.push(module.to_string());
                }
            }
        }
        // Single-line require
        if let Some(rest) = trimmed.strip_prefix("require ") {
            if !rest.starts_with('(') {
                if let Some(module) = rest.split_whitespace().next() {
                    deps.push(module.to_string());
                }
            }
        }
    }

    deps
}

/// Parse Cargo.toml: extract [dependencies] keys.
fn parse_cargo_toml(dir: &Path) -> Vec<String> {
    let content = match std::fs::read_to_string(dir.join("Cargo.toml")) {
        Ok(c) => c,
        Err(_) => return vec![],
    };

    let mut deps = vec![];
    let mut in_deps = false;

    for line in content.lines() {
        let trimmed = line.trim();
        if trimmed == "[dependencies]" {
            in_deps = true;
            continue;
        }
        if trimmed.starts_with('[') {
            in_deps = false;
            continue;
        }
        if in_deps && !trimmed.is_empty() && !trimmed.starts_with('#') {
            if let Some(name) = trimmed.split('=').next() {
                let name = name.trim();
                if !name.is_empty() {
                    deps.push(name.to_string());
                }
            }
        }
    }

    deps
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

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    #[test]
    fn test_parse_requirements_txt() {
        let dir = tempfile::tempdir().unwrap();
        fs::write(
            dir.path().join("requirements.txt"),
            "requests>=2.28\nflask\n# comment\nnumpy==1.24\n",
        )
        .unwrap();
        let deps = parse_requirements_txt(dir.path());
        assert_eq!(deps, vec!["requests", "flask", "numpy"]);
    }

    #[test]
    fn test_parse_package_json() {
        let dir = tempfile::tempdir().unwrap();
        fs::write(
            dir.path().join("package.json"),
            r#"{"dependencies":{"express":"^4.18"},"devDependencies":{"jest":"^29"},"scripts":{"start":"node index.js","test":"jest"}}"#,
        )
        .unwrap();
        let (deps, scripts) = parse_package_json(dir.path());
        assert!(deps.contains(&"express".to_string()));
        assert!(deps.contains(&"jest".to_string()));
        assert_eq!(scripts.get("start").unwrap(), "node index.js");
    }

    #[test]
    fn test_parse_go_mod() {
        let dir = tempfile::tempdir().unwrap();
        fs::write(
            dir.path().join("go.mod"),
            "module example.com/foo\n\ngo 1.21\n\nrequire (\n\tgithub.com/gin-gonic/gin v1.9.1\n\tgithub.com/lib/pq v1.10.9\n)\n",
        )
        .unwrap();
        let deps = parse_go_mod(dir.path());
        assert_eq!(deps, vec!["github.com/gin-gonic/gin", "github.com/lib/pq"]);
    }

    #[test]
    fn test_parse_cargo_toml() {
        let dir = tempfile::tempdir().unwrap();
        fs::write(
            dir.path().join("Cargo.toml"),
            "[package]\nname = \"foo\"\n\n[dependencies]\nserde = \"1\"\ntokio = { version = \"1\" }\n\n[dev-dependencies]\ntempfile = \"3\"\n",
        )
        .unwrap();
        let deps = parse_cargo_toml(dir.path());
        assert_eq!(deps, vec!["serde", "tokio"]);
    }
}

use std::io::Write;
use std::path::Path;

use flate2::read::GzDecoder;
use flate2::write::GzEncoder;
use flate2::Compression;

const MAX_ARCHIVE_SIZE: u64 = 64 * 1024 * 1024; // 64MB

const SKIP_DIRS: &[&str] = &[
    ".git",
    "node_modules",
    ".venv",
    "venv",
    "__pycache__",
    "target",
    ".berth",
    ".tox",
    "dist",
    "build",
];

const SKIP_EXTENSIONS: &[&str] = &["pyc", "pyo", "o", "so", "dylib"];

/// Create a gzipped tarball of a project directory, skipping common build artifacts.
pub fn create(project_path: &Path) -> anyhow::Result<Vec<u8>> {
    let buf = Vec::new();
    let encoder = GzEncoder::new(buf, Compression::fast());
    let mut archive = tar::Builder::new(encoder);

    walk_and_add(project_path, project_path, &mut archive)?;

    let encoder = archive.into_inner()?;
    let compressed = encoder.finish()?;

    if compressed.len() as u64 > MAX_ARCHIVE_SIZE {
        anyhow::bail!(
            "Source archive is {}MB, exceeds the 64MB limit. \
             Ensure node_modules/, .venv/, and build artifacts are excluded.",
            compressed.len() / (1024 * 1024)
        );
    }

    Ok(compressed)
}

/// Extract a gzipped tarball to a destination directory.
pub fn extract(archive_bytes: &[u8], dest: &Path) -> anyhow::Result<()> {
    let decoder = GzDecoder::new(archive_bytes);
    let mut archive = tar::Archive::new(decoder);
    archive.unpack(dest)?;
    Ok(())
}

fn walk_and_add<W: Write>(
    root: &Path,
    current: &Path,
    archive: &mut tar::Builder<W>,
) -> anyhow::Result<()> {
    let entries = match std::fs::read_dir(current) {
        Ok(e) => e,
        Err(_) => return Ok(()),
    };

    for entry in entries {
        let entry = entry?;
        let path = entry.path();
        let file_name = entry.file_name();
        let name_str = file_name.to_string_lossy();

        // Skip hidden files (except .env)
        if name_str.starts_with('.') && name_str != ".env" {
            if let Some(dir_name) = name_str.strip_prefix('.') {
                if SKIP_DIRS.contains(&dir_name) || !path.is_dir() {
                    continue;
                }
            } else {
                continue;
            }
        }

        // Skip known artifact directories
        if path.is_dir() && SKIP_DIRS.contains(&name_str.as_ref()) {
            continue;
        }

        // Skip known artifact extensions
        if path.is_file() {
            if let Some(ext) = path.extension().and_then(|e| e.to_str()) {
                if SKIP_EXTENSIONS.contains(&ext) {
                    continue;
                }
            }
        }

        let rel_path = path.strip_prefix(root)?;

        if path.is_dir() {
            walk_and_add(root, &path, archive)?;
        } else if path.is_file() {
            archive.append_path_with_name(&path, rel_path)?;
        }
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_create_and_extract_roundtrip() {
        let src = tempfile::tempdir().unwrap();
        std::fs::write(src.path().join("main.py"), "print('hello')").unwrap();
        std::fs::write(src.path().join("requirements.txt"), "requests\n").unwrap();

        // Create a subdirectory with a file
        std::fs::create_dir(src.path().join("lib")).unwrap();
        std::fs::write(src.path().join("lib/utils.py"), "def helper(): pass").unwrap();

        let archive = create(src.path()).unwrap();
        assert!(!archive.is_empty());

        let dest = tempfile::tempdir().unwrap();
        extract(&archive, dest.path()).unwrap();

        assert!(dest.path().join("main.py").exists());
        assert!(dest.path().join("requirements.txt").exists());
        assert!(dest.path().join("lib/utils.py").exists());
    }

    #[test]
    fn test_skips_git_and_node_modules() {
        let src = tempfile::tempdir().unwrap();
        std::fs::write(src.path().join("index.js"), "console.log('hi')").unwrap();

        std::fs::create_dir(src.path().join(".git")).unwrap();
        std::fs::write(src.path().join(".git/HEAD"), "ref: refs/heads/main").unwrap();

        std::fs::create_dir(src.path().join("node_modules")).unwrap();
        std::fs::write(src.path().join("node_modules/pkg.js"), "module").unwrap();

        let archive = create(src.path()).unwrap();
        let dest = tempfile::tempdir().unwrap();
        extract(&archive, dest.path()).unwrap();

        assert!(dest.path().join("index.js").exists());
        assert!(!dest.path().join(".git").exists());
        assert!(!dest.path().join("node_modules").exists());
    }

    #[test]
    fn test_preserves_env_file() {
        let src = tempfile::tempdir().unwrap();
        std::fs::write(src.path().join("main.py"), "import os").unwrap();
        std::fs::write(src.path().join(".env"), "SECRET=abc").unwrap();

        let archive = create(src.path()).unwrap();
        let dest = tempfile::tempdir().unwrap();
        extract(&archive, dest.path()).unwrap();

        assert!(dest.path().join(".env").exists());
    }
}

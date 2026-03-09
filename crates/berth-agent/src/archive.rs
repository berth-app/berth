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
///
/// Validates each entry's path to prevent directory traversal attacks
/// (e.g., entries containing `../` or absolute paths).
pub fn extract(archive_bytes: &[u8], dest: &Path) -> anyhow::Result<()> {
    let decoder = GzDecoder::new(archive_bytes);
    let mut archive = tar::Archive::new(decoder);

    // Validate every entry path before extraction
    for entry in archive.entries()? {
        let mut entry = entry?;
        let path = entry.path()?;

        // Reject absolute paths
        if path.is_absolute() {
            anyhow::bail!("Archive contains absolute path: {}", path.display());
        }

        // Reject path traversal components
        for component in path.components() {
            if let std::path::Component::ParentDir = component {
                anyhow::bail!(
                    "Archive contains path traversal: {}",
                    path.display()
                );
            }
        }

        // Reject symlinks pointing outside dest
        if entry.header().entry_type().is_symlink() || entry.header().entry_type().is_hard_link() {
            if let Ok(link_target) = entry.link_name() {
                if let Some(target) = link_target {
                    if target.is_absolute() || target.components().any(|c| matches!(c, std::path::Component::ParentDir)) {
                        anyhow::bail!(
                            "Archive contains suspicious link: {} -> {}",
                            path.display(),
                            target.display()
                        );
                    }
                }
            }
        }

        entry.unpack_in(dest)?;
    }
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

    #[test]
    fn test_rejects_path_traversal_in_archive() {
        use flate2::write::GzEncoder;
        use flate2::Compression;

        // Craft a malicious tarball by using append_data with a traversal path
        let buf = Vec::new();
        let encoder = GzEncoder::new(buf, Compression::fast());
        let mut builder = tar::Builder::new(encoder);

        let data = b"malicious content";
        let mut header = tar::Header::new_gnu();
        header.set_size(data.len() as u64);
        header.set_mode(0o644);
        header.set_entry_type(tar::EntryType::Regular);
        // Write path bytes directly to bypass set_path validation
        {
            let path_bytes = b"../../etc/malicious";
            let raw = header.as_mut_bytes();
            raw[..path_bytes.len()].copy_from_slice(path_bytes);
        }
        header.set_cksum();
        builder.append(&header, &data[..]).unwrap();

        let encoder = builder.into_inner().unwrap();
        let compressed = encoder.finish().unwrap();

        let dest = tempfile::tempdir().unwrap();
        let result = extract(&compressed, dest.path());
        assert!(result.is_err(), "Should reject archives with path traversal entries");
        let err_msg = result.unwrap_err().to_string();
        assert!(
            err_msg.contains("path traversal") || err_msg.contains(".."),
            "Error should mention path traversal, got: {err_msg}"
        );
    }
}

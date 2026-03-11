//! Path sanitization utilities to prevent directory traversal attacks.

use std::path::{Path, PathBuf};

/// Sanitize a project name for use in filesystem paths.
///
/// Rejects names containing path separators, `..`, null bytes, or control characters.
/// Allows alphanumeric, `-`, `_`, `.`, and space.
pub fn sanitize_project_name(name: &str) -> Result<String, String> {
    if name.is_empty() {
        return Err("Project name cannot be empty".into());
    }
    if name.len() > 128 {
        return Err("Project name too long (max 128 characters)".into());
    }
    if name.contains('/') || name.contains('\\') {
        return Err("Project name cannot contain path separators".into());
    }
    if name.contains("..") {
        return Err("Project name cannot contain '..'".into());
    }
    if name.contains('\0') {
        return Err("Project name cannot contain null bytes".into());
    }
    if name.chars().any(|c| c.is_control()) {
        return Err("Project name cannot contain control characters".into());
    }
    Ok(name.to_string())
}

/// Sanitize an entrypoint path (must be relative, no directory traversal).
pub fn sanitize_entrypoint(entrypoint: &str) -> Result<String, String> {
    if entrypoint.is_empty() {
        return Err("Entrypoint cannot be empty".into());
    }
    if entrypoint.contains("..") {
        return Err("Entrypoint cannot contain '..' (directory traversal)".into());
    }
    if entrypoint.starts_with('/') || entrypoint.starts_with('\\') {
        return Err("Entrypoint must be a relative path".into());
    }
    if entrypoint.contains('\0') {
        return Err("Entrypoint cannot contain null bytes".into());
    }
    Ok(entrypoint.to_string())
}

/// Validate that a resolved path is within the expected base directory.
///
/// Both paths are canonicalized before comparison. Returns the canonical
/// target path on success.
pub fn validate_path_within(base: &Path, target: &Path) -> Result<PathBuf, String> {
    let canonical_base = base.canonicalize()
        .map_err(|e| format!("Invalid base path '{}': {e}", base.display()))?;
    let canonical_target = target.canonicalize()
        .map_err(|e| format!("Invalid path '{}': {e}", target.display()))?;

    if !canonical_target.starts_with(&canonical_base) {
        return Err(format!(
            "Path '{}' is outside the allowed directory '{}'",
            target.display(),
            base.display()
        ));
    }

    Ok(canonical_target)
}

/// Sanitize a filename (no path separators allowed).
pub fn sanitize_filename(name: &str) -> Result<String, String> {
    if name.is_empty() {
        return Err("Filename cannot be empty".into());
    }
    if name.contains('/') || name.contains('\\') {
        return Err("Filename cannot contain path separators".into());
    }
    if name.contains("..") {
        return Err("Filename cannot contain '..'".into());
    }
    if name.contains('\0') {
        return Err("Filename cannot contain null bytes".into());
    }
    Ok(name.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_valid_project_names() {
        assert!(sanitize_project_name("my-project").is_ok());
        assert!(sanitize_project_name("my_project").is_ok());
        assert!(sanitize_project_name("my project").is_ok());
        assert!(sanitize_project_name("project.v2").is_ok());
        assert!(sanitize_project_name("123").is_ok());
    }

    #[test]
    fn test_invalid_project_names() {
        assert!(sanitize_project_name("").is_err());
        assert!(sanitize_project_name("../evil").is_err());
        assert!(sanitize_project_name("foo/bar").is_err());
        assert!(sanitize_project_name("foo\\bar").is_err());
        assert!(sanitize_project_name("a\0b").is_err());
        assert!(sanitize_project_name("a\nb").is_err());
        let long = "a".repeat(129);
        assert!(sanitize_project_name(&long).is_err());
    }

    #[test]
    fn test_valid_entrypoints() {
        assert!(sanitize_entrypoint("main.py").is_ok());
        assert!(sanitize_entrypoint("src/main.py").is_ok());
        assert!(sanitize_entrypoint("script.sh").is_ok());
    }

    #[test]
    fn test_invalid_entrypoints() {
        assert!(sanitize_entrypoint("").is_err());
        assert!(sanitize_entrypoint("../../etc/passwd").is_err());
        assert!(sanitize_entrypoint("/etc/passwd").is_err());
        assert!(sanitize_entrypoint("a\0b").is_err());
    }

    #[test]
    fn test_validate_path_within() {
        let tmp = std::env::temp_dir();
        let valid = tmp.join("test_file");
        // Create the file so canonicalize works
        std::fs::write(&valid, "test").unwrap();

        assert!(validate_path_within(&tmp, &valid).is_ok());

        // Cleanup
        let _ = std::fs::remove_file(&valid);
    }

    #[test]
    fn test_valid_filenames() {
        assert!(sanitize_filename("main.py").is_ok());
        assert!(sanitize_filename("my-script.sh").is_ok());
    }

    #[test]
    fn test_invalid_filenames() {
        assert!(sanitize_filename("").is_err());
        assert!(sanitize_filename("../evil.py").is_err());
        assert!(sanitize_filename("foo/bar.py").is_err());
    }
}

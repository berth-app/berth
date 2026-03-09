use std::io::Write;
use std::path::PathBuf;

use sha2::{Digest, Sha256};

const GITHUB_REPO: &str = "berth-app/berth-agent";

fn berth_dir() -> PathBuf {
    dirs_next::home_dir()
        .unwrap_or_else(|| PathBuf::from("/tmp"))
        .join(".berth")
}

fn detect_arch() -> &'static str {
    match std::env::consts::ARCH {
        "x86_64" => "x86_64",
        "aarch64" => "aarch64",
        _ => "x86_64",
    }
}

fn installed_via_package_manager() -> bool {
    let exe = std::env::current_exe().unwrap_or_default();
    let exe_str = exe.to_string_lossy();
    // Binary in /usr/bin/ was likely installed via apt/yum
    exe_str.starts_with("/usr/bin/") && !exe_str.contains(".berth/bin/")
}

fn running_under_systemd() -> bool {
    std::env::var("INVOCATION_ID").is_ok()
}

/// Resolve the latest (or specific) version from GitHub Releases.
/// Returns (version_tag, download_url, sha256_url).
async fn resolve_release(
    client: &reqwest::Client,
    version: Option<&str>,
) -> anyhow::Result<(String, String, Option<String>)> {
    let arch = detect_arch();
    let asset_name = format!("berth-agent-linux-{arch}");

    if let Some(v) = version {
        let tag = if v.starts_with('v') { v.to_string() } else { format!("v{v}") };
        let url = format!(
            "https://github.com/{GITHUB_REPO}/releases/download/{tag}/{asset_name}"
        );
        let sha_url = format!("{url}.sha256");
        return Ok((tag, url, Some(sha_url)));
    }

    // Query latest release
    let api_url = format!("https://api.github.com/repos/{GITHUB_REPO}/releases/latest");
    let resp: serde_json::Value = client
        .get(&api_url)
        .header("Accept", "application/vnd.github+json")
        .send()
        .await?
        .error_for_status()?
        .json()
        .await?;

    let tag = resp["tag_name"]
        .as_str()
        .ok_or_else(|| anyhow::anyhow!("No tag_name in release"))?
        .to_string();

    let url = format!(
        "https://github.com/{GITHUB_REPO}/releases/download/{tag}/{asset_name}"
    );
    let sha_url = format!("{url}.sha256");

    Ok((tag, url, Some(sha_url)))
}

pub async fn run_update(version: Option<&str>, auto_yes: bool) -> anyhow::Result<()> {
    let current = env!("CARGO_PKG_VERSION");

    if installed_via_package_manager() {
        eprintln!("berth-agent was installed via a package manager.");
        eprintln!("Use your package manager to update (e.g., apt upgrade berth-agent).");
        std::process::exit(1);
    }

    let client = reqwest::Client::builder()
        .user_agent("berth-agent")
        .build()?;

    println!("Current version: {current}");
    println!("Checking for updates...");

    let (tag, download_url, sha_url) = resolve_release(&client, version).await?;
    let release_version = tag.trim_start_matches('v');

    if release_version == current && version.is_none() {
        println!("Already up to date (v{current}).");
        return Ok(());
    }

    println!("New version available: {tag}");

    if !auto_yes {
        print!("Upgrade to {tag}? [y/N] ");
        std::io::stdout().flush()?;
        let mut answer = String::new();
        std::io::stdin().read_line(&mut answer)?;
        if !matches!(answer.trim(), "y" | "Y" | "yes") {
            println!("Cancelled.");
            return Ok(());
        }
    }

    // Download binary
    println!("Downloading {tag}...");
    let data = client
        .get(&download_url)
        .send()
        .await?
        .error_for_status()?
        .bytes()
        .await?
        .to_vec();

    // Try to download and verify checksum
    if let Some(ref sha_url) = sha_url {
        if let Ok(resp) = client.get(sha_url).send().await {
            if resp.status().is_success() {
                if let Ok(text) = resp.text().await {
                    let expected = text.split_whitespace().next().unwrap_or("").to_lowercase();
                    let mut hasher = Sha256::new();
                    hasher.update(&data);
                    let actual = format!("{:x}", hasher.finalize());
                    if expected != actual {
                        eprintln!("Checksum mismatch! Expected {expected}, got {actual}");
                        std::process::exit(1);
                    }
                    println!("Checksum verified.");
                }
            }
        }
    }

    // Perform the binary swap
    let dir = berth_dir();
    let bin_dir = dir.join("bin");
    std::fs::create_dir_all(&bin_dir)?;

    let active_path = bin_dir.join("berth-agent");
    let staging_path = bin_dir.join("berth-agent.new");
    let backup_path = bin_dir.join("berth-agent.old");

    // Write staging binary
    std::fs::write(&staging_path, &data)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&staging_path, std::fs::Permissions::from_mode(0o755))?;
    }

    // Verify
    let output = std::process::Command::new(&staging_path)
        .arg("--version")
        .output()?;
    if !output.status.success() {
        let _ = std::fs::remove_file(&staging_path);
        eprintln!("New binary verification failed.");
        std::process::exit(1);
    }
    let new_version = String::from_utf8_lossy(&output.stdout).trim().to_string();
    println!("Verified: {new_version}");

    // Backup → promote (atomic renames on same filesystem)
    let _ = std::fs::remove_file(&backup_path);
    if active_path.exists() {
        std::fs::rename(&active_path, &backup_path)?;
    }
    std::fs::rename(&staging_path, &active_path)?;

    // Write markers for probation
    let meta = serde_json::json!({
        "old_version": current,
        "new_version": &new_version,
        "upgraded_at": chrono::Utc::now().to_rfc3339(),
    })
    .to_string();
    let _ = std::fs::write(dir.join(".upgrading"), &meta);
    let _ = std::fs::write(dir.join(".probation"), &meta);
    let _ = std::fs::remove_file(dir.join(".rollback-count"));

    if running_under_systemd() {
        println!("Restarting via systemd...");
        std::process::exit(super::EXIT_CODE_UPGRADE);
    } else {
        println!("Upgrade complete. Restart the agent to use {new_version}.");
    }

    Ok(())
}

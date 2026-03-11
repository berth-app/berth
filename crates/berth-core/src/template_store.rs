use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use uuid::Uuid;

use crate::project::Project;
use crate::runtime::Runtime;
use crate::store::ProjectStore;

const STORE_REPO_RAW_BASE: &str =
    "https://raw.githubusercontent.com/berth-app/berth-store/main";
const CACHE_TTL_SECS: u64 = 3600; // 1 hour

// --- Data types ---

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TemplateCategory {
    pub id: String,
    pub name: String,
    pub icon: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TemplateEnvHint {
    pub key: String,
    pub description: String,
    #[serde(default)]
    pub required: bool,
    #[serde(default)]
    pub default: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TemplateMeta {
    pub id: String,
    pub name: String,
    pub description: String,
    pub category: String,
    pub runtime: String,
    pub entrypoint: String,
    pub version: String,
    pub author: String,
    #[serde(default)]
    pub pro_only: bool,
    #[serde(default)]
    pub featured: bool,
    #[serde(default)]
    pub env_vars: Vec<TemplateEnvHint>,
    #[serde(default)]
    pub tags: Vec<String>,
    #[serde(default)]
    pub files: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StoreCatalog {
    pub version: u32,
    pub updated_at: String,
    pub categories: Vec<TemplateCategory>,
    pub templates: Vec<TemplateMeta>,
}

// --- Cache ---

fn cache_dir() -> PathBuf {
    dirs_next::cache_dir()
        .unwrap_or_else(|| PathBuf::from("/tmp"))
        .join("com.berth.app")
        .join("store")
}

fn save_catalog_cache(catalog: &StoreCatalog) -> Result<()> {
    let dir = cache_dir();
    std::fs::create_dir_all(&dir)?;
    let json = serde_json::to_string_pretty(catalog)?;
    std::fs::write(dir.join("index.json"), &json)?;
    std::fs::write(
        dir.join("cached_at.txt"),
        chrono::Utc::now().to_rfc3339(),
    )?;
    Ok(())
}

fn load_catalog_cache() -> Option<(StoreCatalog, chrono::DateTime<chrono::Utc>)> {
    let dir = cache_dir();
    let json = std::fs::read_to_string(dir.join("index.json")).ok()?;
    let catalog: StoreCatalog = serde_json::from_str(&json).ok()?;
    let ts_str = std::fs::read_to_string(dir.join("cached_at.txt")).ok()?;
    let ts: chrono::DateTime<chrono::Utc> = ts_str.trim().parse().ok()?;
    Some((catalog, ts))
}

// --- Fetch ---

async fn fetch_catalog() -> Result<StoreCatalog> {
    let url = format!("{}/index.json", STORE_REPO_RAW_BASE);
    let resp = reqwest::get(&url)
        .await
        .context("Failed to fetch template store catalog")?;
    let catalog: StoreCatalog = resp
        .json()
        .await
        .context("Failed to parse template store catalog")?;
    Ok(catalog)
}

/// Get the catalog, using cache when fresh enough.
/// Falls back to stale cache on network error.
pub async fn get_catalog(force_refresh: bool) -> Result<StoreCatalog> {
    if !force_refresh {
        if let Some((catalog, cached_at)) = load_catalog_cache() {
            let age = chrono::Utc::now()
                .signed_duration_since(cached_at)
                .num_seconds() as u64;
            if age < CACHE_TTL_SECS {
                return Ok(catalog);
            }
        }
    }

    match fetch_catalog().await {
        Ok(catalog) => {
            let _ = save_catalog_cache(&catalog);
            Ok(catalog)
        }
        Err(e) => {
            // Fallback to stale cache
            if let Some((catalog, _)) = load_catalog_cache() {
                tracing::warn!("Using stale catalog cache: {e}");
                Ok(catalog)
            } else {
                Err(e)
            }
        }
    }
}

// --- Search & filter ---

pub fn search_templates<'a>(
    catalog: &'a StoreCatalog,
    query: &str,
) -> Vec<&'a TemplateMeta> {
    let q = query.to_lowercase();
    catalog
        .templates
        .iter()
        .filter(|t| {
            t.name.to_lowercase().contains(&q)
                || t.description.to_lowercase().contains(&q)
                || t.category.to_lowercase().contains(&q)
                || t.tags.iter().any(|tag| tag.to_lowercase().contains(&q))
        })
        .collect()
}

pub fn filter_by_category<'a>(
    catalog: &'a StoreCatalog,
    category: &str,
) -> Vec<&'a TemplateMeta> {
    catalog
        .templates
        .iter()
        .filter(|t| t.category == category)
        .collect()
}

// --- Install ---

fn parse_runtime_str(s: &str) -> Runtime {
    match s {
        "python" => Runtime::Python,
        "node" => Runtime::Node,
        "go" => Runtime::Go,
        "rust" => Runtime::Rust,
        "shell" => Runtime::Shell,
        _ => Runtime::Unknown,
    }
}

/// Download template files from GitHub and create a Berth project.
/// If `tier` is None, Pro check is skipped (MCP mode).
///
/// Split into async download phase + sync store phase so that
/// `ProjectStore` (which is `!Send`) is never held across `.await`.
pub async fn install_template(
    store: &ProjectStore,
    template: &TemplateMeta,
) -> Result<Project> {
    let project_dir = download_template_files(template).await?;
    finalize_template_install(store, template, &project_dir)
}

/// Create the project record and set env vars after downloading.
/// Sync-only — safe to call from Tauri commands without Send issues.
pub fn finalize_template_install(
    store: &ProjectStore,
    template: &TemplateMeta,
    project_dir: &std::path::Path,
) -> Result<Project> {
    let runtime = parse_runtime_str(&template.runtime);
    let mut project = Project::new(
        template.name.clone(),
        project_dir.to_string_lossy().to_string(),
        runtime,
    );
    project.entrypoint = Some(template.entrypoint.clone());
    store.insert(&project)?;

    for hint in &template.env_vars {
        if let Some(default) = &hint.default {
            store.set_env_var(project.id, &hint.key, default)?;
        } else if hint.required {
            store.set_env_var(project.id, &hint.key, "")?;
        }
    }

    store.record_template_install(&template.id, project.id, &template.version)?;

    Ok(project)
}

/// Download all template files to a new project directory. Returns the path.
pub async fn download_template_files(template: &TemplateMeta) -> Result<PathBuf> {
    let short_id = &Uuid::new_v4().to_string()[..8];
    let project_dir = dirs_next::data_dir()
        .unwrap_or_else(|| PathBuf::from("/tmp"))
        .join("com.berth.app")
        .join("projects")
        .join(format!("{}-{}", template.id, short_id));
    std::fs::create_dir_all(&project_dir)
        .context("Failed to create project directory")?;

    let client = reqwest::Client::new();
    let mut handles = Vec::new();

    for file in &template.files {
        let url = format!("{}/{}/{}", STORE_REPO_RAW_BASE, template.id, file);
        let dest = project_dir.join(file);
        let c = client.clone();

        handles.push(tokio::spawn(async move {
            if let Some(parent) = dest.parent() {
                tokio::fs::create_dir_all(parent).await?;
            }
            let resp = c
                .get(&url)
                .send()
                .await
                .map_err(|e| anyhow::anyhow!("Failed to download {}: {}", url, e))?;
            if !resp.status().is_success() {
                anyhow::bail!("Failed to download {} (HTTP {})", url, resp.status());
            }
            let bytes = resp.bytes().await?;
            tokio::fs::write(&dest, &bytes).await?;
            Ok::<(), anyhow::Error>(())
        }));
    }

    for handle in handles {
        handle
            .await
            .context("Download task panicked")?
            .context("Failed to download template file")?;
    }

    Ok(project_dir)
}

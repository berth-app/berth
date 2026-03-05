use runway_core::project::Project;
use runway_core::runtime::{self, RuntimeInfo};
use runway_core::store::ProjectStore;
use serde::Serialize;
use std::path::Path;

fn get_store() -> Result<ProjectStore, String> {
    let data_dir = dirs_next::data_dir()
        .unwrap_or_else(|| std::path::PathBuf::from("."))
        .join("com.runway.app");
    std::fs::create_dir_all(&data_dir).map_err(|e| e.to_string())?;
    let db_path = data_dir.join("runway.db");
    ProjectStore::open(db_path.to_str().unwrap_or("runway.db")).map_err(|e| e.to_string())
}

#[derive(Serialize)]
pub struct ProjectResponse {
    pub projects: Vec<Project>,
}

#[tauri::command]
pub fn list_projects() -> Result<ProjectResponse, String> {
    let store = get_store()?;
    let projects = store.list().map_err(|e| e.to_string())?;
    Ok(ProjectResponse { projects })
}

#[tauri::command]
pub fn create_project(name: String, path: String) -> Result<Project, String> {
    let store = get_store()?;
    let info = runtime::detect_runtime(Path::new(&path));
    let mut project = Project::new(name, path, info.runtime);
    project.entrypoint = info.entrypoint;
    store.insert(&project).map_err(|e| e.to_string())?;
    Ok(project)
}

#[tauri::command]
pub fn detect_runtime(path: String) -> RuntimeInfo {
    runtime::detect_runtime(Path::new(&path))
}

#[tauri::command]
pub fn delete_project(id: String) -> Result<(), String> {
    let store = get_store()?;
    let uuid = id.parse().map_err(|e: uuid::Error| e.to_string())?;
    store.delete(uuid).map_err(|e| e.to_string())
}

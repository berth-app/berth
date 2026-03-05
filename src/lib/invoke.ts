import { invoke as tauriInvoke } from "@tauri-apps/api/core";

export interface Project {
  id: string;
  name: string;
  path: string;
  runtime: string;
  entrypoint: string | null;
  status: "idle" | "running" | "stopped" | "failed";
  created_at: string;
  updated_at: string;
}

export interface RuntimeInfo {
  runtime: string;
  version_file: string | null;
  entrypoint: string | null;
  confidence: number;
}

export async function listProjects(): Promise<Project[]> {
  const result = await tauriInvoke<{ projects: Project[] }>("list_projects");
  return result.projects;
}

export async function createProject(
  name: string,
  path: string
): Promise<Project> {
  return tauriInvoke<Project>("create_project", { name, path });
}

export async function detectRuntime(path: string): Promise<RuntimeInfo> {
  return tauriInvoke<RuntimeInfo>("detect_runtime", { path });
}

export async function deleteProject(id: string): Promise<void> {
  return tauriInvoke("delete_project", { id });
}

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

export async function savePasteCode(
  name: string,
  code: string
): Promise<string> {
  return tauriInvoke<string>("save_paste_code", { name, code });
}

export async function deleteProject(id: string): Promise<void> {
  return tauriInvoke("delete_project", { id });
}

export async function runProject(id: string): Promise<void> {
  return tauriInvoke("run_project", { id });
}

export async function stopProject(id: string): Promise<void> {
  return tauriInvoke("stop_project", { id });
}

export interface LogEvent {
  project_id: string;
  stream: "stdout" | "stderr";
  text: string;
  timestamp: string;
}

export interface StatusEvent {
  project_id: string;
  status: "idle" | "running" | "stopped" | "failed";
}

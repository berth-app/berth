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
  last_run_at: string | null;
  last_exit_code: number | null;
  run_count: number;
  notify_on_complete: boolean;
  default_target: string | null;
  tunnel_url: string | null;
  tunnel_provider: string | null;
  run_mode: "oneshot" | "service";
  service_port: number | null;
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

export async function updateProject(
  id: string,
  name: string,
  entrypoint: string | null
): Promise<void> {
  return tauriInvoke("update_project", { id, name, entrypoint });
}

export async function deleteProject(id: string): Promise<void> {
  return tauriInvoke("delete_project", { id });
}

export async function runProject(id: string, target?: string): Promise<void> {
  return tauriInvoke("run_project", { id, target: target ?? null });
}

export async function stopProject(id: string, target?: string): Promise<void> {
  return tauriInvoke("stop_project", { id, target: target ?? null });
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
  exit_code: number | null;
}

// --- Targets ---

export interface TargetInfo {
  id: string;
  name: string;
  kind: string;
  host: string | null;
  port: number;
  status: string;
  agent_version: string | null;
  last_seen_at: string | null;
  nats_agent_id: string | null;
  nats_enabled: boolean;
  tunnel_providers: string[];
}

export async function listTargets(): Promise<TargetInfo[]> {
  return tauriInvoke<TargetInfo[]>("list_targets");
}

export async function addTarget(
  name: string,
  host: string,
  port: number,
  natsAgentId?: string
): Promise<TargetInfo> {
  return tauriInvoke<TargetInfo>("add_target", {
    name,
    host,
    port,
    nats_agent_id: natsAgentId || null,
  });
}

export async function removeTarget(id: string): Promise<void> {
  return tauriInvoke("remove_target", { id });
}

export async function updateTargetNats(
  id: string,
  natsAgentId: string,
  natsEnabled: boolean
): Promise<void> {
  return tauriInvoke("update_target_nats", {
    id,
    nats_agent_id: natsAgentId,
    nats_enabled: natsEnabled,
  });
}

export async function pingTarget(id: string): Promise<TargetInfo> {
  return tauriInvoke<TargetInfo>("ping_target", { id });
}

export interface AgentRunningProject {
  project_id: string;
  status: string;
  started_at: string;
}

export interface AgentStats {
  agent_id: string;
  version: string;
  status: string;
  uptime_seconds: number;
  cpu_usage: number;
  memory_mb: number;
  podman_version: string | null;
  container_ready: boolean;
  running_projects: AgentRunningProject[];
  os: string | null;
  arch: string | null;
  tunnel_providers: string[];
}

export async function getAgentStats(id: string): Promise<AgentStats> {
  return tauriInvoke<AgentStats>("get_agent_stats", { id });
}

// --- Schedules ---

export interface ScheduleInfo {
  id: string;
  project_id: string;
  cron_expr: string;
  enabled: boolean;
  created_at: string;
  last_triggered_at: string | null;
  next_run_at: string | null;
}

export async function listSchedules(projectId: string): Promise<ScheduleInfo[]> {
  return tauriInvoke<ScheduleInfo[]>("list_schedules", { projectId });
}

export async function addSchedule(
  projectId: string,
  cronExpr: string
): Promise<ScheduleInfo> {
  return tauriInvoke<ScheduleInfo>("add_schedule", { projectId, cronExpr });
}

export async function removeSchedule(id: string): Promise<void> {
  return tauriInvoke("remove_schedule", { id });
}

export async function toggleSchedule(
  id: string,
  enabled: boolean
): Promise<void> {
  return tauriInvoke("toggle_schedule", { id, enabled });
}

// --- Execution Logs ---

export interface ExecutionLogInfo {
  id: string;
  project_id: string;
  started_at: string;
  finished_at: string | null;
  exit_code: number | null;
  output: string;
  trigger: string;
}

export async function listExecutionLogs(
  projectId: string,
  limit?: number
): Promise<ExecutionLogInfo[]> {
  return tauriInvoke<ExecutionLogInfo[]>("list_execution_logs", {
    projectId,
    limit: limit ?? null,
  });
}

// --- Settings ---

export async function getSettings(): Promise<Record<string, string>> {
  return tauriInvoke<Record<string, string>>("get_settings");
}

export async function updateSetting(
  key: string,
  value: string
): Promise<void> {
  return tauriInvoke("update_setting", { key, value });
}

// --- Project Notification Setting ---

export async function setProjectTarget(
  id: string,
  targetId: string | null
): Promise<void> {
  return tauriInvoke("set_project_target", { id, targetId });
}

export async function setProjectNotify(
  id: string,
  enabled: boolean
): Promise<void> {
  return tauriInvoke("set_project_notify", { id, enabled });
}

export async function setProjectRunMode(
  id: string,
  runMode: "oneshot" | "service",
  servicePort?: number
): Promise<void> {
  return tauriInvoke("set_project_run_mode", {
    id,
    runMode,
    servicePort: servicePort ?? null,
  });
}

// --- Project File Access ---

export async function readProjectFile(id: string): Promise<string> {
  return tauriInvoke<string>("read_project_file", { id });
}

export async function writeProjectFile(
  id: string,
  content: string
): Promise<void> {
  return tauriInvoke("write_project_file", { id, content });
}

// --- File Import ---

export async function importFile(filePath: string): Promise<Project> {
  return tauriInvoke<Project>("import_file", { filePath });
}

// --- Agent Upgrade ---

export interface UpgradeCheck {
  available: boolean;
  current_version: string;
  latest_version: string;
  arch: string | null;
}

export interface UpgradeResult {
  target_id: string;
  target_name: string;
  success: boolean;
  new_version: string;
  message: string;
}

export interface RollbackResult {
  success: boolean;
  restored_version: string;
  message: string;
}

export async function checkAgentUpgrade(id: string): Promise<UpgradeCheck> {
  return tauriInvoke<UpgradeCheck>("check_agent_upgrade", { id });
}

export async function upgradeAgent(id: string): Promise<UpgradeResult> {
  return tauriInvoke<UpgradeResult>("upgrade_agent", { id });
}

export async function rollbackAgent(id: string): Promise<RollbackResult> {
  return tauriInvoke<RollbackResult>("rollback_agent", { id });
}

export async function upgradeAllAgents(): Promise<UpgradeResult[]> {
  return tauriInvoke<UpgradeResult[]>("upgrade_all_agents");
}

// --- Publish / Tunnel ---

export interface PublishResult {
  success: boolean;
  url: string;
  provider: string;
  message: string;
}

export interface UnpublishResult {
  success: boolean;
  message: string;
}

export async function publishProject(
  id: string,
  port: number,
  provider?: string,
  target?: string,
): Promise<PublishResult> {
  return tauriInvoke<PublishResult>("publish_project", {
    id,
    port,
    provider: provider ?? null,
    target: target ?? null,
  });
}

export async function unpublishProject(
  id: string,
  target?: string,
): Promise<UnpublishResult> {
  return tauriInvoke<UnpublishResult>("unpublish_project", {
    id,
    target: target ?? null,
  });
}

// --- Environment Variables ---

export async function getEnvVars(
  projectId: string
): Promise<Record<string, string>> {
  return tauriInvoke<Record<string, string>>("get_env_vars", { projectId });
}

export async function setEnvVar(
  projectId: string,
  key: string,
  value: string
): Promise<void> {
  return tauriInvoke("set_env_var", { projectId, key, value });
}

export async function deleteEnvVar(
  projectId: string,
  key: string
): Promise<void> {
  return tauriInvoke("delete_env_var", { projectId, key });
}

export async function importEnvFile(
  projectId: string,
  content: string
): Promise<number> {
  return tauriInvoke<number>("import_env_file", { projectId, content });
}


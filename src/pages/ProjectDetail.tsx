import { useEffect, useState, useCallback, useRef } from "react";
import { listen } from "@tauri-apps/api/event";
import {
  Play,
  Square,
  Trash2,
  Bell,
  Clock,
  History,
  X,
  ChevronDown,
  Settings,
} from "lucide-react";
import {
  runProject,
  stopProject,
  deleteProject,
  listProjects,
  listTargets,
  listSchedules,
  addSchedule,
  removeSchedule,
  toggleSchedule,
  listExecutionLogs,
  readProjectFile,
  writeProjectFile,
  setProjectNotify,
  setProjectTarget,
  type Project,
  type LogEvent,
  type StatusEvent,
  type TargetInfo,
  type ScheduleInfo,
  type ExecutionLogInfo,
} from "../lib/invoke";
import { useToast } from "../components/Toast";
import Terminal from "../components/Terminal";

interface Props {
  projectId: string;
  onBack: () => void;
}

const STATUS_COLORS: Record<string, string> = {
  idle: "bg-runway-text-tertiary",
  running: "bg-runway-success",
  stopped: "bg-runway-text-tertiary",
  failed: "bg-runway-error",
};

function timeAgo(dateStr: string | null): string {
  if (!dateStr) return "never";
  const diff = Date.now() - new Date(dateStr).getTime();
  if (diff < 60000) return "just now";
  if (diff < 3600000) return `${Math.floor(diff / 60000)}m ago`;
  if (diff < 86400000) return `${Math.floor(diff / 3600000)}h ago`;
  return `${Math.floor(diff / 86400000)}d ago`;
}

function formatDate(dateStr: string | null): string {
  if (!dateStr) return "--";
  return new Date(dateStr).toLocaleString(undefined, {
    month: "short",
    day: "numeric",
    hour: "2-digit",
    minute: "2-digit",
  });
}

function humanizeCron(expr: string): string {
  if (expr === "@hourly") return "Every hour";
  if (expr === "@daily") return "Daily at midnight";
  if (expr === "@weekly") return "Weekly on Sunday";
  const everyMatch = expr.match(/^@every\s+(\d+)\s*(s|m|h)$/);
  if (everyMatch) {
    const [, n, unit] = everyMatch;
    const units: Record<string, string> = { s: "second", m: "minute", h: "hour" };
    return `Every ${n} ${units[unit]}${Number(n) > 1 ? "s" : ""}`;
  }
  return expr;
}

// ─── Schedule Panel ────────────────────────────────────────────────

function SchedulePanel({ projectId }: { projectId: string }) {
  const [schedules, setSchedules] = useState<ScheduleInfo[]>([]);
  const [cronExpr, setCronExpr] = useState("");
  const [adding, setAdding] = useState(false);
  const { toast } = useToast();

  const refresh = useCallback(() => {
    listSchedules(projectId).then(setSchedules).catch(console.error);
  }, [projectId]);

  useEffect(() => {
    refresh();
  }, [refresh]);

  useEffect(() => {
    const unlisten = listen<{
      project_id: string;
      success: boolean;
      exit_code: number | null;
    }>("schedule-executed", (event) => {
      if (event.payload.project_id === projectId) {
        refresh();
        const msg = event.payload.success
          ? "Schedule ran successfully"
          : `Schedule failed (exit ${event.payload.exit_code})`;
        toast(msg, event.payload.success ? "success" : "error");
      }
    });
    return () => {
      unlisten.then((fn) => fn());
    };
  }, [projectId, refresh, toast]);

  async function handleAdd() {
    const expr = cronExpr.trim();
    if (!expr) return;
    setAdding(true);
    try {
      await addSchedule(projectId, expr);
      toast("Schedule added", "success");
      setCronExpr("");
      refresh();
    } catch (e) {
      toast(`Failed: ${e}`, "error");
    } finally {
      setAdding(false);
    }
  }

  async function handleRemove(id: string) {
    try {
      await removeSchedule(id);
      toast("Schedule removed", "info");
      refresh();
    } catch (e) {
      toast(`Failed: ${e}`, "error");
    }
  }

  async function handleToggle(id: string, enabled: boolean) {
    try {
      await toggleSchedule(id, enabled);
      refresh();
    } catch (e) {
      toast(`Failed: ${e}`, "error");
    }
  }

  const quickPresets = ["@every 5m", "@hourly", "@daily"];

  return (
    <div className="side-panel-body flex flex-col gap-4">
      {schedules.length === 0 ? (
        <div className="text-xs text-runway-text-tertiary py-4 text-center">
          No schedules yet
        </div>
      ) : (
        <div className="flex flex-col gap-1">
          {schedules.map((s) => (
            <div
              key={s.id}
              className="flex items-start gap-3 py-2.5 border-b border-runway-border-subtle last:border-b-0 group"
            >
              <button
                onClick={() => handleToggle(s.id, !s.enabled)}
                className={`w-2 h-2 rounded-full mt-1.5 shrink-0 transition-colors cursor-pointer ${
                  s.enabled ? "bg-runway-success" : "bg-runway-text-tertiary"
                }`}
                title={s.enabled ? "Active — click to pause" : "Paused — click to enable"}
              />
              <div className="flex-1 min-w-0">
                <span className="schedule-expr">
                  {s.cron_expr}
                </span>
                <div className="text-[11px] text-runway-text-tertiary mt-1">
                  {humanizeCron(s.cron_expr)}
                  {s.next_run_at && <span className="ml-2">Next: {formatDate(s.next_run_at)}</span>}
                </div>
              </div>
              <button
                onClick={() => handleRemove(s.id)}
                className="opacity-0 group-hover:opacity-100 transition-opacity text-runway-text-tertiary hover:text-runway-error p-0.5"
              >
                <X size={12} />
              </button>
            </div>
          ))}
        </div>
      )}

      <div className="border-t border-runway-border-subtle pt-3">
        <div className="flex gap-2">
          <input
            type="text"
            value={cronExpr}
            onChange={(e) => setCronExpr(e.target.value)}
            placeholder="@every 5m, 30 9 * * *"
            onKeyDown={(e) => {
              if (e.key === "Enter") handleAdd();
            }}
            className="input !py-1.5 !text-xs flex-1"
          />
          <button
            onClick={handleAdd}
            disabled={adding || !cronExpr.trim()}
            className="btn btn-primary btn-sm"
          >
            {adding ? "..." : "Add"}
          </button>
        </div>
        <div className="flex gap-1.5 mt-2">
          {quickPresets.map((p) => (
            <button
              key={p}
              onClick={() => setCronExpr(p)}
              className="schedule-quick-btn"
            >
              {p}
            </button>
          ))}
        </div>
      </div>
    </div>
  );
}

// ─── History Panel ─────────────────────────────────────────────────

function HistoryPanel({
  projectId,
  refreshKey,
}: {
  projectId: string;
  refreshKey: number;
}) {
  const [logs, setLogs] = useState<ExecutionLogInfo[]>([]);

  useEffect(() => {
    listExecutionLogs(projectId, 30).then(setLogs).catch(console.error);
  }, [projectId, refreshKey]);

  if (logs.length === 0) {
    return (
      <div className="side-panel-body">
        <div className="text-xs text-runway-text-tertiary py-4 text-center">
          No executions yet
        </div>
      </div>
    );
  }

  return (
    <div className="side-panel-body">
      <div className="history-timeline">
        {logs.map((log) => {
          const isSuccess = log.exit_code === 0;
          const isRunning = log.finished_at === null;
          const dotClass = isRunning
            ? "history-dot--running"
            : isSuccess
              ? "history-dot--success"
              : "history-dot--error";

          let duration = "";
          if (log.started_at && log.finished_at) {
            const ms =
              new Date(log.finished_at).getTime() -
              new Date(log.started_at).getTime();
            if (ms < 1000) duration = `${ms}ms`;
            else if (ms < 60000) duration = `${(ms / 1000).toFixed(1)}s`;
            else duration = `${Math.floor(ms / 60000)}m ${Math.floor((ms % 60000) / 1000)}s`;
          }

          return (
            <div key={log.id} className="history-item">
              <div className={`history-dot ${dotClass}`} />
              <div className="flex items-center gap-2 text-[11px]">
                <span className="text-runway-text-secondary">
                  {formatDate(log.started_at)}
                </span>
                <span
                  className={`badge ${
                    log.trigger === "schedule" ? "badge-warning" : "badge-accent"
                  }`}
                  style={{ fontSize: 9, padding: "1px 6px" }}
                >
                  {log.trigger}
                </span>
                {!isRunning && (
                  <span
                    className={`font-mono ${
                      isSuccess ? "text-runway-success" : "text-runway-error"
                    }`}
                  >
                    exit {log.exit_code}
                  </span>
                )}
                {isRunning && (
                  <span className="text-runway-accent">running</span>
                )}
                {duration && (
                  <span className="text-runway-text-tertiary ml-auto">
                    {duration}
                  </span>
                )}
              </div>
              {!isSuccess && !isRunning && log.output && (
                <div className="mt-1 text-[10px] font-mono text-runway-error/70 truncate">
                  {log.output.split("\n")[0]?.slice(0, 80)}
                </div>
              )}
            </div>
          );
        })}
      </div>
    </div>
  );
}

// ─── Settings Panel ────────────────────────────────────────────────

function SettingsPanel({
  project,
  notifyEnabled,
  onToggleNotify,
  onDelete,
}: {
  project: Project;
  notifyEnabled: boolean;
  onToggleNotify: () => void;
  onDelete: () => void;
}) {
  const [confirmDelete, setConfirmDelete] = useState(false);

  return (
    <div className="side-panel-body flex flex-col gap-5">
      <div>
        <div className="flex items-center justify-between">
          <div className="flex items-center gap-2">
            <Bell size={13} className="text-runway-text-secondary" />
            <span className="text-xs font-medium text-runway-text-primary">
              Notifications
            </span>
          </div>
          <button
            onClick={onToggleNotify}
            className="toggle"
            data-checked={notifyEnabled}
            style={{ width: 32, height: 19 }}
          />
        </div>
        <div className="text-[11px] text-runway-text-tertiary mt-1 ml-5">
          Notify on completion or failure
        </div>
      </div>

      <div className="border-t border-runway-border-subtle pt-4">
        <div className="text-[11px] text-runway-text-tertiary uppercase tracking-wider font-medium mb-2">
          Info
        </div>
        <div className="space-y-2">
          <div>
            <div className="text-[10px] text-runway-text-tertiary">Entrypoint</div>
            <div className="text-xs font-mono text-runway-text-secondary">
              {project.entrypoint ?? "none"}
            </div>
          </div>
          <div>
            <div className="text-[10px] text-runway-text-tertiary">Runtime</div>
            <div className="text-xs text-runway-text-secondary">
              {project.runtime}
            </div>
          </div>
          <div>
            <div className="text-[10px] text-runway-text-tertiary">Path</div>
            <div className="text-xs font-mono text-runway-text-secondary truncate">
              {project.path}
            </div>
          </div>
        </div>
      </div>

      <div className="border-t border-runway-border-subtle pt-4 mt-auto">
        {!confirmDelete ? (
          <button
            onClick={() => setConfirmDelete(true)}
            className="btn btn-danger w-full"
          >
            <Trash2 size={13} />
            Delete Project
          </button>
        ) : (
          <div className="space-y-2">
            <div className="text-xs text-runway-error text-center">
              Are you sure? This cannot be undone.
            </div>
            <div className="flex gap-2">
              <button
                onClick={() => setConfirmDelete(false)}
                className="btn btn-secondary flex-1"
              >
                Cancel
              </button>
              <button onClick={onDelete} className="btn btn-danger flex-1">
                Delete
              </button>
            </div>
          </div>
        )}
      </div>
    </div>
  );
}

// ─── Target Dropdown ───────────────────────────────────────────────

function TargetDropdown({
  targets,
  selectedTarget,
  onSelect,
}: {
  targets: TargetInfo[];
  selectedTarget: string | null;
  onSelect: (id: string | null) => void;
}) {
  const [open, setOpen] = useState(false);
  const ref = useRef<HTMLDivElement>(null);

  useEffect(() => {
    if (!open) return;
    function handleClick(e: MouseEvent) {
      if (ref.current && !ref.current.contains(e.target as Node)) {
        setOpen(false);
      }
    }
    document.addEventListener("mousedown", handleClick);
    return () => document.removeEventListener("mousedown", handleClick);
  }, [open]);

  const selected = selectedTarget
    ? targets.find((t) => t.id === selectedTarget)
    : null;
  const label = selected ? selected.name : "local";
  const dotColor = selected
    ? selected.status === "online"
      ? "bg-runway-success"
      : selected.status === "offline"
        ? "bg-runway-error"
        : "bg-runway-text-tertiary"
    : "bg-runway-success";

  return (
    <div className="relative" ref={ref}>
      <button
        onClick={() => setOpen(!open)}
        className="target-trigger"
      >
        <span className={`w-1.5 h-1.5 rounded-full ${dotColor}`} />
        {label}
        <ChevronDown size={10} className="text-runway-text-tertiary" />
      </button>

      {open && (
        <div className="target-popover">
          <button
            onClick={() => {
              onSelect(null);
              setOpen(false);
            }}
            className="target-option"
            data-selected={selectedTarget === null}
          >
            <span className="w-1.5 h-1.5 rounded-full bg-runway-success" />
            local
          </button>
          {targets.map((t) => (
            <button
              key={t.id}
              onClick={() => {
                onSelect(t.id);
                setOpen(false);
              }}
              className="target-option"
              data-selected={selectedTarget === t.id}
            >
              <span
                className={`w-1.5 h-1.5 rounded-full ${
                  t.status === "online"
                    ? "bg-runway-success"
                    : t.status === "offline"
                      ? "bg-runway-error"
                      : "bg-runway-text-tertiary"
                }`}
              />
              {t.name}
              <span className="text-[10px] text-runway-text-tertiary ml-auto">
                {t.kind}
              </span>
            </button>
          ))}
        </div>
      )}
    </div>
  );
}

// ─── Main Component ────────────────────────────────────────────────

type PanelType = "schedule" | "history" | "settings" | null;

export default function ProjectDetail({ projectId, onBack }: Props) {
  const [project, setProject] = useState<Project | null>(null);
  const [status, setStatus] = useState<string>("idle");
  const [logs, setLogs] = useState<LogEvent[]>([]);
  const [error, setError] = useState<string | null>(null);
  const [startedAt, setStartedAt] = useState<number | null>(null);
  const [uptime, setUptime] = useState("");
  const [targets, setTargets] = useState<TargetInfo[]>([]);
  const [selectedTarget, setSelectedTarget] = useState<string | null>(null);
  const [historyRefreshKey, setHistoryRefreshKey] = useState(0);
  const [notifyEnabled, setNotifyEnabled] = useState(true);
  const [activeTab, setActiveTab] = useState<"logs" | "code">("logs");
  const [fileContent, setFileContent] = useState<string | null>(null);
  const [fileLoading, setFileLoading] = useState(false);
  const [fileDirty, setFileDirty] = useState(false);
  const [activePanel, setActivePanel] = useState<PanelType>(null);
  const { toast } = useToast();

  useEffect(() => {
    listProjects().then((projects) => {
      const p = projects.find((p) => p.id === projectId);
      if (p) {
        setProject(p);
        setStatus(p.status);
        setNotifyEnabled(p.notify_on_complete);
        setSelectedTarget(p.default_target);
        if (p.status === "running") setStartedAt(Date.now());
      }
    });
    listTargets().then(setTargets).catch(console.error);
  }, [projectId]);

  useEffect(() => {
    const unlisten = listen<LogEvent>("project-log", (event) => {
      if (event.payload.project_id === projectId) {
        setLogs((prev) => [...prev, event.payload]);
      }
    });
    return () => {
      unlisten.then((fn) => fn());
    };
  }, [projectId]);

  useEffect(() => {
    const unlisten = listen<StatusEvent>(
      "project-status-change",
      (event) => {
        if (event.payload.project_id === projectId) {
          setStatus(event.payload.status);
          if (event.payload.status === "running") {
            setStartedAt(Date.now());
          } else {
            setStartedAt(null);
            setHistoryRefreshKey((k) => k + 1);
            if (event.payload.status === "failed") {
              toast(
                `Process exited with code ${event.payload.exit_code ?? "unknown"}`,
                "error"
              );
            } else if (event.payload.status === "idle") {
              toast("Process completed successfully", "success");
            }
          }
        }
      }
    );
    return () => {
      unlisten.then((fn) => fn());
    };
  }, [projectId, toast]);

  useEffect(() => {
    if (!startedAt) {
      setUptime("");
      return;
    }
    const interval = setInterval(() => {
      const secs = Math.floor((Date.now() - startedAt) / 1000);
      const m = Math.floor(secs / 60);
      const s = secs % 60;
      setUptime(`${m}:${s.toString().padStart(2, "0")}`);
    }, 1000);
    return () => clearInterval(interval);
  }, [startedAt]);

  const loadCode = useCallback(async () => {
    setFileLoading(true);
    try {
      const content = await readProjectFile(projectId);
      setFileContent(content);
      setFileDirty(false);
    } catch (e) {
      setFileContent(null);
      toast(`Failed to load code: ${e}`, "error");
    } finally {
      setFileLoading(false);
    }
  }, [projectId, toast]);

  useEffect(() => {
    if (activeTab === "code" && fileContent === null && !fileLoading) {
      loadCode();
    }
  }, [activeTab, fileContent, fileLoading, loadCode]);

  const handleSaveCode = useCallback(async () => {
    if (fileContent === null) return;
    try {
      await writeProjectFile(projectId, fileContent);
      setFileDirty(false);
      toast("File saved", "success");
    } catch (e) {
      toast(`Failed to save: ${e}`, "error");
    }
  }, [projectId, fileContent, toast]);

  const handleToggleNotify = useCallback(async () => {
    const next = !notifyEnabled;
    setNotifyEnabled(next);
    try {
      await setProjectNotify(projectId, next);
    } catch (e) {
      setNotifyEnabled(!next);
      toast(`Failed to update: ${e}`, "error");
    }
  }, [projectId, notifyEnabled, toast]);

  const handleRun = useCallback(async () => {
    setError(null);
    setLogs([]);
    try {
      await runProject(projectId, selectedTarget || undefined);
      if (selectedTarget) {
        const t = targets.find((t) => t.id === selectedTarget);
        toast(`Started on ${t?.name ?? "remote"}`, "success");
      } else {
        toast("Project started", "success");
      }
    } catch (e) {
      const msg = String(e);
      setError(msg);
      toast(msg, "error");
    }
  }, [projectId, selectedTarget, targets, toast]);

  const handleStop = useCallback(async () => {
    setError(null);
    try {
      await stopProject(projectId, selectedTarget || undefined);
      toast("Project stopped", "info");
    } catch (e) {
      const msg = String(e);
      setError(msg);
      toast(msg, "error");
    }
  }, [projectId, selectedTarget, toast]);

  const handleDelete = useCallback(async () => {
    try {
      await deleteProject(projectId);
      toast("Project deleted", "info");
      onBack();
    } catch (e) {
      toast(String(e), "error");
    }
  }, [projectId, onBack, toast]);

  const handleSelectTarget = useCallback(
    (id: string | null) => {
      setSelectedTarget(id);
      setProjectTarget(projectId, id).catch(console.error);
    },
    [projectId]
  );

  const togglePanel = useCallback((panel: PanelType) => {
    setActivePanel((prev) => (prev === panel ? null : panel));
  }, []);

  const isRunning = status === "running";

  if (!project) {
    return (
      <div className="h-full flex flex-col p-5">
        <div className="skeleton h-6 w-48 mb-4" />
        <div className="skeleton h-10 w-full mb-3" />
        <div className="skeleton flex-1 w-full" />
      </div>
    );
  }

  const exitCodeColor =
    project.last_exit_code === null
      ? "text-runway-text-tertiary"
      : project.last_exit_code === 0
        ? "text-runway-success"
        : "text-runway-error";

  return (
    <div className="h-full flex flex-col animate-page-enter">
      {/* ── Header ──────────────────────────────────────────────── */}
      <div className="h-[52px] flex items-center px-5 gap-3 border-b border-runway-border-subtle shrink-0">
        <h1 className="text-[15px] font-semibold text-runway-text-primary tracking-tight truncate">
          {project.name}
        </h1>
        <span className="badge badge-neutral">{project.runtime}</span>

        {/* Status cluster */}
        <div className="flex items-center gap-2 ml-2">
          <div
            className={`w-2 h-2 rounded-full ${
              STATUS_COLORS[status] ?? STATUS_COLORS.idle
            } ${isRunning ? "animate-pulse-soft" : ""}`}
          />
          <span className="text-xs text-runway-text-secondary capitalize">
            {status}
          </span>
          {uptime && (
            <span className="text-xs font-mono text-runway-text-tertiary tabular-nums">
              {uptime}
            </span>
          )}
        </div>

        {/* Actions */}
        <div className="ml-auto flex items-center gap-2">
          <button
            onClick={handleRun}
            disabled={isRunning}
            className="btn btn-primary"
          >
            <Play size={12} fill="currentColor" />
            Run
          </button>
          <button
            onClick={handleStop}
            disabled={!isRunning}
            className="btn btn-secondary"
          >
            <Square size={12} fill="currentColor" />
            Stop
          </button>
        </div>
      </div>

      {/* ── Toolbar ─────────────────────────────────────────────── */}
      <div className="h-10 flex items-center px-5 gap-4 border-b border-runway-border-subtle shrink-0">
        {/* Target dropdown */}
        {targets.length > 0 && (
          <TargetDropdown
            targets={targets}
            selectedTarget={selectedTarget}
            onSelect={handleSelectTarget}
          />
        )}

        {/* Inline stats */}
        <div className="flex items-center gap-0 text-xs text-runway-text-secondary tabular-nums">
          <span>{project.run_count} runs</span>
          <span className="text-runway-text-tertiary mx-2">&middot;</span>
          <span>{timeAgo(project.last_run_at)}</span>
          <span className="text-runway-text-tertiary mx-2">&middot;</span>
          <span className={exitCodeColor}>
            exit {project.last_exit_code !== null ? project.last_exit_code : "--"}
          </span>
        </div>

        {/* Panel toggles */}
        <div className="ml-auto flex items-center gap-1">
          <button
            onClick={() => togglePanel("schedule")}
            className="toolbar-icon"
            data-active={activePanel === "schedule"}
            title="Schedules"
          >
            <Clock size={15} strokeWidth={1.75} />
          </button>
          <button
            onClick={() => togglePanel("history")}
            className="toolbar-icon"
            data-active={activePanel === "history"}
            title="History"
          >
            <History size={15} strokeWidth={1.75} />
          </button>
          <button
            onClick={() => togglePanel("settings")}
            className="toolbar-icon"
            data-active={activePanel === "settings"}
            title="Settings"
          >
            <Settings size={15} strokeWidth={1.75} />
          </button>
        </div>
      </div>

      {/* ── Error banner ────────────────────────────────────────── */}
      {error && (
        <div className="px-5 py-2 bg-runway-error-bg border-b border-runway-error/20 flex items-center gap-2">
          <span className="text-xs text-runway-error flex-1">{error}</span>
          <button
            onClick={() => setError(null)}
            className="text-runway-error/60 hover:text-runway-error"
          >
            <X size={12} />
          </button>
        </div>
      )}

      {/* ── Workspace ───────────────────────────────────────────── */}
      <div className="flex-1 flex flex-col min-h-0 relative">
        {/* Tab bar */}
        <div className="workspace-tab-bar">
          <button
            onClick={() => setActiveTab("logs")}
            className="workspace-tab"
            data-active={activeTab === "logs"}
          >
            Output
          </button>
          <button
            onClick={() => setActiveTab("code")}
            className="workspace-tab"
            data-active={activeTab === "code"}
          >
            Code{fileDirty ? " *" : ""}
          </button>
        </div>

        {/* Tab content */}
        <div className="flex-1 flex flex-col min-h-0">
          {activeTab === "logs" && (
            <>
              {logs.length === 0 && !isRunning ? (
                <div className="flex-1 flex items-center justify-center">
                  <div className="text-center">
                    <div className="text-runway-text-secondary text-sm">
                      No output yet
                    </div>
                    <div className="text-runway-text-tertiary text-xs mt-1">
                      Click Run to start the project
                    </div>
                  </div>
                </div>
              ) : (
                <Terminal logs={logs} flush />
              )}
            </>
          )}

          {activeTab === "code" && (
            <>
              {fileLoading ? (
                <div className="flex-1 flex items-center justify-center">
                  <div className="text-runway-text-secondary text-sm">
                    Loading...
                  </div>
                </div>
              ) : fileContent === null ? (
                <div className="flex-1 flex items-center justify-center">
                  <div className="text-center">
                    <div className="text-runway-text-secondary text-sm">
                      No entrypoint file
                    </div>
                    <div className="text-runway-text-tertiary text-xs mt-1">
                      This project has no entrypoint to view
                    </div>
                  </div>
                </div>
              ) : (
                <div className="flex-1 flex flex-col min-h-0">
                  <div className="flex items-center justify-between px-5 py-1.5 border-b border-runway-border-subtle">
                    <span className="text-[11px] text-runway-text-tertiary font-mono">
                      {project.entrypoint}
                    </span>
                    {fileDirty && (
                      <button
                        onClick={handleSaveCode}
                        className="btn btn-primary btn-sm"
                      >
                        Save
                      </button>
                    )}
                  </div>
                  <textarea
                    value={fileContent}
                    onChange={(e) => {
                      setFileContent(e.target.value);
                      setFileDirty(true);
                    }}
                    onKeyDown={(e) => {
                      if ((e.metaKey || e.ctrlKey) && e.key === "s") {
                        e.preventDefault();
                        handleSaveCode();
                      }
                      if (e.key === "Tab") {
                        e.preventDefault();
                        const target = e.target as HTMLTextAreaElement;
                        const start = target.selectionStart;
                        const end = target.selectionEnd;
                        const val = fileContent;
                        setFileContent(
                          val.substring(0, start) + "  " + val.substring(end)
                        );
                        setFileDirty(true);
                        requestAnimationFrame(() => {
                          target.selectionStart = target.selectionEnd = start + 2;
                        });
                      }
                    }}
                    spellCheck={false}
                    className="flex-1 w-full p-4 bg-transparent text-sm font-mono text-runway-text-primary resize-none focus:outline-none leading-relaxed"
                  />
                </div>
              )}
            </>
          )}
        </div>

        {/* ── Side Panel ────────────────────────────────────────── */}
        {activePanel && (
          <div className="side-panel">
            <div className="side-panel-header">
              <span>
                {activePanel === "schedule" && "Schedules"}
                {activePanel === "history" && "History"}
                {activePanel === "settings" && "Settings"}
              </span>
              <button
                onClick={() => setActivePanel(null)}
                className="text-runway-text-tertiary hover:text-runway-text-primary transition-colors"
              >
                <X size={14} />
              </button>
            </div>

            {activePanel === "schedule" && (
              <SchedulePanel projectId={projectId} />
            )}
            {activePanel === "history" && (
              <HistoryPanel
                projectId={projectId}
                refreshKey={historyRefreshKey}
              />
            )}
            {activePanel === "settings" && (
              <SettingsPanel
                project={project}
                notifyEnabled={notifyEnabled}
                onToggleNotify={handleToggleNotify}
                onDelete={handleDelete}
              />
            )}
          </div>
        )}
      </div>
    </div>
  );
}

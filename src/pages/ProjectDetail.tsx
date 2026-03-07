import { useEffect, useState, useCallback } from "react";
import { listen } from "@tauri-apps/api/event";
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
  idle: "bg-runway-muted",
  running: "bg-green-500",
  stopped: "bg-runway-muted",
  failed: "bg-red-500",
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
  if (!dateStr) return "—";
  return new Date(dateStr).toLocaleString(undefined, {
    month: "short", day: "numeric", hour: "2-digit", minute: "2-digit",
  });
}

// --- Schedule Section ---
function ScheduleSection({ projectId }: { projectId: string }) {
  const [schedules, setSchedules] = useState<ScheduleInfo[]>([]);
  const [showAdd, setShowAdd] = useState(false);
  const [cronExpr, setCronExpr] = useState("");
  const [adding, setAdding] = useState(false);
  const { toast } = useToast();

  const refresh = useCallback(() => {
    listSchedules(projectId).then(setSchedules).catch(console.error);
  }, [projectId]);

  useEffect(() => { refresh(); }, [refresh]);

  // Auto-refresh when a schedule executes
  useEffect(() => {
    const unlisten = listen<{ project_id: string; success: boolean; exit_code: number | null }>(
      "schedule-executed",
      (event) => {
        if (event.payload.project_id === projectId) {
          refresh();
          const msg = event.payload.success ? "Schedule ran successfully" : `Schedule failed (exit ${event.payload.exit_code})`;
          toast(msg, event.payload.success ? "success" : "error");
        }
      }
    );
    return () => { unlisten.then((fn) => fn()); };
  }, [projectId, refresh, toast]);

  async function handleAdd() {
    const expr = cronExpr.trim();
    if (!expr) return;
    setAdding(true);
    try {
      await addSchedule(projectId, expr);
      toast("Schedule added", "success");
      setCronExpr("");
      setShowAdd(false);
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

  return (
    <div className="rounded-lg bg-runway-surface border border-runway-border">
      <div className="flex items-center justify-between px-3 py-2 border-b border-runway-border">
        <div className="flex items-center gap-2">
          <svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" className="text-runway-muted">
            <circle cx="12" cy="12" r="10" /><polyline points="12 6 12 12 16 14" />
          </svg>
          <span className="text-xs font-medium">Schedules</span>
          {schedules.length > 0 && (
            <span className="text-[10px] text-runway-muted">{schedules.length}</span>
          )}
        </div>
        <button onClick={() => setShowAdd(!showAdd)}
          className="text-xs text-runway-accent hover:underline">
          {showAdd ? "Cancel" : "+ Add"}
        </button>
      </div>

      {showAdd && (
        <div className="px-3 py-2 border-b border-runway-border flex gap-2 items-end">
          <div className="flex-1">
            <label className="block text-[10px] text-runway-muted mb-0.5">Cron Expression</label>
            <input type="text" value={cronExpr} onChange={(e) => setCronExpr(e.target.value)}
              placeholder="@every 5m, @hourly, 30 9 * * *"
              onKeyDown={(e) => { if (e.key === "Enter") handleAdd(); }}
              className="w-full px-2 py-1.5 rounded bg-runway-bg border border-runway-border text-xs text-runway-text placeholder-runway-muted focus:outline-none focus:border-runway-accent transition-colors" />
          </div>
          <button onClick={handleAdd} disabled={adding || !cronExpr.trim()}
            className="px-3 py-1.5 rounded bg-runway-accent text-white text-xs font-medium disabled:opacity-50">
            {adding ? "..." : "Add"}
          </button>
        </div>
      )}

      {schedules.length === 0 && !showAdd ? (
        <div className="px-3 py-3 text-center text-xs text-runway-muted">
          No schedules. Add one to run this project on a timer.
        </div>
      ) : (
        <div>
          {schedules.map((s) => (
            <div key={s.id} className="flex items-center gap-2 px-3 py-2 border-b border-runway-border last:border-b-0">
              <button onClick={() => handleToggle(s.id, !s.enabled)} title={s.enabled ? "Disable" : "Enable"}
                className={`w-3.5 h-3.5 rounded-full border-2 shrink-0 transition-colors ${
                  s.enabled ? "bg-runway-success border-runway-success" : "bg-transparent border-runway-muted"
                }`} />
              <div className="flex-1 min-w-0">
                <span className={`text-xs font-mono ${s.enabled ? "text-runway-text" : "text-runway-muted"}`}>
                  {s.cron_expr}
                </span>
                <div className="flex gap-3 text-[10px] text-runway-muted">
                  {s.next_run_at && <span>Next: {formatDate(s.next_run_at)}</span>}
                  {s.last_triggered_at && <span>Last: {formatDate(s.last_triggered_at)}</span>}
                </div>
              </div>
              <button onClick={() => handleRemove(s.id)}
                className="p-1 rounded hover:bg-red-500/10 text-runway-muted hover:text-red-400 transition-colors shrink-0">
                <svg width="12" height="12" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round">
                  <line x1="18" y1="6" x2="6" y2="18" /><line x1="6" y1="6" x2="18" y2="18" />
                </svg>
              </button>
            </div>
          ))}
        </div>
      )}
    </div>
  );
}

// --- Execution History Section ---
function ExecutionHistory({ projectId, refreshKey }: { projectId: string; refreshKey: number }) {
  const [logs, setLogs] = useState<ExecutionLogInfo[]>([]);
  const [expandedId, setExpandedId] = useState<string | null>(null);

  useEffect(() => {
    listExecutionLogs(projectId, 20).then(setLogs).catch(console.error);
  }, [projectId, refreshKey]);

  if (logs.length === 0) return null;

  return (
    <div className="rounded-lg bg-runway-surface border border-runway-border">
      <div className="flex items-center gap-2 px-3 py-2 border-b border-runway-border">
        <svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" className="text-runway-muted">
          <path d="M12 8v4l3 3" /><circle cx="12" cy="12" r="10" />
        </svg>
        <span className="text-xs font-medium">Execution History</span>
        <span className="text-[10px] text-runway-muted">{logs.length}</span>
      </div>
      <div className="max-h-48 overflow-y-auto">
        {logs.map((log) => {
          const isExpanded = expandedId === log.id;
          const isSuccess = log.exit_code === 0;
          const isRunning = log.finished_at === null;
          return (
            <div key={log.id} className="border-b border-runway-border last:border-b-0">
              <button
                onClick={() => setExpandedId(isExpanded ? null : log.id)}
                className="w-full flex items-center gap-2 px-3 py-1.5 text-left hover:bg-runway-bg/50 transition-colors"
              >
                <div className={`w-2 h-2 rounded-full shrink-0 ${
                  isRunning ? "bg-runway-accent animate-pulse-soft" :
                  isSuccess ? "bg-runway-success" : "bg-runway-error"
                }`} />
                <span className="text-xs text-runway-text flex-1 truncate">
                  {formatDate(log.started_at)}
                </span>
                <span className={`text-[10px] px-1.5 py-0.5 rounded ${
                  log.trigger === "schedule"
                    ? "bg-purple-500/10 text-purple-400"
                    : "bg-runway-accent/10 text-runway-accent"
                }`}>
                  {log.trigger}
                </span>
                {!isRunning && (
                  <span className={`text-[10px] font-mono ${isSuccess ? "text-runway-success" : "text-runway-error"}`}>
                    exit {log.exit_code}
                  </span>
                )}
                {isRunning && (
                  <span className="text-[10px] text-runway-accent">running</span>
                )}
                <svg
                  width="10" height="10" viewBox="0 0 24 24" fill="none" stroke="currentColor"
                  strokeWidth="2" strokeLinecap="round"
                  className={`text-runway-muted shrink-0 transition-transform ${isExpanded ? "rotate-90" : ""}`}
                >
                  <polyline points="9 18 15 12 9 6" />
                </svg>
              </button>
              {isExpanded && log.output && (
                <div className="px-3 pb-2">
                  <pre className="text-[11px] font-mono text-runway-muted bg-runway-bg rounded p-2 max-h-32 overflow-y-auto whitespace-pre-wrap break-all">
                    {log.output || "(no output)"}
                  </pre>
                </div>
              )}
            </div>
          );
        })}
      </div>
    </div>
  );
}

// --- Main Component ---
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
    return () => { unlisten.then((fn) => fn()); };
  }, [projectId]);

  useEffect(() => {
    const unlisten = listen<StatusEvent>("project-status-change", (event) => {
      if (event.payload.project_id === projectId) {
        setStatus(event.payload.status);
        if (event.payload.status === "running") {
          setStartedAt(Date.now());
        } else {
          setStartedAt(null);
          setHistoryRefreshKey((k) => k + 1);
          if (event.payload.status === "failed") {
            toast(`Process exited with code ${event.payload.exit_code ?? "unknown"}`, "error");
          } else if (event.payload.status === "idle") {
            toast("Process completed successfully", "success");
          }
        }
      }
    });
    return () => { unlisten.then((fn) => fn()); };
  }, [projectId, toast]);

  useEffect(() => {
    if (!startedAt) { setUptime(""); return; }
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
    setError(null); setLogs([]);
    try {
      await runProject(projectId, selectedTarget || undefined);
      if (selectedTarget) {
        const t = targets.find((t) => t.id === selectedTarget);
        toast(`Started on ${t?.name ?? "remote"}`, "success");
      } else {
        toast("Project started", "success");
      }
    } catch (e) {
      const msg = String(e); setError(msg); toast(msg, "error");
    }
  }, [projectId, selectedTarget, targets, toast]);

  const handleStop = useCallback(async () => {
    setError(null);
    try {
      await stopProject(projectId, selectedTarget || undefined);
      toast("Project stopped", "info");
    } catch (e) {
      const msg = String(e); setError(msg); toast(msg, "error");
    }
  }, [projectId, selectedTarget, toast]);

  const handleDelete = useCallback(async () => {
    try {
      await deleteProject(projectId);
      toast("Project deleted", "info");
      onBack();
    } catch (e) { toast(String(e), "error"); }
  }, [projectId, onBack, toast]);

  const isRunning = status === "running";

  if (!project) {
    return (
      <div className="h-full flex flex-col">
        <div className="flex items-center gap-3 px-4 py-3 border-b border-runway-border">
          <button onClick={onBack} className="text-runway-accent text-sm hover:underline">&larr; Back</button>
          <div className="skeleton h-4 w-32" />
        </div>
        <div className="p-4 flex flex-col gap-3">
          <div className="skeleton h-10 w-full" />
          <div className="skeleton h-8 w-48" />
          <div className="skeleton h-64 w-full" />
        </div>
      </div>
    );
  }

  return (
    <div className="h-full flex flex-col">
      <div className="flex items-center gap-3 px-4 py-3 border-b border-runway-border">
        <button onClick={onBack} className="text-runway-accent text-sm hover:underline">&larr; Back</button>
        <h1 className="text-sm font-semibold">{project.name}</h1>
        <button onClick={handleDelete} className="ml-auto text-xs text-runway-muted hover:text-runway-error transition-colors">Delete</button>
      </div>

      <div className="flex-1 flex flex-col gap-3 p-4 overflow-hidden">
        {/* Status bar */}
        <div className="flex items-center gap-2 px-3 py-2 rounded-lg bg-runway-surface">
          <div className={`w-2.5 h-2.5 rounded-full ${STATUS_COLORS[status] ?? STATUS_COLORS.idle} ${isRunning ? "animate-pulse-soft" : ""}`} />
          <span className="text-sm font-medium capitalize">{status}</span>
          {uptime && <span className="text-xs text-runway-muted font-mono">{uptime}</span>}
          <div className="ml-auto flex items-center gap-3 text-xs text-runway-muted">
            <span>{project.runtime}</span>
            <span>{project.entrypoint ?? "no entrypoint"}</span>
          </div>
        </div>

        {/* Monitoring stats */}
        <div className="flex gap-3">
          <div className="flex-1 px-3 py-2 rounded-lg bg-runway-surface text-center">
            <div className="text-lg font-semibold tabular-nums">{project.run_count}</div>
            <div className="text-xs text-runway-muted">Runs</div>
          </div>
          <div className="flex-1 px-3 py-2 rounded-lg bg-runway-surface text-center">
            <div className="text-lg font-semibold">{timeAgo(project.last_run_at)}</div>
            <div className="text-xs text-runway-muted">Last Run</div>
          </div>
          <div className="flex-1 px-3 py-2 rounded-lg bg-runway-surface text-center">
            <div className="text-lg font-semibold tabular-nums">{project.last_exit_code !== null ? project.last_exit_code : "--"}</div>
            <div className="text-xs text-runway-muted">Exit Code</div>
          </div>
        </div>

        {/* Notification toggle */}
        <div className="flex items-center justify-between px-3 py-1.5 rounded-lg bg-runway-surface">
          <div className="flex items-center gap-2">
            <svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" className="text-runway-muted">
              <path d="M18 8A6 6 0 0 0 6 8c0 7-3 9-3 9h18s-3-2-3-9" /><path d="M13.73 21a2 2 0 0 1-3.46 0" />
            </svg>
            <span className="text-xs text-runway-text">Notifications</span>
          </div>
          <button
            onClick={handleToggleNotify}
            className={`relative w-8 h-5 rounded-full transition-colors ${
              notifyEnabled ? "bg-runway-accent" : "bg-runway-border"
            }`}
          >
            <div className={`absolute top-0.5 w-4 h-4 rounded-full bg-white transition-transform ${
              notifyEnabled ? "translate-x-3.5" : "translate-x-0.5"
            }`} />
          </button>
        </div>

        {/* Target selector */}
        {targets.length > 0 && (
          <div className="flex items-center gap-2">
            <span className="text-xs text-runway-muted">Target:</span>
            <div className="flex gap-1.5">
              <button onClick={() => { setSelectedTarget(null); setProjectTarget(projectId, null).catch(console.error); }}
                className={`flex items-center gap-1.5 px-2.5 py-1 rounded-md text-xs font-medium transition-colors ${
                  selectedTarget === null ? "bg-runway-accent text-white" : "bg-runway-surface text-runway-muted border border-runway-border hover:border-runway-accent/30"
                }`}>
                <div className="w-1.5 h-1.5 rounded-full bg-runway-success" />local
              </button>
              {targets.map((t) => (
                <button key={t.id} onClick={() => { setSelectedTarget(t.id); setProjectTarget(projectId, t.id).catch(console.error); }}
                  className={`flex items-center gap-1.5 px-2.5 py-1 rounded-md text-xs font-medium transition-colors ${
                    selectedTarget === t.id ? "bg-runway-accent text-white" : "bg-runway-surface text-runway-muted border border-runway-border hover:border-runway-accent/30"
                  }`}>
                  <div className={`w-1.5 h-1.5 rounded-full ${t.status === "online" ? "bg-runway-success" : t.status === "offline" ? "bg-runway-error" : "bg-runway-muted"}`} />
                  {t.name}
                </button>
              ))}
            </div>
          </div>
        )}

        {error && (
          <div className="px-3 py-2 rounded-lg bg-red-500/10 border border-red-500/30 text-sm text-red-400">{error}</div>
        )}

        {/* Actions */}
        <div className="flex gap-2">
          <button onClick={handleRun} disabled={isRunning}
            className="flex items-center gap-1.5 px-4 py-2 rounded-lg bg-runway-accent text-white text-sm font-medium hover:opacity-90 transition-opacity disabled:opacity-50 disabled:cursor-not-allowed">
            <svg width="14" height="14" viewBox="0 0 24 24" fill="currentColor"><path d="M8 5v14l11-7z" /></svg>Run
          </button>
          <button onClick={handleStop} disabled={!isRunning}
            className="flex items-center gap-1.5 px-4 py-2 rounded-lg bg-runway-surface text-runway-text text-sm border border-runway-border hover:bg-runway-border transition-colors disabled:opacity-50 disabled:cursor-not-allowed">
            <svg width="14" height="14" viewBox="0 0 24 24" fill="currentColor"><rect x="6" y="6" width="12" height="12" rx="1" /></svg>Stop
          </button>
        </div>

        {/* Schedules */}
        <ScheduleSection projectId={projectId} />

        {/* Execution History */}
        <ExecutionHistory projectId={projectId} refreshKey={historyRefreshKey} />

        {/* Logs / Code tabs */}
        <div className="flex items-center gap-1 border-b border-runway-border">
          <button
            onClick={() => setActiveTab("logs")}
            className={`px-3 py-1.5 text-xs font-medium border-b-2 transition-colors ${
              activeTab === "logs"
                ? "border-runway-accent text-runway-accent"
                : "border-transparent text-runway-muted hover:text-runway-text"
            }`}
          >
            Logs
          </button>
          <button
            onClick={() => setActiveTab("code")}
            className={`px-3 py-1.5 text-xs font-medium border-b-2 transition-colors ${
              activeTab === "code"
                ? "border-runway-accent text-runway-accent"
                : "border-transparent text-runway-muted hover:text-runway-text"
            }`}
          >
            Code{fileDirty ? " *" : ""}
          </button>
        </div>

        {activeTab === "logs" && (
          <>
            {logs.length === 0 && !isRunning ? (
              <div className="flex-1 flex items-center justify-center rounded-lg bg-runway-surface border border-runway-border">
                <div className="text-center">
                  <div className="text-runway-muted text-sm">No logs yet</div>
                  <div className="text-runway-muted/60 text-xs mt-1">Click Run to start the project</div>
                </div>
              </div>
            ) : (
              <Terminal logs={logs} />
            )}
          </>
        )}

        {activeTab === "code" && (
          <div className="flex-1 flex flex-col min-h-0 rounded-lg bg-runway-surface border border-runway-border overflow-hidden">
            {fileLoading ? (
              <div className="flex-1 flex items-center justify-center">
                <div className="text-runway-muted text-sm">Loading...</div>
              </div>
            ) : fileContent === null ? (
              <div className="flex-1 flex items-center justify-center">
                <div className="text-center">
                  <div className="text-runway-muted text-sm">No entrypoint file</div>
                  <div className="text-runway-muted/60 text-xs mt-1">This project has no entrypoint to view</div>
                </div>
              </div>
            ) : (
              <>
                <div className="flex items-center justify-between px-3 py-1.5 border-b border-runway-border bg-runway-bg/50">
                  <span className="text-[11px] text-runway-muted font-mono">
                    {project.entrypoint}
                  </span>
                  <button
                    onClick={handleSaveCode}
                    disabled={!fileDirty}
                    className="px-2.5 py-1 rounded text-[11px] font-medium bg-runway-accent text-white disabled:opacity-30 hover:opacity-90 transition-opacity"
                  >
                    Save
                  </button>
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
                      setFileContent(val.substring(0, start) + "  " + val.substring(end));
                      setFileDirty(true);
                      requestAnimationFrame(() => {
                        target.selectionStart = target.selectionEnd = start + 2;
                      });
                    }
                  }}
                  spellCheck={false}
                  className="flex-1 w-full p-3 bg-transparent text-sm font-mono text-runway-text resize-none focus:outline-none leading-relaxed"
                />
              </>
            )}
          </div>
        )}
      </div>
    </div>
  );
}

import { useEffect, useState, useCallback } from "react";
import { listen } from "@tauri-apps/api/event";
import {
  runProject,
  stopProject,
  deleteProject,
  listProjects,
  listTargets,
  runProjectRemote,
  stopProjectRemote,
  type Project,
  type LogEvent,
  type StatusEvent,
  type TargetInfo,
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

export default function ProjectDetail({ projectId, onBack }: Props) {
  const [project, setProject] = useState<Project | null>(null);
  const [status, setStatus] = useState<string>("idle");
  const [logs, setLogs] = useState<LogEvent[]>([]);
  const [error, setError] = useState<string | null>(null);
  const [startedAt, setStartedAt] = useState<number | null>(null);
  const [uptime, setUptime] = useState("");
  const [targets, setTargets] = useState<TargetInfo[]>([]);
  const [selectedTarget, setSelectedTarget] = useState<string | null>(null);
  const { toast } = useToast();

  useEffect(() => {
    listProjects().then((projects) => {
      const p = projects.find((p) => p.id === projectId);
      if (p) {
        setProject(p);
        setStatus(p.status);
        if (p.status === "running") {
          setStartedAt(Date.now());
        }
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
    const unlisten = listen<StatusEvent>("project-status-change", (event) => {
      if (event.payload.project_id === projectId) {
        setStatus(event.payload.status);
        if (event.payload.status === "running") {
          setStartedAt(Date.now());
        } else {
          setStartedAt(null);
          if (event.payload.status === "failed") {
            const code = event.payload.exit_code;
            toast(
              `Process exited with code ${code ?? "unknown"}`,
              "error"
            );
          } else if (event.payload.status === "idle") {
            toast("Process completed successfully", "success");
          }
        }
      }
    });
    return () => {
      unlisten.then((fn) => fn());
    };
  }, [projectId, toast]);

  // Uptime ticker
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

  const handleRun = useCallback(async () => {
    setError(null);
    setLogs([]);
    try {
      if (selectedTarget) {
        await runProjectRemote(projectId, selectedTarget);
        const t = targets.find((t) => t.id === selectedTarget);
        toast(`Started on ${t?.name ?? "remote"}`, "success");
      } else {
        await runProject(projectId);
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
      if (selectedTarget) {
        await stopProjectRemote(projectId, selectedTarget);
      } else {
        await stopProject(projectId);
      }
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

  const isRunning = status === "running";

  if (!project) {
    return (
      <div className="h-full flex flex-col">
        <div className="flex items-center gap-3 px-4 py-3 border-b border-runway-border">
          <button
            onClick={onBack}
            className="text-runway-accent text-sm hover:underline"
          >
            &larr; Back
          </button>
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
      {/* Header */}
      <div className="flex items-center gap-3 px-4 py-3 border-b border-runway-border">
        <button
          onClick={onBack}
          className="text-runway-accent text-sm hover:underline"
        >
          &larr; Back
        </button>
        <h1 className="text-sm font-semibold">{project.name}</h1>
        <button
          onClick={handleDelete}
          className="ml-auto text-xs text-runway-muted hover:text-runway-error transition-colors"
        >
          Delete
        </button>
      </div>

      <div className="flex-1 flex flex-col gap-3 p-4 overflow-hidden">
        {/* Status bar */}
        <div className="flex items-center gap-2 px-3 py-2 rounded-lg bg-runway-surface">
          <div
            className={`w-2.5 h-2.5 rounded-full ${STATUS_COLORS[status] ?? STATUS_COLORS.idle} ${isRunning ? "animate-pulse-soft" : ""}`}
          />
          <span className="text-sm font-medium capitalize">{status}</span>
          {uptime && (
            <span className="text-xs text-runway-muted font-mono">
              {uptime}
            </span>
          )}
          <div className="ml-auto flex items-center gap-3 text-xs text-runway-muted">
            <span>{project.runtime}</span>
            <span>{project.entrypoint ?? "no entrypoint"}</span>
          </div>
        </div>

        {/* Monitoring stats */}
        <div className="flex gap-3">
          <div className="flex-1 px-3 py-2 rounded-lg bg-runway-surface text-center">
            <div className="text-lg font-semibold tabular-nums">
              {project.run_count}
            </div>
            <div className="text-xs text-runway-muted">Runs</div>
          </div>
          <div className="flex-1 px-3 py-2 rounded-lg bg-runway-surface text-center">
            <div className="text-lg font-semibold">
              {timeAgo(project.last_run_at)}
            </div>
            <div className="text-xs text-runway-muted">Last Run</div>
          </div>
          <div className="flex-1 px-3 py-2 rounded-lg bg-runway-surface text-center">
            <div className="text-lg font-semibold tabular-nums">
              {project.last_exit_code !== null
                ? project.last_exit_code
                : "--"}
            </div>
            <div className="text-xs text-runway-muted">Exit Code</div>
          </div>
        </div>

        {/* Target selector */}
        {targets.length > 0 && (
          <div className="flex items-center gap-2">
            <span className="text-xs text-runway-muted">Target:</span>
            <div className="flex gap-1.5">
              <button
                onClick={() => setSelectedTarget(null)}
                className={`flex items-center gap-1.5 px-2.5 py-1 rounded-md text-xs font-medium transition-colors ${
                  selectedTarget === null
                    ? "bg-runway-accent text-white"
                    : "bg-runway-surface text-runway-muted border border-runway-border hover:border-runway-accent/30"
                }`}
              >
                <div className="w-1.5 h-1.5 rounded-full bg-runway-success" />
                local
              </button>
              {targets.map((t) => (
                <button
                  key={t.id}
                  onClick={() => setSelectedTarget(t.id)}
                  className={`flex items-center gap-1.5 px-2.5 py-1 rounded-md text-xs font-medium transition-colors ${
                    selectedTarget === t.id
                      ? "bg-runway-accent text-white"
                      : "bg-runway-surface text-runway-muted border border-runway-border hover:border-runway-accent/30"
                  }`}
                >
                  <div
                    className={`w-1.5 h-1.5 rounded-full ${
                      t.status === "online"
                        ? "bg-runway-success"
                        : t.status === "offline"
                        ? "bg-runway-error"
                        : "bg-runway-muted"
                    }`}
                  />
                  {t.name}
                </button>
              ))}
            </div>
          </div>
        )}

        {/* Error display */}
        {error && (
          <div className="px-3 py-2 rounded-lg bg-red-500/10 border border-red-500/30 text-sm text-red-400">
            {error}
          </div>
        )}

        {/* Actions */}
        <div className="flex gap-2">
          <button
            onClick={handleRun}
            disabled={isRunning}
            className="flex items-center gap-1.5 px-4 py-2 rounded-lg bg-runway-accent text-white text-sm font-medium hover:opacity-90 transition-opacity disabled:opacity-50 disabled:cursor-not-allowed"
          >
            <svg
              width="14"
              height="14"
              viewBox="0 0 24 24"
              fill="currentColor"
            >
              <path d="M8 5v14l11-7z" />
            </svg>
            Run
          </button>
          <button
            onClick={handleStop}
            disabled={!isRunning}
            className="flex items-center gap-1.5 px-4 py-2 rounded-lg bg-runway-surface text-runway-text text-sm border border-runway-border hover:bg-runway-border transition-colors disabled:opacity-50 disabled:cursor-not-allowed"
          >
            <svg
              width="14"
              height="14"
              viewBox="0 0 24 24"
              fill="currentColor"
            >
              <rect x="6" y="6" width="12" height="12" rx="1" />
            </svg>
            Stop
          </button>
        </div>

        {/* xterm.js log viewer */}
        {logs.length === 0 && !isRunning ? (
          <div className="flex-1 flex items-center justify-center rounded-lg bg-runway-surface border border-runway-border">
            <div className="text-center">
              <div className="text-runway-muted text-sm">No logs yet</div>
              <div className="text-runway-muted/60 text-xs mt-1">
                Click Run to start the project
              </div>
            </div>
          </div>
        ) : (
          <Terminal logs={logs} />
        )}
      </div>
    </div>
  );
}

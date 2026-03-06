import { useEffect, useState, useRef, useCallback } from "react";
import { listen } from "@tauri-apps/api/event";
import {
  runProject,
  stopProject,
  listProjects,
  type Project,
  type LogEvent,
  type StatusEvent,
} from "../lib/invoke";

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

export default function ProjectDetail({ projectId, onBack }: Props) {
  const [project, setProject] = useState<Project | null>(null);
  const [status, setStatus] = useState<string>("idle");
  const [logs, setLogs] = useState<LogEvent[]>([]);
  const [error, setError] = useState<string | null>(null);
  const logEndRef = useRef<HTMLDivElement>(null);

  useEffect(() => {
    listProjects().then((projects) => {
      const p = projects.find((p) => p.id === projectId);
      if (p) {
        setProject(p);
        setStatus(p.status);
      }
    });
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
      }
    });
    return () => {
      unlisten.then((fn) => fn());
    };
  }, [projectId]);

  useEffect(() => {
    logEndRef.current?.scrollIntoView({ behavior: "smooth" });
  }, [logs]);

  const handleRun = useCallback(async () => {
    setError(null);
    setLogs([]);
    try {
      await runProject(projectId);
    } catch (e) {
      setError(String(e));
    }
  }, [projectId]);

  const handleStop = useCallback(async () => {
    setError(null);
    try {
      await stopProject(projectId);
    } catch (e) {
      setError(String(e));
    }
  }, [projectId]);

  const isRunning = status === "running";

  return (
    <div className="h-full flex flex-col">
      <div className="flex items-center gap-3 px-4 py-3 border-b border-runway-border">
        <button
          onClick={onBack}
          className="text-runway-accent text-sm hover:underline"
        >
          &larr; Back
        </button>
        <h1 className="text-sm font-semibold">
          {project?.name ?? "Project"}
        </h1>
      </div>

      <div className="flex-1 flex flex-col gap-4 p-4">
        {/* Status bar */}
        <div className="flex items-center gap-2 px-3 py-2 rounded-lg bg-runway-surface">
          <div
            className={`w-2 h-2 rounded-full ${STATUS_COLORS[status] ?? STATUS_COLORS.idle}`}
          />
          <span className="text-sm capitalize">{status}</span>
          <span className="text-xs text-runway-muted ml-auto">
            {project?.runtime} &middot;{" "}
            {project?.entrypoint ?? "no entrypoint"}
          </span>
        </div>

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
            className="px-4 py-2 rounded-lg bg-runway-accent text-white text-sm font-medium hover:opacity-90 transition-opacity disabled:opacity-50 disabled:cursor-not-allowed"
          >
            Run
          </button>
          <button
            onClick={handleStop}
            disabled={!isRunning}
            className="px-4 py-2 rounded-lg bg-runway-surface text-runway-text text-sm border border-runway-border hover:bg-runway-border transition-colors disabled:opacity-50 disabled:cursor-not-allowed"
          >
            Stop
          </button>
        </div>

        {/* Log viewer */}
        <div className="flex-1 rounded-lg bg-runway-surface border border-runway-border p-3 font-mono text-xs overflow-y-auto">
          {logs.length === 0 ? (
            <div className="text-runway-muted">
              Logs will appear here when the project is running.
            </div>
          ) : (
            logs.map((line, i) => (
              <div
                key={i}
                className={
                  line.stream === "stderr"
                    ? "text-red-400"
                    : "text-runway-text"
                }
              >
                {line.text}
              </div>
            ))
          )}
          <div ref={logEndRef} />
        </div>
      </div>
    </div>
  );
}

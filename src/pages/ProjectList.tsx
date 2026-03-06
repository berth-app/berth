import { useEffect, useState } from "react";
import { listen } from "@tauri-apps/api/event";
import { listProjects, type Project, type StatusEvent } from "../lib/invoke";

interface Props {
  onSelect: (id: string) => void;
  onNewProject: () => void;
  onTargets?: () => void;
}

const STATUS_COLORS: Record<string, string> = {
  idle: "bg-runway-muted",
  running: "bg-runway-success",
  stopped: "bg-runway-border",
  failed: "bg-runway-error",
};

const STATUS_LABELS: Record<string, string> = {
  idle: "Idle",
  running: "Running",
  stopped: "Stopped",
  failed: "Failed",
};

const RUNTIME_ICONS: Record<string, string> = {
  python: "Py",
  node: "JS",
  go: "Go",
  rust: "Rs",
  shell: "Sh",
  unknown: "?",
};

function timeAgo(dateStr: string): string {
  const diff = Date.now() - new Date(dateStr).getTime();
  if (diff < 60000) return "just now";
  if (diff < 3600000) return `${Math.floor(diff / 60000)}m ago`;
  if (diff < 86400000) return `${Math.floor(diff / 3600000)}h ago`;
  return `${Math.floor(diff / 86400000)}d ago`;
}

function LoadingSkeleton() {
  return (
    <div className="h-full flex flex-col">
      <div className="flex items-center justify-between px-4 py-3 border-b border-runway-border">
        <div className="skeleton h-4 w-16" />
        <div className="skeleton h-7 w-14 rounded-md" />
      </div>
      <div className="flex-1 overflow-y-auto">
        {[1, 2, 3].map((i) => (
          <div
            key={i}
            className="flex items-center gap-3 px-4 py-3 border-b border-runway-border"
          >
            <div className="skeleton w-8 h-8 rounded-lg shrink-0" />
            <div className="flex-1 flex flex-col gap-1.5">
              <div className="skeleton h-3.5 w-32" />
              <div className="skeleton h-3 w-48" />
            </div>
            <div className="skeleton h-5 w-14 rounded-full" />
          </div>
        ))}
      </div>
    </div>
  );
}

export default function ProjectList({ onSelect, onNewProject, onTargets }: Props) {
  const [projects, setProjects] = useState<Project[]>([]);
  const [loading, setLoading] = useState(true);

  useEffect(() => {
    listProjects()
      .then(setProjects)
      .catch(console.error)
      .finally(() => setLoading(false));
  }, []);

  // Live status updates
  useEffect(() => {
    const unlisten = listen<StatusEvent>("project-status-change", (event) => {
      setProjects((prev) =>
        prev.map((p) =>
          p.id === event.payload.project_id
            ? { ...p, status: event.payload.status }
            : p
        )
      );
    });
    return () => {
      unlisten.then((fn) => fn());
    };
  }, []);

  if (loading) return <LoadingSkeleton />;

  if (projects.length === 0) {
    return (
      <div className="flex flex-col items-center justify-center h-full gap-5 px-6">
        <div className="w-16 h-16 rounded-2xl bg-runway-surface border border-runway-border flex items-center justify-center">
          <svg
            width="28"
            height="28"
            viewBox="0 0 24 24"
            fill="none"
            stroke="currentColor"
            strokeWidth="1.5"
            className="text-runway-muted"
          >
            <path d="M12 5v14M5 12h14" strokeLinecap="round" />
          </svg>
        </div>
        <div className="text-center">
          <div className="text-base font-semibold text-runway-text mb-1">
            No projects yet
          </div>
          <p className="text-sm text-runway-muted max-w-[260px]">
            Paste code from Claude Code, Cursor, or any AI tool and deploy it
            instantly.
          </p>
        </div>
        <button
          onClick={onNewProject}
          className="px-5 py-2.5 rounded-lg bg-runway-accent text-white text-sm font-medium hover:opacity-90 transition-opacity"
        >
          Paste &amp; Deploy
        </button>
      </div>
    );
  }

  return (
    <div className="h-full flex flex-col">
      <div className="flex items-center justify-between px-4 py-3 border-b border-runway-border">
        <h1 className="text-sm font-semibold">
          Projects
          <span className="ml-1.5 text-xs font-normal text-runway-muted">
            {projects.length}
          </span>
        </h1>
        <div className="flex gap-2">
          {onTargets && (
            <button
              onClick={onTargets}
              className="px-3 py-1.5 rounded-md border border-runway-border text-runway-muted text-xs font-medium hover:text-runway-text hover:border-runway-accent transition-colors"
            >
              Targets
            </button>
          )}
          <button
            onClick={onNewProject}
            className="px-3 py-1.5 rounded-md bg-runway-accent text-white text-xs font-medium hover:opacity-90 transition-opacity"
          >
            + New
          </button>
        </div>
      </div>
      <div className="flex-1 overflow-y-auto">
        {projects.map((project) => (
          <button
            key={project.id}
            onClick={() => onSelect(project.id)}
            className="w-full flex items-center gap-3 px-4 py-3 border-b border-runway-border hover:bg-runway-surface/50 transition-colors text-left group"
          >
            {/* Runtime badge */}
            <div className="w-8 h-8 rounded-lg bg-runway-surface border border-runway-border flex items-center justify-center text-xs font-bold text-runway-muted shrink-0 group-hover:border-runway-accent/30 transition-colors">
              {RUNTIME_ICONS[project.runtime] ?? "?"}
            </div>

            {/* Info */}
            <div className="flex-1 min-w-0">
              <div className="flex items-center gap-2">
                <span className="text-sm font-medium truncate">
                  {project.name}
                </span>
                {project.run_count > 0 && (
                  <span className="text-[10px] text-runway-muted">
                    {project.run_count} run{project.run_count !== 1 ? "s" : ""}
                  </span>
                )}
              </div>
              <div className="text-xs text-runway-muted truncate">
                {project.entrypoint ?? project.path}
              </div>
            </div>

            {/* Status + time */}
            <div className="flex flex-col items-end gap-0.5 shrink-0">
              <div className="flex items-center gap-1.5">
                <div
                  className={`w-1.5 h-1.5 rounded-full ${STATUS_COLORS[project.status] ?? STATUS_COLORS.idle} ${project.status === "running" ? "animate-pulse-soft" : ""}`}
                />
                <span className="text-xs text-runway-muted">
                  {STATUS_LABELS[project.status] ?? "Idle"}
                </span>
              </div>
              <span className="text-[10px] text-runway-muted/60">
                {timeAgo(project.updated_at)}
              </span>
            </div>
          </button>
        ))}
      </div>
    </div>
  );
}

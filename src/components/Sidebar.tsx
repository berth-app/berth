import { useEffect, useState, useCallback } from "react";
import { LayoutGrid, Server, Settings, Plus, Rocket } from "lucide-react";
import { listProjects, type Project } from "../lib/invoke";
import { listen } from "@tauri-apps/api/event";
import type { StatusEvent } from "../lib/invoke";

type View = "list" | "detail" | "paste" | "targets" | "settings";

interface Props {
  view: View;
  setView: (v: View) => void;
  selectedProjectId: string | null;
  onSelectProject: (id: string) => void;
  onNewProject: () => void;
}

const STATUS_DOT: Record<string, string> = {
  running: "bg-berth-success",
  failed: "bg-berth-error",
  idle: "bg-berth-text-tertiary",
  stopped: "bg-berth-text-tertiary",
};

export default function Sidebar({
  view,
  setView,
  selectedProjectId,
  onSelectProject,
  onNewProject,
}: Props) {
  const [projects, setProjects] = useState<Project[]>([]);

  const refresh = useCallback(() => {
    listProjects().then(setProjects).catch(console.error);
  }, []);

  useEffect(() => {
    refresh();
  }, [refresh]);

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

  // Re-fetch when navigating back to list (project may have been created/deleted)
  useEffect(() => {
    if (view === "list") refresh();
  }, [view, refresh]);

  const sorted = [...projects].sort((a, b) => {
    if (a.status === "running" && b.status !== "running") return -1;
    if (b.status === "running" && a.status !== "running") return 1;
    return 0;
  });

  return (
    <aside className="sidebar">
      <div data-tauri-drag-region className="sidebar-brand">
        <Rocket size={16} strokeWidth={1.75} className="text-berth-accent" />
        <span>Berth</span>
      </div>

      <nav className="sidebar-nav">
        <button
          className="sidebar-nav-item"
          data-active={view === "list" || view === "detail" || view === "paste"}
          onClick={() => setView("list")}
        >
          <LayoutGrid size={16} strokeWidth={1.75} />
          Projects
        </button>
        <button
          className="sidebar-nav-item"
          data-active={view === "targets"}
          onClick={() => setView("targets")}
        >
          <Server size={16} strokeWidth={1.75} />
          Targets
        </button>
        <button
          className="sidebar-nav-item"
          data-active={view === "settings"}
          onClick={() => setView("settings")}
        >
          <Settings size={16} strokeWidth={1.75} />
          Settings
        </button>
      </nav>

      <div className="sidebar-divider" />

      <div className="sidebar-section">
        <div className="sidebar-section-header">
          <span>Projects</span>
          <button
            onClick={onNewProject}
            className="btn btn-ghost btn-icon"
            style={{ padding: 3 }}
          >
            <Plus size={14} strokeWidth={2} className="text-berth-accent" />
          </button>
        </div>

        <div className="sidebar-project-list">
          {sorted.map((p) => (
            <button
              key={p.id}
              className="sidebar-project-item"
              data-active={selectedProjectId === p.id && view === "detail"}
              onClick={() => onSelectProject(p.id)}
            >
              <div
                className={`w-[6px] h-[6px] rounded-full shrink-0 ${
                  STATUS_DOT[p.status] ?? STATUS_DOT.idle
                } ${p.status === "running" ? "animate-pulse-soft" : ""}`}
              />
              <span className="truncate">{p.name}</span>
            </button>
          ))}
          {projects.length === 0 && (
            <div className="px-3 py-4 text-[11px] text-berth-text-tertiary text-center">
              No projects yet
            </div>
          )}
        </div>
      </div>
    </aside>
  );
}

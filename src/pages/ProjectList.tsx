import { useEffect, useState, useCallback, useRef } from "react";
import { listen } from "@tauri-apps/api/event";
import { getCurrentWindow } from "@tauri-apps/api/window";
import {
  Play,
  Square,
  Pencil,
  Trash2,
  Download,
  Plus,
  Search,
  RefreshCw,
} from "lucide-react";
import {
  listProjects,
  deleteProject,
  updateProject,
  runProject,
  stopProject,
  detectRuntime,
  importFile,
  listTargets,
  type Project,
  type StatusEvent,
  type RuntimeInfo,
  type TargetInfo,
} from "../lib/invoke";
import { useToast } from "../components/Toast";

interface Props {
  onSelect: (id: string) => void;
  onNewProject: () => void;
}

const STATUS_COLORS: Record<string, string> = {
  idle: "bg-berth-text-tertiary",
  running: "bg-berth-success",
  restarting: "bg-yellow-400",
  stopped: "bg-berth-text-tertiary",
  failed: "bg-berth-error",
};

const STATUS_LABELS: Record<string, string> = {
  idle: "Idle",
  running: "Running",
  restarting: "Restarting",
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

const RUNTIME_COLORS: Record<string, string> = {
  python: "bg-blue-500/10 text-blue-400 border-blue-500/20",
  node: "bg-green-500/10 text-green-400 border-green-500/20",
  go: "bg-cyan-500/10 text-cyan-400 border-cyan-500/20",
  rust: "bg-orange-500/10 text-orange-400 border-orange-500/20",
  shell: "bg-yellow-500/10 text-yellow-400 border-yellow-500/20",
  unknown: "bg-berth-surface-2 text-berth-text-tertiary border-berth-border",
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
    <div className="h-full flex flex-col p-5">
      <div className="flex items-center justify-between mb-4">
        <div className="skeleton h-8 w-48 rounded-berth-sm" />
        <div className="skeleton h-8 w-24 rounded-berth-sm" />
      </div>
      <div className="flex flex-col gap-3">
        {[1, 2, 3].map((i) => (
          <div key={i} className="skeleton h-16 w-full rounded-berth-lg" />
        ))}
      </div>
    </div>
  );
}

function EditProjectModal({
  project,
  onClose,
  onSaved,
}: {
  project: Project;
  onClose: () => void;
  onSaved: (updated: Partial<Project>) => void;
}) {
  const [name, setName] = useState(project.name);
  const [entrypoint, setEntrypoint] = useState(project.entrypoint ?? "");
  const [runtime, setRuntime] = useState(project.runtime);
  const [detecting, setDetecting] = useState(false);
  const [saving, setSaving] = useState(false);
  const { toast } = useToast();
  const nameRef = useRef<HTMLInputElement>(null);

  useEffect(() => {
    nameRef.current?.focus();
  }, []);

  useEffect(() => {
    const handler = (e: KeyboardEvent) => {
      if (e.key === "Escape") onClose();
    };
    window.addEventListener("keydown", handler);
    return () => window.removeEventListener("keydown", handler);
  }, [onClose]);

  async function handleDetect() {
    setDetecting(true);
    try {
      const info: RuntimeInfo = await detectRuntime(project.path);
      setRuntime(info.runtime);
      if (info.entrypoint) setEntrypoint(info.entrypoint);
      toast(
        `Detected ${info.runtime} (${Math.round(info.confidence * 100)}%)`,
        "success"
      );
    } catch (e) {
      toast(`Detection failed: ${e}`, "error");
    } finally {
      setDetecting(false);
    }
  }

  async function handleSave() {
    const trimmedName = name.trim();
    if (!trimmedName) return;
    setSaving(true);
    try {
      await updateProject(project.id, trimmedName, entrypoint.trim() || null);
      onSaved({
        name: trimmedName,
        entrypoint: entrypoint.trim() || null,
        runtime,
      });
      toast("Project updated", "success");
      onClose();
    } catch (e) {
      toast(`Failed to save: ${e}`, "error");
    } finally {
      setSaving(false);
    }
  }

  return (
    <div className="modal-overlay" onClick={onClose}>
      <div className="modal-content" onClick={(e) => e.stopPropagation()}>
        <h3 className="text-base font-semibold text-berth-text-primary mb-5">
          Edit Project
        </h3>
        <div className="mb-4">
          <label className="block text-xs font-medium text-berth-text-secondary mb-1.5">
            Name
          </label>
          <input
            ref={nameRef}
            type="text"
            value={name}
            onChange={(e) => setName(e.target.value)}
            onKeyDown={(e) => {
              if (e.key === "Enter") handleSave();
            }}
            className="input"
          />
        </div>
        <div className="mb-4">
          <label className="block text-xs font-medium text-berth-text-secondary mb-1.5">
            Entrypoint
          </label>
          <input
            type="text"
            value={entrypoint}
            onChange={(e) => setEntrypoint(e.target.value)}
            placeholder="main.py, index.js, etc."
            className="input"
          />
        </div>
        <div className="mb-4">
          <label className="block text-xs font-medium text-berth-text-secondary mb-1.5">
            Runtime
          </label>
          <div className="flex items-center gap-2">
            <div className="input flex-1 !cursor-default text-berth-text-secondary">
              {runtime}
            </div>
            <button
              onClick={handleDetect}
              disabled={detecting}
              className="btn btn-secondary btn-sm"
            >
              {detecting ? "..." : "Re-detect"}
            </button>
          </div>
        </div>
        <div className="mb-5">
          <label className="block text-xs font-medium text-berth-text-secondary mb-1.5">
            Path
          </label>
          <div className="input !cursor-default text-berth-text-tertiary text-xs truncate">
            {project.path}
          </div>
        </div>
        <div className="flex justify-end gap-2">
          <button onClick={onClose} className="btn btn-secondary">
            Cancel
          </button>
          <button
            onClick={handleSave}
            disabled={saving || !name.trim()}
            className="btn btn-primary"
          >
            {saving ? "Saving..." : "Save"}
          </button>
        </div>
      </div>
    </div>
  );
}

export default function ProjectList({ onSelect, onNewProject }: Props) {
  const [projects, setProjects] = useState<Project[]>([]);
  const [loading, setLoading] = useState(true);
  const [search, setSearch] = useState("");
  const [contextMenu, setContextMenu] = useState<{
    id: string;
    x: number;
    y: number;
  } | null>(null);
  const [confirmDeleteId, setConfirmDeleteId] = useState<string | null>(null);
  const [editingProject, setEditingProject] = useState<Project | null>(null);
  const [isDragging, setIsDragging] = useState(false);
  const [targets, setTargets] = useState<TargetInfo[]>([]);
  const { toast } = useToast();

  const refresh = useCallback(() => {
    listProjects()
      .then(setProjects)
      .catch(console.error)
      .finally(() => setLoading(false));
  }, []);

  useEffect(() => {
    refresh();
    listTargets().then(setTargets).catch(console.error);
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

  useEffect(() => {
    if (!contextMenu) return;
    const handler = () => setContextMenu(null);
    window.addEventListener("click", handler);
    return () => window.removeEventListener("click", handler);
  }, [contextMenu]);

  useEffect(() => {
    const unlisten = getCurrentWindow().onDragDropEvent((event) => {
      if (event.payload.type === "enter") {
        setIsDragging(true);
      } else if (event.payload.type === "leave") {
        setIsDragging(false);
      } else if (event.payload.type === "drop") {
        setIsDragging(false);
        const paths = event.payload.paths;
        if (paths && paths.length > 0) {
          importFile(paths[0])
            .then((project) => {
              toast(`Imported "${project.name}"`, "success");
              refresh();
              onSelect(project.id);
            })
            .catch((e) => toast(`Import failed: ${e}`, "error"));
        }
      }
    });
    return () => {
      unlisten.then((fn) => fn());
    };
  }, [toast, onSelect, refresh]);

  const askDelete = useCallback((id: string) => {
    setConfirmDeleteId(id);
    setContextMenu(null);
  }, []);

  const confirmDelete = useCallback(async () => {
    if (!confirmDeleteId) return;
    const project = projects.find((p) => p.id === confirmDeleteId);
    try {
      await deleteProject(confirmDeleteId);
      setProjects((prev) => prev.filter((p) => p.id !== confirmDeleteId));
      toast(`Deleted ${project?.name ?? "project"}`, "info");
    } catch (e) {
      toast(String(e), "error");
    }
    setConfirmDeleteId(null);
  }, [confirmDeleteId, projects, toast]);

  const handleRun = useCallback(
    async (e: React.MouseEvent, id: string) => {
      e.stopPropagation();
      try {
        await runProject(id);
        toast("Project started", "success");
      } catch (err) {
        toast(String(err), "error");
      }
    },
    [toast]
  );

  const handleStop = useCallback(
    async (e: React.MouseEvent, id: string) => {
      e.stopPropagation();
      try {
        await stopProject(id);
        toast("Project stopped", "info");
      } catch (err) {
        toast(String(err), "error");
      }
    },
    [toast]
  );

  const startEditing = useCallback(
    (id: string) => {
      const project = projects.find((p) => p.id === id);
      if (project) setEditingProject(project);
      setContextMenu(null);
    },
    [projects]
  );

  const handleEditSaved = useCallback(
    (updated: Partial<Project>) => {
      if (!editingProject) return;
      setProjects((prev) =>
        prev.map((p) =>
          p.id === editingProject.id ? { ...p, ...updated } : p
        )
      );
    },
    [editingProject]
  );

  const handleContextMenu = useCallback(
    (e: React.MouseEvent, id: string) => {
      e.preventDefault();
      e.stopPropagation();
      setContextMenu({ id, x: e.clientX, y: e.clientY });
    },
    []
  );

  if (loading) return <LoadingSkeleton />;

  const filtered = search
    ? projects.filter((p) =>
        p.name.toLowerCase().includes(search.toLowerCase())
      )
    : projects;

  if (projects.length === 0) {
    return (
      <div className="flex flex-col items-center justify-center h-full gap-5 px-6">
        <div className="w-16 h-16 rounded-berth-xl bg-berth-accent-bg flex items-center justify-center">
          <Plus size={28} strokeWidth={1.5} className="text-berth-accent" />
        </div>
        <div className="text-center">
          <div className="text-base font-semibold text-berth-text-primary mb-1">
            No projects yet
          </div>
          <p className="text-sm text-berth-text-secondary max-w-[280px]">
            Paste code from Claude Code, Cursor, or any AI tool and deploy it
            instantly.
          </p>
        </div>
        <button onClick={onNewProject} className="btn btn-primary btn-lg">
          Paste & Deploy
        </button>
      </div>
    );
  }

  return (
    <div className="h-full flex flex-col relative">
      {/* Toolbar */}
      <div className="flex items-center gap-3 px-5 py-4 shrink-0">
        <div className="relative flex-1 max-w-[280px]">
          <Search
            size={14}
            className="absolute left-3 top-1/2 -translate-y-1/2 text-berth-text-tertiary"
          />
          <input
            type="text"
            value={search}
            onChange={(e) => setSearch(e.target.value)}
            placeholder="Search projects..."
            className="input !pl-8 !py-1.5 !text-sm"
          />
        </div>
        <div className="ml-auto flex items-center gap-2">
          <span className="text-xs text-berth-text-tertiary">
            {projects.length} project{projects.length !== 1 ? "s" : ""}
          </span>
          <button onClick={onNewProject} className="btn btn-primary">
            <Plus size={14} strokeWidth={2} />
            New
          </button>
        </div>
      </div>

      {/* Project list */}
      <div className="flex-1 overflow-y-auto px-5 pb-4">
        <div className="flex flex-col gap-2">
          {filtered.map((project, i) => {
            const isRunning = project.status === "running";
            return (
              <div
                key={project.id}
                onContextMenu={(e) => handleContextMenu(e, project.id)}
                className="glass-card flex items-center gap-3 px-4 py-3 cursor-pointer group animate-card-enter"
                style={{ animationDelay: `${i * 30}ms` }}
                onClick={() => onSelect(project.id)}
              >
                {/* Runtime badge */}
                <div
                  className={`w-9 h-9 rounded-berth-sm border flex items-center justify-center text-xs font-bold shrink-0 ${
                    RUNTIME_COLORS[project.runtime] ?? RUNTIME_COLORS.unknown
                  }`}
                >
                  {RUNTIME_ICONS[project.runtime] ?? "?"}
                </div>

                {/* Info */}
                <div className="flex-1 min-w-0">
                  <div className="flex items-center gap-2">
                    <span className="text-sm font-medium text-berth-text-primary truncate">
                      {project.name}
                    </span>
                    {(() => {
                      const targetName = project.default_target
                        ? targets.find((t) => t.id === project.default_target)
                            ?.name ?? "Remote"
                        : "Local";
                      const isRemote = !!project.default_target;
                      return (
                        <span
                          className={`badge ${
                            isRemote ? "badge-accent" : "badge-neutral"
                          }`}
                        >
                          {targetName}
                        </span>
                      );
                    })()}
                    {project.run_mode === "service" && (
                      <span className="badge badge-neutral text-[10px]">
                        <RefreshCw size={9} className="mr-1" />
                        service
                      </span>
                    )}
                    {project.run_count > 0 && (
                      <span className="text-[10px] text-berth-text-tertiary">
                        {project.run_count} run
                        {project.run_count !== 1 ? "s" : ""}
                      </span>
                    )}
                  </div>
                  <div className="text-xs text-berth-text-secondary truncate mt-0.5">
                    {project.entrypoint ?? project.path}
                  </div>
                </div>

                {/* Hover actions */}
                <div className="hidden group-hover:flex items-center gap-1 shrink-0">
                  {isRunning ? (
                    <button
                      onClick={(e) => handleStop(e, project.id)}
                      title="Stop"
                      className="btn btn-ghost btn-icon"
                    >
                      <Square
                        size={14}
                        fill="currentColor"
                        className="text-berth-error"
                      />
                    </button>
                  ) : (
                    <button
                      onClick={(e) => handleRun(e, project.id)}
                      title="Run"
                      className="btn btn-ghost btn-icon"
                    >
                      <Play
                        size={14}
                        fill="currentColor"
                        className="text-berth-success"
                      />
                    </button>
                  )}
                  <button
                    onClick={(e) => {
                      e.stopPropagation();
                      startEditing(project.id);
                    }}
                    title="Edit"
                    className="btn btn-ghost btn-icon"
                  >
                    <Pencil size={14} />
                  </button>
                  <button
                    onClick={(e) => {
                      e.stopPropagation();
                      askDelete(project.id);
                    }}
                    title="Delete"
                    className="btn btn-ghost btn-icon hover:!text-berth-error"
                  >
                    <Trash2 size={14} />
                  </button>
                </div>

                {/* Status (hidden on hover) */}
                <div className="flex flex-col items-end gap-0.5 shrink-0 group-hover:hidden">
                  <div className="flex items-center gap-1.5">
                    <div
                      className={`w-2 h-2 rounded-full ${
                        STATUS_COLORS[project.status] ?? STATUS_COLORS.idle
                      } ${isRunning ? "animate-pulse-soft" : ""}`}
                    />
                    <span className="text-xs text-berth-text-secondary">
                      {STATUS_LABELS[project.status] ?? "Idle"}
                    </span>
                  </div>
                  <span className="text-[10px] text-berth-text-tertiary">
                    {timeAgo(project.updated_at)}
                  </span>
                </div>
              </div>
            );
          })}
        </div>
      </div>

      {/* Drag-and-drop overlay */}
      {isDragging && (
        <div className="absolute inset-4 z-40 flex items-center justify-center border-2 border-dashed border-berth-accent rounded-berth-xl animate-fade-in pointer-events-none bg-berth-accent-bg">
          <div className="flex flex-col items-center gap-2">
            <Download size={32} strokeWidth={1.5} className="text-berth-accent" />
            <span className="text-sm font-medium text-berth-accent">
              Drop file to import
            </span>
            <span className="text-xs text-berth-text-secondary">
              .py, .js, .ts, .go, .sh, .rs
            </span>
          </div>
        </div>
      )}

      {/* Context menu */}
      {contextMenu && (
        <div
          className="context-menu fixed z-50"
          style={{ left: contextMenu.x, top: contextMenu.y }}
          onClick={(e) => e.stopPropagation()}
        >
          {(() => {
            const p = projects.find((p) => p.id === contextMenu.id);
            const running = p?.status === "running";
            return (
              <>
                {running ? (
                  <button
                    onClick={(e) => {
                      handleStop(e, contextMenu.id);
                      setContextMenu(null);
                    }}
                    className="context-menu-item"
                  >
                    <Square size={14} />
                    Stop
                  </button>
                ) : (
                  <button
                    onClick={(e) => {
                      handleRun(e, contextMenu.id);
                      setContextMenu(null);
                    }}
                    className="context-menu-item"
                  >
                    <Play size={14} />
                    Run
                  </button>
                )}
                <button
                  onClick={() => startEditing(contextMenu.id)}
                  className="context-menu-item"
                >
                  <Pencil size={14} />
                  Edit
                </button>
                <button
                  onClick={() => askDelete(contextMenu.id)}
                  className="context-menu-item context-menu-item--danger"
                >
                  <Trash2 size={14} />
                  Delete
                </button>
              </>
            );
          })()}
        </div>
      )}

      {editingProject && (
        <EditProjectModal
          project={editingProject}
          onClose={() => setEditingProject(null)}
          onSaved={handleEditSaved}
        />
      )}

      {confirmDeleteId && (
        <div
          className="modal-overlay"
          onClick={() => setConfirmDeleteId(null)}
        >
          <div
            className="modal-content"
            onClick={(e) => e.stopPropagation()}
          >
            <h3 className="text-base font-semibold text-berth-text-primary mb-1">
              Delete project?
            </h3>
            <p className="text-sm text-berth-text-secondary mb-5">
              <span className="font-medium text-berth-text-primary">
                {projects.find((p) => p.id === confirmDeleteId)?.name}
              </span>{" "}
              will be removed. This cannot be undone.
            </p>
            <div className="flex justify-end gap-2">
              <button
                onClick={() => setConfirmDeleteId(null)}
                className="btn btn-secondary"
              >
                Cancel
              </button>
              <button onClick={confirmDelete} className="btn btn-danger">
                Delete
              </button>
            </div>
          </div>
        </div>
      )}
    </div>
  );
}

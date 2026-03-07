import { useEffect, useState, useCallback, useRef } from "react";
import { listen } from "@tauri-apps/api/event";
import { getCurrentWindow } from "@tauri-apps/api/window";
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
  onTargets?: () => void;
  onSettings?: () => void;
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
          <div key={i} className="flex items-center gap-3 px-4 py-3 border-b border-runway-border">
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

  useEffect(() => { nameRef.current?.focus(); }, []);

  useEffect(() => {
    const handler = (e: KeyboardEvent) => { if (e.key === "Escape") onClose(); };
    window.addEventListener("keydown", handler);
    return () => window.removeEventListener("keydown", handler);
  }, [onClose]);

  async function handleDetect() {
    setDetecting(true);
    try {
      const info: RuntimeInfo = await detectRuntime(project.path);
      setRuntime(info.runtime);
      if (info.entrypoint) setEntrypoint(info.entrypoint);
      toast(`Detected ${info.runtime} (${Math.round(info.confidence * 100)}%)`, "success");
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
      onSaved({ name: trimmedName, entrypoint: entrypoint.trim() || null, runtime });
      toast("Project updated", "success");
      onClose();
    } catch (e) {
      toast(`Failed to save: ${e}`, "error");
    } finally {
      setSaving(false);
    }
  }

  return (
    <div className="fixed inset-0 z-50 flex items-center justify-center bg-black/50" onClick={onClose}>
      <div className="w-[380px] rounded-xl bg-runway-bg border border-runway-border p-5 shadow-2xl" onClick={(e) => e.stopPropagation()}>
        <h3 className="text-sm font-semibold mb-4">Edit Project</h3>
        <div className="mb-3">
          <label className="block text-xs font-medium text-runway-muted mb-1">Name</label>
          <input ref={nameRef} type="text" value={name} onChange={(e) => setName(e.target.value)}
            onKeyDown={(e) => { if (e.key === "Enter") handleSave(); }}
            className="w-full px-3 py-2 rounded-lg bg-runway-surface border border-runway-border text-sm text-runway-text focus:outline-none focus:border-runway-accent transition-colors" />
        </div>
        <div className="mb-3">
          <label className="block text-xs font-medium text-runway-muted mb-1">Entrypoint</label>
          <input type="text" value={entrypoint} onChange={(e) => setEntrypoint(e.target.value)}
            placeholder="main.py, index.js, etc."
            className="w-full px-3 py-2 rounded-lg bg-runway-surface border border-runway-border text-sm text-runway-text placeholder-runway-muted focus:outline-none focus:border-runway-accent transition-colors" />
        </div>
        <div className="mb-3">
          <label className="block text-xs font-medium text-runway-muted mb-1">Runtime</label>
          <div className="flex items-center gap-2">
            <div className="flex-1 px-3 py-2 rounded-lg bg-runway-surface border border-runway-border text-sm text-runway-muted">{runtime}</div>
            <button onClick={handleDetect} disabled={detecting}
              className="px-3 py-2 rounded-lg bg-runway-surface border border-runway-border text-xs text-runway-accent hover:bg-runway-border transition-colors disabled:opacity-50">
              {detecting ? "..." : "Re-detect"}
            </button>
          </div>
        </div>
        <div className="mb-4">
          <label className="block text-xs font-medium text-runway-muted mb-1">Path</label>
          <div className="px-3 py-2 rounded-lg bg-runway-surface border border-runway-border text-xs text-runway-muted truncate">{project.path}</div>
        </div>
        <div className="flex justify-end gap-2">
          <button onClick={onClose} className="px-3 py-1.5 rounded-md text-xs font-medium text-runway-muted hover:text-runway-text border border-runway-border hover:border-runway-accent/30 transition-colors">Cancel</button>
          <button onClick={handleSave} disabled={saving || !name.trim()}
            className="px-4 py-1.5 rounded-md text-xs font-medium text-white bg-runway-accent hover:opacity-90 transition-opacity disabled:opacity-50">
            {saving ? "Saving..." : "Save"}
          </button>
        </div>
      </div>
    </div>
  );
}

export default function ProjectList({ onSelect, onNewProject, onTargets, onSettings }: Props) {
  const [projects, setProjects] = useState<Project[]>([]);
  const [loading, setLoading] = useState(true);
  const [contextMenu, setContextMenu] = useState<{ id: string; x: number; y: number } | null>(null);
  const [confirmDeleteId, setConfirmDeleteId] = useState<string | null>(null);
  const [editingProject, setEditingProject] = useState<Project | null>(null);
  const [isDragging, setIsDragging] = useState(false);
  const [targets, setTargets] = useState<TargetInfo[]>([]);
  const { toast } = useToast();

  const refresh = useCallback(() => {
    listProjects().then(setProjects).catch(console.error).finally(() => setLoading(false));
  }, []);

  useEffect(() => { refresh(); listTargets().then(setTargets).catch(console.error); }, [refresh]);

  useEffect(() => {
    const unlisten = listen<StatusEvent>("project-status-change", (event) => {
      setProjects((prev) => prev.map((p) =>
        p.id === event.payload.project_id ? { ...p, status: event.payload.status } : p
      ));
    });
    return () => { unlisten.then((fn) => fn()); };
  }, []);

  useEffect(() => {
    if (!contextMenu) return;
    const handler = () => setContextMenu(null);
    window.addEventListener("click", handler);
    return () => window.removeEventListener("click", handler);
  }, [contextMenu]);

  // Drag-and-drop file import
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
    return () => { unlisten.then((fn) => fn()); };
  }, [toast, onSelect, refresh]);

  const askDelete = useCallback((id: string) => { setConfirmDeleteId(id); setContextMenu(null); }, []);

  const confirmDelete = useCallback(async () => {
    if (!confirmDeleteId) return;
    const project = projects.find((p) => p.id === confirmDeleteId);
    try {
      await deleteProject(confirmDeleteId);
      setProjects((prev) => prev.filter((p) => p.id !== confirmDeleteId));
      toast(`Deleted ${project?.name ?? "project"}`, "info");
    } catch (e) { toast(String(e), "error"); }
    setConfirmDeleteId(null);
  }, [confirmDeleteId, projects, toast]);

  const handleRun = useCallback(async (e: React.MouseEvent, id: string) => {
    e.stopPropagation();
    try { await runProject(id); toast("Project started", "success"); }
    catch (err) { toast(String(err), "error"); }
  }, [toast]);

  const handleStop = useCallback(async (e: React.MouseEvent, id: string) => {
    e.stopPropagation();
    try { await stopProject(id); toast("Project stopped", "info"); }
    catch (err) { toast(String(err), "error"); }
  }, [toast]);

  const startEditing = useCallback((id: string) => {
    const project = projects.find((p) => p.id === id);
    if (project) setEditingProject(project);
    setContextMenu(null);
  }, [projects]);

  const handleEditSaved = useCallback((updated: Partial<Project>) => {
    if (!editingProject) return;
    setProjects((prev) => prev.map((p) => p.id === editingProject.id ? { ...p, ...updated } : p));
  }, [editingProject]);

  const handleContextMenu = useCallback((e: React.MouseEvent, id: string) => {
    e.preventDefault(); e.stopPropagation();
    setContextMenu({ id, x: e.clientX, y: e.clientY });
  }, []);

  if (loading) return <LoadingSkeleton />;

  if (projects.length === 0) {
    return (
      <div className="flex flex-col items-center justify-center h-full gap-5 px-6">
        <div className="w-16 h-16 rounded-2xl bg-runway-surface border border-runway-border flex items-center justify-center">
          <svg width="28" height="28" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="1.5" className="text-runway-muted">
            <path d="M12 5v14M5 12h14" strokeLinecap="round" />
          </svg>
        </div>
        <div className="text-center">
          <div className="text-base font-semibold text-runway-text mb-1">No projects yet</div>
          <p className="text-sm text-runway-muted max-w-[260px]">Paste code from Claude Code, Cursor, or any AI tool and deploy it instantly.</p>
        </div>
        <button onClick={onNewProject} className="px-5 py-2.5 rounded-lg bg-runway-accent text-white text-sm font-medium hover:opacity-90 transition-opacity">
          Paste &amp; Deploy
        </button>
      </div>
    );
  }

  return (
    <div className="h-full flex flex-col relative">
      <div className="flex items-center justify-between px-4 py-3 border-b border-runway-border">
        <h1 className="text-sm font-semibold">
          Projects<span className="ml-1.5 text-xs font-normal text-runway-muted">{projects.length}</span>
        </h1>
        <div className="flex gap-2">
          {onSettings && (
            <button onClick={onSettings} title="Settings"
              className="p-1.5 rounded-md border border-runway-border text-runway-muted hover:text-runway-text hover:border-runway-accent transition-colors">
              <svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round">
                <circle cx="12" cy="12" r="3" /><path d="M19.4 15a1.65 1.65 0 00.33 1.82l.06.06a2 2 0 010 2.83 2 2 0 01-2.83 0l-.06-.06a1.65 1.65 0 00-1.82-.33 1.65 1.65 0 00-1 1.51V21a2 2 0 01-2 2 2 2 0 01-2-2v-.09A1.65 1.65 0 009 19.4a1.65 1.65 0 00-1.82.33l-.06.06a2 2 0 01-2.83 0 2 2 0 010-2.83l.06-.06A1.65 1.65 0 004.68 15a1.65 1.65 0 00-1.51-1H3a2 2 0 01-2-2 2 2 0 012-2h.09A1.65 1.65 0 004.6 9a1.65 1.65 0 00-.33-1.82l-.06-.06a2 2 0 010-2.83 2 2 0 012.83 0l.06.06A1.65 1.65 0 009 4.68a1.65 1.65 0 001-1.51V3a2 2 0 012-2 2 2 0 012 2v.09a1.65 1.65 0 001 1.51 1.65 1.65 0 001.82-.33l.06-.06a2 2 0 012.83 0 2 2 0 010 2.83l-.06.06A1.65 1.65 0 0019.4 9a1.65 1.65 0 001.51 1H21a2 2 0 012 2 2 2 0 01-2 2h-.09a1.65 1.65 0 00-1.51 1z" />
              </svg>
            </button>
          )}
          {onTargets && (
            <button onClick={onTargets} className="px-3 py-1.5 rounded-md border border-runway-border text-runway-muted text-xs font-medium hover:text-runway-text hover:border-runway-accent transition-colors">Targets</button>
          )}
          <button onClick={onNewProject} className="px-3 py-1.5 rounded-md bg-runway-accent text-white text-xs font-medium hover:opacity-90 transition-opacity">+ New</button>
        </div>
      </div>
      <div className="flex-1 overflow-y-auto">
        {projects.map((project) => {
          const isRunning = project.status === "running";
          return (
            <div key={project.id} onContextMenu={(e) => handleContextMenu(e, project.id)}
              className="relative w-full flex items-center gap-3 px-4 py-3 border-b border-runway-border hover:bg-runway-surface/50 transition-colors text-left group cursor-pointer"
              onClick={() => onSelect(project.id)}>
              <div className="w-8 h-8 rounded-lg bg-runway-surface border border-runway-border flex items-center justify-center text-xs font-bold text-runway-muted shrink-0 group-hover:border-runway-accent/30 transition-colors">
                {RUNTIME_ICONS[project.runtime] ?? "?"}
              </div>
              <div className="flex-1 min-w-0">
                <div className="flex items-center gap-2">
                  <span className="text-sm font-medium truncate">{project.name}</span>
                  {(() => {
                    const targetName = project.default_target
                      ? targets.find((t) => t.id === project.default_target)?.name ?? "Remote"
                      : "Local";
                    const isRemote = !!project.default_target;
                    return (
                      <span className={`text-[10px] px-1.5 py-0.5 rounded border leading-none ${
                        isRemote
                          ? "bg-runway-accent/10 border-runway-accent/30 text-runway-accent"
                          : "bg-runway-surface border-runway-border text-runway-muted"
                      }`}>
                        {targetName}
                      </span>
                    );
                  })()}
                  {project.run_count > 0 && (
                    <span className="text-[10px] text-runway-muted">{project.run_count} run{project.run_count !== 1 ? "s" : ""}</span>
                  )}
                </div>
                <div className="text-xs text-runway-muted truncate">{project.entrypoint ?? project.path}</div>
              </div>
              {/* Hover actions */}
              <div className="hidden group-hover:flex items-center gap-0.5 shrink-0">
                {isRunning ? (
                  <button onClick={(e) => handleStop(e, project.id)} title="Stop"
                    className="p-1.5 rounded hover:bg-runway-border/50 text-runway-error hover:text-runway-error transition-colors">
                    <svg width="14" height="14" viewBox="0 0 24 24" fill="currentColor"><rect x="6" y="6" width="12" height="12" rx="1" /></svg>
                  </button>
                ) : (
                  <button onClick={(e) => handleRun(e, project.id)} title="Run"
                    className="p-1.5 rounded hover:bg-runway-success/10 text-runway-success hover:text-runway-success transition-colors">
                    <svg width="14" height="14" viewBox="0 0 24 24" fill="currentColor"><path d="M8 5v14l11-7z" /></svg>
                  </button>
                )}
                <button onClick={(e) => { e.stopPropagation(); startEditing(project.id); }} title="Edit"
                  className="p-1.5 rounded hover:bg-runway-border/50 text-runway-muted hover:text-runway-text transition-colors">
                  <svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round">
                    <path d="M11 4H4a2 2 0 00-2 2v14a2 2 0 002 2h14a2 2 0 002-2v-7" /><path d="M18.5 2.5a2.121 2.121 0 013 3L12 15l-4 1 1-4 9.5-9.5z" />
                  </svg>
                </button>
                <button onClick={(e) => { e.stopPropagation(); askDelete(project.id); }} title="Delete"
                  className="p-1.5 rounded hover:bg-red-500/10 text-runway-muted hover:text-red-400 transition-colors">
                  <svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round">
                    <polyline points="3 6 5 6 21 6" /><path d="M19 6v14a2 2 0 01-2 2H7a2 2 0 01-2-2V6m3 0V4a2 2 0 012-2h4a2 2 0 012 2v2" />
                  </svg>
                </button>
              </div>
              {/* Status (hidden on hover) */}
              <div className="flex flex-col items-end gap-0.5 shrink-0 group-hover:hidden">
                <div className="flex items-center gap-1.5">
                  <div className={`w-1.5 h-1.5 rounded-full ${STATUS_COLORS[project.status] ?? STATUS_COLORS.idle} ${isRunning ? "animate-pulse-soft" : ""}`} />
                  <span className="text-xs text-runway-muted">{STATUS_LABELS[project.status] ?? "Idle"}</span>
                </div>
                <span className="text-[10px] text-runway-muted/60">{timeAgo(project.updated_at)}</span>
              </div>
            </div>
          );
        })}
      </div>

      {/* Drag-and-drop overlay */}
      {isDragging && (
        <div className="absolute inset-0 z-40 flex items-center justify-center bg-runway-accent/10 border-2 border-dashed border-runway-accent rounded-lg animate-fade-in pointer-events-none">
          <div className="flex flex-col items-center gap-2">
            <svg width="32" height="32" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="1.5" className="text-runway-accent">
              <path d="M12 3v12m0 0l-4-4m4 4l4-4" strokeLinecap="round" strokeLinejoin="round" />
              <path d="M3 17v2a2 2 0 002 2h14a2 2 0 002-2v-2" strokeLinecap="round" />
            </svg>
            <span className="text-sm font-medium text-runway-accent">Drop file to import</span>
            <span className="text-xs text-runway-muted">.py, .js, .ts, .go, .sh, .rs</span>
          </div>
        </div>
      )}

      {contextMenu && (
        <div className="fixed z-50 min-w-[140px] py-1 rounded-lg bg-runway-surface border border-runway-border shadow-lg"
          style={{ left: contextMenu.x, top: contextMenu.y }} onClick={(e) => e.stopPropagation()}>
          {(() => {
            const p = projects.find((p) => p.id === contextMenu.id);
            const running = p?.status === "running";
            return (<>
              {running ? (
                <button onClick={(e) => { handleStop(e, contextMenu.id); setContextMenu(null); }}
                  className="w-full px-3 py-1.5 text-left text-xs text-runway-text hover:bg-runway-border/50 transition-colors">Stop</button>
              ) : (
                <button onClick={(e) => { handleRun(e, contextMenu.id); setContextMenu(null); }}
                  className="w-full px-3 py-1.5 text-left text-xs text-runway-text hover:bg-runway-border/50 transition-colors">Run</button>
              )}
              <button onClick={() => startEditing(contextMenu.id)}
                className="w-full px-3 py-1.5 text-left text-xs text-runway-text hover:bg-runway-border/50 transition-colors">Edit</button>
              <button onClick={() => askDelete(contextMenu.id)}
                className="w-full px-3 py-1.5 text-left text-xs text-red-400 hover:bg-red-500/10 transition-colors">Delete</button>
            </>);
          })()}
        </div>
      )}

      {editingProject && (
        <EditProjectModal project={editingProject} onClose={() => setEditingProject(null)} onSaved={handleEditSaved} />
      )}

      {confirmDeleteId && (
        <div className="fixed inset-0 z-50 flex items-center justify-center bg-black/50" onClick={() => setConfirmDeleteId(null)}>
          <div className="w-[300px] rounded-xl bg-runway-bg border border-runway-border p-5 shadow-2xl" onClick={(e) => e.stopPropagation()}>
            <h3 className="text-sm font-semibold mb-1">Delete project?</h3>
            <p className="text-xs text-runway-muted mb-4">
              <span className="font-medium text-runway-text">{projects.find((p) => p.id === confirmDeleteId)?.name}</span>{" "}
              will be removed. This cannot be undone.
            </p>
            <div className="flex justify-end gap-2">
              <button onClick={() => setConfirmDeleteId(null)}
                className="px-3 py-1.5 rounded-md text-xs font-medium text-runway-muted hover:text-runway-text border border-runway-border hover:border-runway-accent/30 transition-colors">Cancel</button>
              <button onClick={confirmDelete}
                className="px-3 py-1.5 rounded-md text-xs font-medium text-white bg-red-500 hover:bg-red-600 transition-colors">Delete</button>
            </div>
          </div>
        </div>
      )}
    </div>
  );
}

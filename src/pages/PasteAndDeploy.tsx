import { useState, useEffect } from "react";
import {
  createProject,
  detectRuntime,
  savePasteCode,
  runProject,
  getSettings,
  listTargets,
  type RuntimeInfo,
  type TargetInfo,
} from "../lib/invoke";
import { useToast } from "../components/Toast";

interface Props {
  onBack: () => void;
  onCreated: (id: string) => void;
}

export default function PasteAndDeploy({ onBack, onCreated }: Props) {
  const [name, setName] = useState("");
  const [code, setCode] = useState("");
  const [path, setPath] = useState("");
  const [runtimeInfo, setRuntimeInfo] = useState<RuntimeInfo | null>(null);
  const [mode, setMode] = useState<"paste" | "directory">("paste");
  const [creating, setCreating] = useState(false);
  const [targets, setTargets] = useState<TargetInfo[]>([]);
  const [selectedTarget, setSelectedTarget] = useState<string>("local");
  const [autoRun, setAutoRun] = useState(false);
  const { toast } = useToast();

  useEffect(() => {
    listTargets().then(setTargets).catch(console.error);
    getSettings().then((s) => {
      setAutoRun(s.auto_run_on_create === "true");
      if (s.default_target && s.default_target !== "local") {
        setSelectedTarget(s.default_target);
      }
    }).catch(console.error);
  }, []);

  async function handleDetect() {
    if (!path) return;
    try {
      const info = await detectRuntime(path);
      setRuntimeInfo(info);
      toast(`Detected ${info.runtime} (${Math.round(info.confidence * 100)}%)`, "success");
    } catch (e) {
      toast(`Detection failed: ${e}`, "error");
    }
  }

  async function handleCreate() {
    if (!name) return;
    setCreating(true);
    try {
      let projectPath = path;
      if (mode === "paste" && code) {
        projectPath = await savePasteCode(name, code);
      }
      if (!projectPath) {
        toast("No code or path provided", "error");
        return;
      }
      const project = await createProject(name, projectPath);
      toast(`Project "${name}" created`, "success");
      if (autoRun && project.entrypoint) {
        try {
          const target = selectedTarget === "local" ? undefined : selectedTarget;
          await runProject(project.id, target);
          toast("Auto-running project...", "info");
        } catch (runErr) {
          toast(`Auto-run failed: ${runErr}`, "error");
        }
      }
      onCreated(project.id);
    } catch (e) {
      toast(`Failed to create project: ${e}`, "error");
    } finally {
      setCreating(false);
    }
  }

  return (
    <div className="h-full flex flex-col">
      <div className="flex items-center gap-3 px-4 py-3 border-b border-runway-border">
        <button
          onClick={onBack}
          className="text-runway-accent text-sm hover:underline"
        >
          &larr; Back
        </button>
        <h1 className="text-sm font-semibold">Paste &amp; Deploy</h1>
      </div>

      <div className="flex-1 overflow-y-auto p-4 flex flex-col gap-4">
        {/* Project name */}
        <div>
          <label className="block text-xs font-medium text-runway-muted mb-1">
            Project Name
          </label>
          <input
            type="text"
            value={name}
            onChange={(e) => setName(e.target.value)}
            placeholder="my-crawler"
            className="w-full px-3 py-2 rounded-lg bg-runway-surface border border-runway-border text-sm text-runway-text placeholder-runway-muted focus:outline-none focus:border-runway-accent transition-colors"
          />
        </div>

        {/* Mode toggle */}
        <div className="flex gap-2">
          <button
            onClick={() => setMode("paste")}
            className={`px-3 py-1.5 rounded-md text-xs font-medium transition-colors ${
              mode === "paste"
                ? "bg-runway-accent text-white"
                : "bg-runway-surface text-runway-muted border border-runway-border hover:border-runway-accent/30"
            }`}
          >
            Paste Code
          </button>
          <button
            onClick={() => setMode("directory")}
            className={`px-3 py-1.5 rounded-md text-xs font-medium transition-colors ${
              mode === "directory"
                ? "bg-runway-accent text-white"
                : "bg-runway-surface text-runway-muted border border-runway-border hover:border-runway-accent/30"
            }`}
          >
            Select Directory
          </button>
        </div>

        {/* Code paste area */}
        {mode === "paste" && (
          <div>
            <label className="block text-xs font-medium text-runway-muted mb-1">
              Paste your code
            </label>
            <textarea
              value={code}
              onChange={(e) => setCode(e.target.value)}
              placeholder="Paste code from Claude Code, Cursor, or any AI tool..."
              rows={12}
              className="w-full px-3 py-2 rounded-lg bg-runway-surface border border-runway-border text-sm font-mono text-runway-text placeholder-runway-muted focus:outline-none focus:border-runway-accent resize-none transition-colors"
            />
          </div>
        )}

        {/* Directory path */}
        {mode === "directory" && (
          <div>
            <label className="block text-xs font-medium text-runway-muted mb-1">
              Project Path
            </label>
            <div className="flex gap-2">
              <input
                type="text"
                value={path}
                onChange={(e) => setPath(e.target.value)}
                placeholder="/Users/you/projects/my-bot"
                className="flex-1 px-3 py-2 rounded-lg bg-runway-surface border border-runway-border text-sm text-runway-text placeholder-runway-muted focus:outline-none focus:border-runway-accent transition-colors"
              />
              <button
                onClick={handleDetect}
                className="px-3 py-2 rounded-lg bg-runway-surface border border-runway-border text-sm text-runway-accent hover:bg-runway-border transition-colors"
              >
                Detect
              </button>
            </div>
          </div>
        )}

        {/* Runtime detection result */}
        {runtimeInfo && (
          <div className="px-3 py-2 rounded-lg bg-runway-surface border border-runway-border">
            <div className="text-xs text-runway-muted mb-1">
              Detected Runtime
            </div>
            <div className="flex items-center gap-2">
              <span className="text-sm font-medium">{runtimeInfo.runtime}</span>
              {runtimeInfo.entrypoint && (
                <span className="text-xs text-runway-muted">
                  Entry: {runtimeInfo.entrypoint}
                </span>
              )}
              <span className="text-xs text-runway-muted ml-auto">
                {Math.round(runtimeInfo.confidence * 100)}% confidence
              </span>
            </div>
          </div>
        )}

        {/* Target selector */}
        {targets.length > 0 && (
          <div>
            <label className="block text-xs font-medium text-runway-muted mb-1">
              Deploy Target
            </label>
            <div className="flex gap-1.5 flex-wrap">
              <button
                onClick={() => setSelectedTarget("local")}
                className={`flex items-center gap-1.5 px-2.5 py-1.5 rounded-md text-xs font-medium transition-colors ${
                  selectedTarget === "local"
                    ? "bg-runway-accent text-white"
                    : "bg-runway-surface text-runway-muted border border-runway-border hover:border-runway-accent/30"
                }`}
              >
                <div className="w-1.5 h-1.5 rounded-full bg-runway-success" />
                Local
              </button>
              {targets.map((t) => (
                <button
                  key={t.id}
                  onClick={() => setSelectedTarget(t.id)}
                  className={`flex items-center gap-1.5 px-2.5 py-1.5 rounded-md text-xs font-medium transition-colors ${
                    selectedTarget === t.id
                      ? "bg-runway-accent text-white"
                      : "bg-runway-surface text-runway-muted border border-runway-border hover:border-runway-accent/30"
                  }`}
                >
                  <div className={`w-1.5 h-1.5 rounded-full ${
                    t.status === "online" ? "bg-runway-success" : t.status === "offline" ? "bg-runway-error" : "bg-runway-muted"
                  }`} />
                  {t.name}
                </button>
              ))}
            </div>
          </div>
        )}

        {/* Deploy button */}
        <button
          onClick={handleCreate}
          disabled={!name || creating || (mode === "paste" && !code) || (mode === "directory" && !path)}
          className="px-4 py-3 rounded-lg bg-runway-accent text-white text-sm font-medium hover:opacity-90 transition-opacity disabled:opacity-50 disabled:cursor-not-allowed"
        >
          {creating ? "Creating..." : "Create Project"}
        </button>
      </div>
    </div>
  );
}

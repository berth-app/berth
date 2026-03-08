import { useState, useEffect } from "react";
import { Folder } from "lucide-react";
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

export default function PasteAndDeploy({ onCreated }: Props) {
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
    getSettings()
      .then((s) => {
        setAutoRun(s.auto_run_on_create === "true");
        if (s.default_target && s.default_target !== "local") {
          setSelectedTarget(s.default_target);
        }
      })
      .catch(console.error);
  }, []);

  async function handleDetect() {
    if (!path) return;
    try {
      const info = await detectRuntime(path);
      setRuntimeInfo(info);
      toast(
        `Detected ${info.runtime} (${Math.round(info.confidence * 100)}%)`,
        "success"
      );
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
          const target =
            selectedTarget === "local" ? undefined : selectedTarget;
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
    <div className="h-full flex flex-col animate-page-enter">
      <div className="flex-1 overflow-y-auto p-5">
        <h1 className="text-lg font-semibold text-runway-text-primary mb-6">
          Paste & Deploy
        </h1>

        <div className="flex flex-col gap-5 max-w-lg">
          {/* Project name */}
          <div>
            <label className="block text-xs font-medium text-runway-text-secondary mb-1.5">
              Project Name
            </label>
            <input
              type="text"
              value={name}
              onChange={(e) => setName(e.target.value)}
              placeholder="my-crawler"
              className="input"
            />
          </div>

          {/* Mode toggle */}
          <div className="segmented-control self-start">
            <button
              onClick={() => setMode("paste")}
              data-active={mode === "paste"}
            >
              Paste Code
            </button>
            <button
              onClick={() => setMode("directory")}
              data-active={mode === "directory"}
            >
              Select Directory
            </button>
          </div>

          {/* Code paste area */}
          {mode === "paste" && (
            <div>
              <label className="block text-xs font-medium text-runway-text-secondary mb-1.5">
                Paste your code
              </label>
              <textarea
                value={code}
                onChange={(e) => setCode(e.target.value)}
                placeholder="Paste code from Claude Code, Cursor, or any AI tool..."
                rows={14}
                className="input !font-mono !leading-relaxed resize-none"
                style={{ minHeight: 300 }}
              />
            </div>
          )}

          {/* Directory path */}
          {mode === "directory" && (
            <div>
              <label className="block text-xs font-medium text-runway-text-secondary mb-1.5">
                Project Path
              </label>
              <div className="flex gap-2">
                <input
                  type="text"
                  value={path}
                  onChange={(e) => setPath(e.target.value)}
                  placeholder="/Users/you/projects/my-bot"
                  className="input flex-1"
                />
                <button onClick={handleDetect} className="btn btn-secondary">
                  <Folder size={14} />
                  Detect
                </button>
              </div>
            </div>
          )}

          {/* Runtime detection result */}
          {runtimeInfo && (
            <div className="glass-card-static px-4 py-3 animate-card-enter">
              <div className="text-xs text-runway-text-secondary mb-1">
                Detected Runtime
              </div>
              <div className="flex items-center gap-2">
                <span className="text-sm font-medium text-runway-text-primary">
                  {runtimeInfo.runtime}
                </span>
                {runtimeInfo.entrypoint && (
                  <span className="text-xs text-runway-text-secondary">
                    Entry: {runtimeInfo.entrypoint}
                  </span>
                )}
                <span className="text-xs text-runway-text-tertiary ml-auto">
                  {Math.round(runtimeInfo.confidence * 100)}% confidence
                </span>
              </div>
            </div>
          )}

          {/* Target selector */}
          {targets.length > 0 && (
            <div>
              <label className="block text-xs font-medium text-runway-text-secondary mb-1.5">
                Deploy Target
              </label>
              <div className="segmented-control">
                <button
                  onClick={() => setSelectedTarget("local")}
                  data-active={selectedTarget === "local"}
                >
                  Local
                </button>
                {targets.map((t) => (
                  <button
                    key={t.id}
                    onClick={() => setSelectedTarget(t.id)}
                    data-active={selectedTarget === t.id}
                  >
                    {t.name}
                  </button>
                ))}
              </div>
            </div>
          )}

          {/* Deploy button */}
          <button
            onClick={handleCreate}
            disabled={
              !name ||
              creating ||
              (mode === "paste" && !code) ||
              (mode === "directory" && !path)
            }
            className="btn btn-primary btn-lg w-full"
          >
            {creating ? "Creating..." : "Create Project"}
          </button>
        </div>
      </div>
    </div>
  );
}

import { useState } from "react";
import { createProject, detectRuntime, savePasteCode, type RuntimeInfo } from "../lib/invoke";

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

  async function handleDetect() {
    if (!path) return;
    try {
      const info = await detectRuntime(path);
      setRuntimeInfo(info);
    } catch (e) {
      console.error("Runtime detection failed:", e);
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
        console.error("No code or path provided");
        return;
      }
      const project = await createProject(name, projectPath);
      onCreated(project.id);
    } catch (e) {
      console.error("Failed to create project:", e);
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
            className="w-full px-3 py-2 rounded-lg bg-runway-surface border border-runway-border text-sm text-runway-text placeholder-runway-muted focus:outline-none focus:border-runway-accent"
          />
        </div>

        {/* Mode toggle */}
        <div className="flex gap-2">
          <button
            onClick={() => setMode("paste")}
            className={`px-3 py-1.5 rounded-md text-xs font-medium transition-colors ${
              mode === "paste"
                ? "bg-runway-accent text-white"
                : "bg-runway-surface text-runway-muted border border-runway-border"
            }`}
          >
            Paste Code
          </button>
          <button
            onClick={() => setMode("directory")}
            className={`px-3 py-1.5 rounded-md text-xs font-medium transition-colors ${
              mode === "directory"
                ? "bg-runway-accent text-white"
                : "bg-runway-surface text-runway-muted border border-runway-border"
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
              className="w-full px-3 py-2 rounded-lg bg-runway-surface border border-runway-border text-sm font-mono text-runway-text placeholder-runway-muted focus:outline-none focus:border-runway-accent resize-none"
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
                className="flex-1 px-3 py-2 rounded-lg bg-runway-surface border border-runway-border text-sm text-runway-text placeholder-runway-muted focus:outline-none focus:border-runway-accent"
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

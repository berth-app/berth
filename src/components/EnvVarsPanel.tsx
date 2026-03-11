import { useEffect, useState, useCallback } from "react";
import { X, Eye, EyeOff, Upload } from "lucide-react";
import {
  getEnvVars,
  setEnvVar,
  deleteEnvVar,
  importEnvFile,
} from "../lib/invoke";
import { useToast } from "./Toast";

export default function EnvVarsPanel({ projectId }: { projectId: string }) {
  const [vars, setVars] = useState<Record<string, string>>({});
  const [revealed, setRevealed] = useState<Set<string>>(new Set());
  const [newKey, setNewKey] = useState("");
  const [newValue, setNewValue] = useState("");
  const [adding, setAdding] = useState(false);
  const [editingKey, setEditingKey] = useState<string | null>(null);
  const [editValue, setEditValue] = useState("");
  const [importMode, setImportMode] = useState(false);
  const [importContent, setImportContent] = useState("");
  const { toast } = useToast();

  const refresh = useCallback(() => {
    getEnvVars(projectId).then(setVars).catch(console.error);
  }, [projectId]);

  useEffect(() => {
    refresh();
  }, [refresh]);

  async function handleAdd() {
    const key = newKey.trim();
    if (!key) return;
    setAdding(true);
    try {
      await setEnvVar(projectId, key, newValue);
      setNewKey("");
      setNewValue("");
      refresh();
    } catch (e) {
      toast(`Failed: ${e}`, "error");
    } finally {
      setAdding(false);
    }
  }

  async function handleDelete(key: string) {
    try {
      await deleteEnvVar(projectId, key);
      toast("Variable removed", "info");
      refresh();
    } catch (e) {
      toast(`Failed: ${e}`, "error");
    }
  }

  async function handleImport() {
    const content = importContent.trim();
    if (!content) return;
    try {
      const count = await importEnvFile(projectId, content);
      toast(`Imported ${count} variable${count !== 1 ? "s" : ""}`, "success");
      setImportContent("");
      setImportMode(false);
      refresh();
    } catch (e) {
      toast(`Import failed: ${e}`, "error");
    }
  }

  function startEditing(key: string, value: string) {
    setEditingKey(key);
    setEditValue(value);
  }

  async function handleSaveEdit(key: string) {
    try {
      await setEnvVar(projectId, key, editValue);
      setEditingKey(null);
      setEditValue("");
      refresh();
    } catch (e) {
      toast(`Failed: ${e}`, "error");
    }
  }

  function toggleReveal(key: string) {
    setRevealed((prev) => {
      const next = new Set(prev);
      if (next.has(key)) next.delete(key);
      else next.add(key);
      return next;
    });
  }

  const entries = Object.entries(vars).sort(([a], [b]) => a.localeCompare(b));

  return (
    <div className="side-panel-body flex flex-col gap-4">
      {entries.length === 0 ? (
        <div className="text-xs text-berth-text-tertiary py-4 text-center">
          No environment variables
        </div>
      ) : (
        <div className="flex flex-col gap-1">
          {entries.map(([key, value]) => (
            <div
              key={key}
              className="flex items-center gap-2 py-2 border-b border-berth-border-subtle last:border-b-0 group"
            >
              <span className="text-xs font-mono text-berth-text-primary shrink-0">
                {key}
              </span>
              {editingKey === key ? (
                <>
                  <input
                    type="text"
                    value={editValue}
                    onChange={(e) => setEditValue(e.target.value)}
                    onKeyDown={(e) => {
                      if (e.key === "Enter") handleSaveEdit(key);
                      if (e.key === "Escape") setEditingKey(null);
                    }}
                    autoFocus
                    className="input !py-0.5 !text-xs flex-1 ml-1 font-mono"
                  />
                  <button
                    onClick={() => handleSaveEdit(key)}
                    className="text-berth-accent hover:text-berth-accent-hover text-[11px] px-1"
                  >
                    Save
                  </button>
                  <button
                    onClick={() => setEditingKey(null)}
                    className="text-berth-text-tertiary hover:text-berth-text-primary p-0.5"
                  >
                    <X size={12} />
                  </button>
                </>
              ) : (
                <>
                  <span
                    onClick={() => startEditing(key, value)}
                    className="text-xs font-mono text-berth-text-tertiary truncate flex-1 ml-1 cursor-pointer hover:text-berth-text-secondary"
                    title="Click to edit"
                  >
                    {revealed.has(key) ? value : "***"}
                  </span>
                  <button
                    onClick={() => toggleReveal(key)}
                    className="opacity-0 group-hover:opacity-100 transition-opacity text-berth-text-tertiary hover:text-berth-text-primary p-0.5"
                    title={revealed.has(key) ? "Hide" : "Reveal"}
                  >
                    {revealed.has(key) ? <EyeOff size={11} /> : <Eye size={11} />}
                  </button>
                  <button
                    onClick={() => handleDelete(key)}
                    className="opacity-0 group-hover:opacity-100 transition-opacity text-berth-text-tertiary hover:text-berth-error p-0.5"
                  >
                    <X size={12} />
                  </button>
                </>
              )}
            </div>
          ))}
        </div>
      )}

      {/* Add variable */}
      <div className="border-t border-berth-border-subtle pt-3">
        <div className="flex gap-2">
          <input
            type="text"
            value={newKey}
            onChange={(e) => setNewKey(e.target.value.toUpperCase())}
            placeholder="KEY"
            className="input !py-1.5 !text-xs w-24 font-mono"
          />
          <input
            type="text"
            value={newValue}
            onChange={(e) => setNewValue(e.target.value)}
            placeholder="value"
            onKeyDown={(e) => {
              if (e.key === "Enter") handleAdd();
            }}
            className="input !py-1.5 !text-xs flex-1 font-mono"
          />
          <button
            onClick={handleAdd}
            disabled={adding || !newKey.trim()}
            className="btn btn-primary btn-sm"
          >
            {adding ? "..." : "Add"}
          </button>
        </div>

        {/* Import .env */}
        <div className="mt-2">
          {!importMode ? (
            <button
              onClick={() => setImportMode(true)}
              className="text-[11px] text-berth-text-tertiary hover:text-berth-text-secondary flex items-center gap-1"
            >
              <Upload size={10} />
              Import .env
            </button>
          ) : (
            <div className="space-y-2">
              <textarea
                value={importContent}
                onChange={(e) => setImportContent(e.target.value)}
                placeholder={"KEY=value\nSECRET=abc123"}
                rows={4}
                className="input !py-1.5 !text-xs w-full font-mono resize-none"
              />
              <div className="flex gap-2">
                <button
                  onClick={() => {
                    setImportMode(false);
                    setImportContent("");
                  }}
                  className="btn btn-secondary btn-sm flex-1"
                >
                  Cancel
                </button>
                <button
                  onClick={handleImport}
                  disabled={!importContent.trim()}
                  className="btn btn-primary btn-sm flex-1"
                >
                  Import
                </button>
              </div>
              <div className="text-[10px] text-berth-text-tertiary">
                Merges with existing variables
              </div>
            </div>
          )}
        </div>
      </div>
    </div>
  );
}

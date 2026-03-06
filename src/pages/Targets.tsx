import { useEffect, useState } from "react";
import {
  TargetInfo,
  listTargets,
  addTarget,
  removeTarget,
  pingTarget,
} from "../lib/invoke";
import { useToast } from "../components/Toast";

interface Props {
  onBack: () => void;
}

export default function Targets({ onBack }: Props) {
  const [targets, setTargets] = useState<TargetInfo[]>([]);
  const [loading, setLoading] = useState(true);
  const [showAdd, setShowAdd] = useState(false);
  const [name, setName] = useState("");
  const [host, setHost] = useState("");
  const [port, setPort] = useState("50051");
  const [pinging, setPinging] = useState<string | null>(null);
  const { addToast } = useToast();

  const refresh = async () => {
    try {
      const t = await listTargets();
      setTargets(t);
    } catch (e) {
      addToast(`Failed to load targets: ${e}`, "error");
    } finally {
      setLoading(false);
    }
  };

  useEffect(() => {
    refresh();
  }, []);

  const handleAdd = async () => {
    if (!name.trim() || !host.trim()) return;
    try {
      await addTarget(name.trim(), host.trim(), parseInt(port) || 50051);
      addToast(`Target '${name}' added`, "success");
      setName("");
      setHost("");
      setPort("50051");
      setShowAdd(false);
      refresh();
    } catch (e) {
      addToast(`${e}`, "error");
    }
  };

  const handleRemove = async (target: TargetInfo) => {
    try {
      await removeTarget(target.id);
      addToast(`Target '${target.name}' removed`, "success");
      refresh();
    } catch (e) {
      addToast(`${e}`, "error");
    }
  };

  const handlePing = async (target: TargetInfo) => {
    setPinging(target.id);
    try {
      const updated = await pingTarget(target.id);
      addToast(
        `${target.name}: ${updated.status} (v${updated.agent_version})`,
        "success"
      );
      refresh();
    } catch (e) {
      addToast(`${target.name}: ${e}`, "error");
      refresh();
    } finally {
      setPinging(null);
    }
  };

  return (
    <div className="h-full flex flex-col">
      {/* Header */}
      <div className="p-4 border-b border-runway-border flex items-center justify-between shrink-0">
        <div className="flex items-center gap-3">
          <button
            onClick={onBack}
            className="text-runway-muted hover:text-runway-text text-sm"
          >
            ← Back
          </button>
          <h1 className="text-lg font-semibold text-runway-text">
            Deploy Targets
          </h1>
        </div>
        <button
          onClick={() => setShowAdd(!showAdd)}
          className="px-3 py-1.5 bg-runway-accent text-white text-sm rounded-md hover:opacity-90"
        >
          + Add Target
        </button>
      </div>

      {/* Add form */}
      {showAdd && (
        <div className="p-4 border-b border-runway-border bg-runway-surface/50">
          <div className="flex gap-3 items-end">
            <div className="flex-1">
              <label className="text-xs text-runway-muted block mb-1">
                Name
              </label>
              <input
                type="text"
                value={name}
                onChange={(e) => setName(e.target.value)}
                placeholder="my-vps"
                className="w-full px-3 py-1.5 bg-runway-surface border border-runway-border rounded text-sm text-runway-text focus:border-runway-accent outline-none"
              />
            </div>
            <div className="flex-1">
              <label className="text-xs text-runway-muted block mb-1">
                Host
              </label>
              <input
                type="text"
                value={host}
                onChange={(e) => setHost(e.target.value)}
                placeholder="192.168.1.50"
                className="w-full px-3 py-1.5 bg-runway-surface border border-runway-border rounded text-sm text-runway-text focus:border-runway-accent outline-none"
              />
            </div>
            <div className="w-24">
              <label className="text-xs text-runway-muted block mb-1">
                Port
              </label>
              <input
                type="text"
                value={port}
                onChange={(e) => setPort(e.target.value)}
                className="w-full px-3 py-1.5 bg-runway-surface border border-runway-border rounded text-sm text-runway-text focus:border-runway-accent outline-none"
              />
            </div>
            <button
              onClick={handleAdd}
              disabled={!name.trim() || !host.trim()}
              className="px-4 py-1.5 bg-runway-accent text-white text-sm rounded disabled:opacity-40"
            >
              Add
            </button>
          </div>
        </div>
      )}

      {/* Target list */}
      <div className="flex-1 overflow-y-auto p-4">
        {/* Built-in local target */}
        <div className="mb-3 p-3 bg-runway-surface border border-runway-border rounded-lg flex items-center justify-between">
          <div className="flex items-center gap-3">
            <div className="w-2 h-2 rounded-full bg-runway-success" />
            <div>
              <span className="text-sm font-medium text-runway-text">
                local
              </span>
              <span className="text-xs text-runway-muted ml-2">
                127.0.0.1:50051
              </span>
            </div>
            <span className="text-xs px-2 py-0.5 rounded-full bg-runway-success/10 text-runway-success">
              built-in
            </span>
          </div>
          <span className="text-xs text-runway-muted">always available</span>
        </div>

        {loading ? (
          <div className="space-y-3">
            {[1, 2].map((i) => (
              <div
                key={i}
                className="h-16 bg-runway-surface border border-runway-border rounded-lg skeleton"
              />
            ))}
          </div>
        ) : targets.length === 0 ? (
          <div className="text-center py-12 text-runway-muted">
            <p className="text-sm">No remote targets configured.</p>
            <p className="text-xs mt-1">
              Add a target to deploy code to remote machines.
            </p>
          </div>
        ) : (
          <div className="space-y-3">
            {targets.map((t) => (
              <div
                key={t.id}
                className="p-3 bg-runway-surface border border-runway-border rounded-lg flex items-center justify-between"
              >
                <div className="flex items-center gap-3">
                  <div
                    className={`w-2 h-2 rounded-full ${
                      t.status === "online"
                        ? "bg-runway-success"
                        : t.status === "offline"
                        ? "bg-runway-error"
                        : "bg-runway-muted"
                    }`}
                  />
                  <div>
                    <span className="text-sm font-medium text-runway-text">
                      {t.name}
                    </span>
                    <span className="text-xs text-runway-muted ml-2">
                      {t.host}:{t.port}
                    </span>
                  </div>
                  {t.agent_version && (
                    <span className="text-xs px-2 py-0.5 rounded-full bg-runway-accent/10 text-runway-accent">
                      v{t.agent_version}
                    </span>
                  )}
                </div>
                <div className="flex gap-2">
                  <button
                    onClick={() => handlePing(t)}
                    disabled={pinging === t.id}
                    className="px-2 py-1 text-xs text-runway-accent hover:bg-runway-accent/10 rounded disabled:opacity-40"
                  >
                    {pinging === t.id ? "..." : "Ping"}
                  </button>
                  <button
                    onClick={() => handleRemove(t)}
                    className="px-2 py-1 text-xs text-runway-error hover:bg-runway-error/10 rounded"
                  >
                    Remove
                  </button>
                </div>
              </div>
            ))}
          </div>
        )}
      </div>
    </div>
  );
}

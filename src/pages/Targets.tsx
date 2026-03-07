import { useEffect, useState } from "react";
import {
  TargetInfo,
  AgentStats,
  listTargets,
  addTarget,
  removeTarget,
  pingTarget,
  getAgentStats,
} from "../lib/invoke";
import { useToast } from "../components/Toast";

interface Props {
  onBack: () => void;
}

function formatUptime(seconds: number): string {
  if (seconds < 60) return `${seconds}s`;
  if (seconds < 3600) return `${Math.floor(seconds / 60)}m`;
  if (seconds < 86400) {
    const h = Math.floor(seconds / 3600);
    const m = Math.floor((seconds % 3600) / 60);
    return m > 0 ? `${h}h ${m}m` : `${h}h`;
  }
  const d = Math.floor(seconds / 86400);
  const h = Math.floor((seconds % 86400) / 3600);
  return h > 0 ? `${d}d ${h}h` : `${d}d`;
}

function StatsPanel({ stats }: { stats: AgentStats }) {
  return (
    <div className="mt-3 pt-3 border-t border-runway-border/50">
      <div className="grid grid-cols-2 gap-x-6 gap-y-2">
        <div className="flex justify-between">
          <span className="text-xs text-runway-muted">Host</span>
          <span className="text-xs text-runway-text font-mono">{stats.agent_id}</span>
        </div>
        {stats.os && (
          <div className="flex justify-between">
            <span className="text-xs text-runway-muted">Platform</span>
            <span className="text-xs text-runway-text">{stats.os}/{stats.arch}</span>
          </div>
        )}
        <div className="flex justify-between">
          <span className="text-xs text-runway-muted">Uptime</span>
          <span className="text-xs text-runway-text">{formatUptime(stats.uptime_seconds)}</span>
        </div>
        <div className="flex justify-between">
          <span className="text-xs text-runway-muted">CPU</span>
          <span className={`text-xs font-mono ${stats.cpu_usage > 80 ? "text-runway-error" : stats.cpu_usage > 50 ? "text-yellow-500" : "text-runway-success"}`}>
            {stats.cpu_usage.toFixed(1)}%
          </span>
        </div>
        <div className="flex justify-between">
          <span className="text-xs text-runway-muted">Memory</span>
          <span className="text-xs text-runway-text font-mono">{stats.memory_mb.toLocaleString()} MB</span>
        </div>
        {stats.podman_version && (
          <div className="flex justify-between">
            <span className="text-xs text-runway-muted">Podman</span>
            <span className="text-xs text-runway-text">v{stats.podman_version}</span>
          </div>
        )}
        {stats.container_ready && (
          <div className="flex justify-between">
            <span className="text-xs text-runway-muted">Containers</span>
            <span className="text-xs text-runway-success">Ready</span>
          </div>
        )}
      </div>
      {stats.running_projects.length > 0 && (
        <div className="mt-2 pt-2 border-t border-runway-border/30">
          <span className="text-[10px] uppercase tracking-wider text-runway-muted">Running ({stats.running_projects.length})</span>
          <div className="mt-1 space-y-1">
            {stats.running_projects.map((p) => (
              <div key={p.project_id} className="flex items-center gap-2">
                <div className="w-1.5 h-1.5 rounded-full bg-runway-success animate-pulse-soft" />
                <span className="text-xs text-runway-text font-mono truncate">{p.project_id}</span>
              </div>
            ))}
          </div>
        </div>
      )}
    </div>
  );
}

export default function Targets({ onBack }: Props) {
  const [targets, setTargets] = useState<TargetInfo[]>([]);
  const [loading, setLoading] = useState(true);
  const [showAdd, setShowAdd] = useState(false);
  const [name, setName] = useState("");
  const [host, setHost] = useState("");
  const [port, setPort] = useState("50051");
  const [pinging, setPinging] = useState<string | null>(null);
  const [expandedId, setExpandedId] = useState<string | null>(null);
  const [stats, setStats] = useState<Record<string, AgentStats>>({});
  const [loadingStats, setLoadingStats] = useState<string | null>(null);
  const { toast } = useToast();

  const refresh = async () => {
    try {
      const t = await listTargets();
      setTargets(t);
    } catch (e) {
      toast(`Failed to load targets: ${e}`, "error");
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
      toast(`Target '${name}' added`, "success");
      setName("");
      setHost("");
      setPort("50051");
      setShowAdd(false);
      refresh();
    } catch (e) {
      toast(`${e}`, "error");
    }
  };

  const handleRemove = async (target: TargetInfo) => {
    try {
      await removeTarget(target.id);
      toast(`Target '${target.name}' removed`, "success");
      if (expandedId === target.id) setExpandedId(null);
      refresh();
    } catch (e) {
      toast(`${e}`, "error");
    }
  };

  const handlePing = async (target: TargetInfo) => {
    setPinging(target.id);
    try {
      const updated = await pingTarget(target.id);
      toast(
        `${target.name}: ${updated.status} (v${updated.agent_version})`,
        "success"
      );
      refresh();
    } catch (e) {
      toast(`${target.name}: ${e}`, "error");
      refresh();
    } finally {
      setPinging(null);
    }
  };

  const handleToggleStats = async (target: TargetInfo) => {
    if (expandedId === target.id) {
      setExpandedId(null);
      return;
    }
    setExpandedId(target.id);
    if (!stats[target.id]) {
      setLoadingStats(target.id);
      try {
        const s = await getAgentStats(target.id);
        setStats((prev) => ({ ...prev, [target.id]: s }));
        refresh();
      } catch (e) {
        toast(`Failed to get stats: ${e}`, "error");
        setExpandedId(null);
      } finally {
        setLoadingStats(null);
      }
    }
  };

  const handleRefreshStats = async (target: TargetInfo) => {
    setLoadingStats(target.id);
    try {
      const s = await getAgentStats(target.id);
      setStats((prev) => ({ ...prev, [target.id]: s }));
    } catch (e) {
      toast(`Failed to refresh stats: ${e}`, "error");
    } finally {
      setLoadingStats(null);
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
                className="p-3 bg-runway-surface border border-runway-border rounded-lg transition-all"
              >
                <div className="flex items-center justify-between">
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
                  <div className="flex gap-1">
                    <button
                      onClick={() => handleToggleStats(t)}
                      className={`px-2 py-1 text-xs rounded transition-colors ${
                        expandedId === t.id
                          ? "bg-runway-accent/10 text-runway-accent"
                          : "text-runway-muted hover:text-runway-text hover:bg-runway-border/50"
                      }`}
                      title="Agent stats"
                    >
                      <svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round">
                        <rect x="3" y="12" width="4" height="9" rx="1" />
                        <rect x="10" y="7" width="4" height="14" rx="1" />
                        <rect x="17" y="3" width="4" height="18" rx="1" />
                      </svg>
                    </button>
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
                {expandedId === t.id && (
                  loadingStats === t.id && !stats[t.id] ? (
                    <div className="mt-3 pt-3 border-t border-runway-border/50">
                      <div className="flex items-center gap-2 text-xs text-runway-muted">
                        <div className="w-3 h-3 border-2 border-runway-muted/30 border-t-runway-accent rounded-full animate-spin" />
                        Connecting to agent...
                      </div>
                    </div>
                  ) : stats[t.id] ? (
                    <div>
                      <StatsPanel stats={stats[t.id]} />
                      <div className="mt-2 flex justify-end">
                        <button
                          onClick={() => handleRefreshStats(t)}
                          disabled={loadingStats === t.id}
                          className="text-[10px] text-runway-muted hover:text-runway-accent transition-colors disabled:opacity-40"
                        >
                          {loadingStats === t.id ? "Refreshing..." : "Refresh stats"}
                        </button>
                      </div>
                    </div>
                  ) : null
                )}
              </div>
            ))}
          </div>
        )}
      </div>
    </div>
  );
}

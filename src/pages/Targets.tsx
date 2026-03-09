import { useEffect, useState } from "react";
import { Server, Activity, Trash2, Wifi, ArrowUpCircle, Undo2, Link2 } from "lucide-react";
import {
  TargetInfo,
  AgentStats,
  UpgradeCheck,
  PairingResult,
  listTargets,
  addTarget,
  removeTarget,
  pingTarget,
  getAgentStats,
  checkAgentUpgrade,
  upgradeAgent,
  rollbackAgent,
  upgradeAllAgents,
  pairAgent,
} from "../lib/invoke";
import { useToast } from "../components/Toast";

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
    <div className="mt-3 pt-3 border-t border-berth-border-subtle">
      <div className="grid grid-cols-2 gap-x-6 gap-y-2">
        <div className="flex justify-between">
          <span className="text-xs text-berth-text-secondary">Host</span>
          <span className="text-xs text-berth-text-primary font-mono">
            {stats.agent_id}
          </span>
        </div>
        {stats.os && (
          <div className="flex justify-between">
            <span className="text-xs text-berth-text-secondary">Platform</span>
            <span className="text-xs text-berth-text-primary">
              {stats.os}/{stats.arch}
            </span>
          </div>
        )}
        <div className="flex justify-between">
          <span className="text-xs text-berth-text-secondary">Uptime</span>
          <span className="text-xs text-berth-text-primary">
            {formatUptime(stats.uptime_seconds)}
          </span>
        </div>
        <div className="flex justify-between">
          <span className="text-xs text-berth-text-secondary">CPU</span>
          <span
            className={`text-xs font-mono ${
              stats.cpu_usage > 80
                ? "text-berth-error"
                : stats.cpu_usage > 50
                  ? "text-berth-warning"
                  : "text-berth-success"
            }`}
          >
            {stats.cpu_usage.toFixed(1)}%
          </span>
        </div>
        <div className="flex justify-between">
          <span className="text-xs text-berth-text-secondary">Memory</span>
          <span className="text-xs text-berth-text-primary font-mono">
            {stats.memory_mb.toLocaleString()} MB
          </span>
        </div>
        {stats.podman_version && (
          <div className="flex justify-between">
            <span className="text-xs text-berth-text-secondary">Podman</span>
            <span className="text-xs text-berth-text-primary">
              v{stats.podman_version}
            </span>
          </div>
        )}
        <div className="flex justify-between">
          <span className="text-xs text-berth-text-secondary">Tunnels</span>
          <span className="text-xs">
            {stats.tunnel_providers.length > 0 ? (
              stats.tunnel_providers.map((p) => (
                <span key={p} className="inline-block px-1.5 py-0.5 rounded text-[10px] font-medium bg-berth-success/15 text-berth-success">
                  {p}
                </span>
              ))
            ) : (
              <span className="text-berth-text-tertiary">None</span>
            )}
          </span>
        </div>
      </div>
      {stats.running_projects.length > 0 && (
        <div className="mt-2 pt-2 border-t border-berth-border-subtle">
          <span className="text-[10px] uppercase tracking-wider text-berth-text-tertiary">
            Running ({stats.running_projects.length})
          </span>
          <div className="mt-1 space-y-1">
            {stats.running_projects.map((p) => (
              <div key={p.project_id} className="flex items-center gap-2">
                <div className="w-1.5 h-1.5 rounded-full bg-berth-success animate-pulse-soft" />
                <span className="text-xs text-berth-text-primary font-mono truncate">
                  {p.project_id}
                </span>
              </div>
            ))}
          </div>
        </div>
      )}
    </div>
  );
}

export default function Targets() {
  const [targets, setTargets] = useState<TargetInfo[]>([]);
  const [loading, setLoading] = useState(true);
  const [showAdd, setShowAdd] = useState(false);
  const [name, setName] = useState("");
  const [host, setHost] = useState("");
  const [port, setPort] = useState("50051");
  const [natsAgentId, setNatsAgentId] = useState("");
  const [pinging, setPinging] = useState<string | null>(null);
  const [expandedId, setExpandedId] = useState<string | null>(null);
  const [stats, setStats] = useState<Record<string, AgentStats>>({});
  const [loadingStats, setLoadingStats] = useState<string | null>(null);
  const [liveStatus, setLiveStatus] = useState<Record<string, "online" | "offline" | "checking">>({});
  const [upgradeChecks, setUpgradeChecks] = useState<Record<string, UpgradeCheck>>({});
  const [upgradingId, setUpgradingId] = useState<string | null>(null);
  const [upgradingAll, setUpgradingAll] = useState(false);
  const [showPair, setShowPair] = useState(false);
  const [pairCode, setPairCode] = useState("");
  const [pairing, setPairing] = useState(false);
  const [pairStatus, setPairStatus] = useState<"idle" | "discovering" | "pairing">("idle");
  const [pairResult, setPairResult] = useState<PairingResult | null>(null);
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

  // Background health check for all targets
  const checkAllTargets = async (targetList: TargetInfo[]) => {
    for (const t of targetList) {
      setLiveStatus((prev) => ({ ...prev, [t.id]: prev[t.id] || "checking" }));
      pingTarget(t.id)
        .then(() => {
          setLiveStatus((prev) => ({ ...prev, [t.id]: "online" }));
          // Check for available upgrades
          checkAgentUpgrade(t.id)
            .then((check) => setUpgradeChecks((prev) => ({ ...prev, [t.id]: check })))
            .catch(() => {});
        })
        .catch(() => setLiveStatus((prev) => ({ ...prev, [t.id]: "offline" })));
    }
  };

  useEffect(() => {
    refresh();
  }, []);

  // Auto-check targets on load and poll every 30s
  useEffect(() => {
    if (targets.length === 0) return;
    checkAllTargets(targets);
    const interval = setInterval(() => checkAllTargets(targets), 30000);
    return () => clearInterval(interval);
  }, [targets.length]);

  const handleAdd = async () => {
    if (!name.trim() || !host.trim()) return;
    try {
      await addTarget(
        name.trim(),
        host.trim(),
        parseInt(port) || 50051,
        natsAgentId.trim()
      );
      toast(`Target '${name}' added`, "success");
      setName("");
      setHost("");
      setPort("50051");
      setNatsAgentId("");
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
    setLiveStatus((prev) => ({ ...prev, [target.id]: "checking" }));
    try {
      const updated = await pingTarget(target.id);
      setLiveStatus((prev) => ({ ...prev, [target.id]: "online" }));
      toast(
        `${target.name}: ${updated.status} (v${updated.agent_version})`,
        "success"
      );
      refresh();
    } catch (e) {
      setLiveStatus((prev) => ({ ...prev, [target.id]: "offline" }));
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

  const handleUpgrade = async (target: TargetInfo) => {
    setUpgradingId(target.id);
    try {
      const result = await upgradeAgent(target.id);
      if (result.success) {
        toast(`${target.name} upgraded to v${result.new_version}`, "success");
        // Clear upgrade check — will re-check after refresh
        setUpgradeChecks((prev) => {
          const next = { ...prev };
          delete next[target.id];
          return next;
        });
      } else {
        toast(`Upgrade failed: ${result.message}`, "error");
      }
      refresh();
    } catch (e) {
      toast(`Upgrade error: ${e}`, "error");
    } finally {
      setUpgradingId(null);
    }
  };

  const handleRollback = async (target: TargetInfo) => {
    try {
      const result = await rollbackAgent(target.id);
      if (result.success) {
        toast(`${target.name} rolled back to v${result.restored_version}`, "success");
      } else {
        toast(`Rollback failed: ${result.message}`, "error");
      }
      refresh();
    } catch (e) {
      toast(`Rollback error: ${e}`, "error");
    }
  };

  const handleUpgradeAll = async () => {
    setUpgradingAll(true);
    try {
      const results = await upgradeAllAgents();
      const succeeded = results.filter((r) => r.success).length;
      const failed = results.length - succeeded;
      if (failed === 0) {
        toast(`All ${succeeded} agent(s) upgraded successfully`, "success");
      } else {
        toast(`${succeeded} upgraded, ${failed} failed`, failed > 0 ? "error" : "success");
      }
      setUpgradeChecks({});
      refresh();
    } catch (e) {
      toast(`Upgrade all failed: ${e}`, "error");
    } finally {
      setUpgradingAll(false);
    }
  };

  const handlePair = async () => {
    if (pairCode.trim().length !== 6) return;
    setPairing(true);
    setPairResult(null);
    setPairStatus("discovering");
    try {
      // Backend discovers agent via advertisement, then claims + waits for ack
      // We show "Discovering..." briefly, then switch to "Pairing..." after a short delay
      const pairTimer = setTimeout(() => setPairStatus("pairing"), 3000);
      const result = await pairAgent(pairCode.trim());
      clearTimeout(pairTimer);
      setPairResult(result);
      toast(`Paired with ${result.agent_hostname}`, "success");
      refresh();
    } catch (e) {
      toast(`Pairing failed: ${e}`, "error");
    } finally {
      setPairing(false);
      setPairStatus("idle");
    }
  };

  const upgradesAvailable = Object.values(upgradeChecks).filter((c) => c.available).length;

  return (
    <div className="h-full flex flex-col animate-page-enter">
      {/* Header */}
      <div className="px-5 pt-5 pb-4 flex items-center justify-between shrink-0">
        <h1 className="text-lg font-semibold text-berth-text-primary">
          Deploy Targets
        </h1>
        <div className="flex gap-2">
          {upgradesAvailable > 0 && (
            <button
              onClick={handleUpgradeAll}
              disabled={upgradingAll}
              className="btn btn-ghost btn-sm text-berth-warning"
            >
              <ArrowUpCircle size={14} />
              {upgradingAll ? "Upgrading..." : `Upgrade All (${upgradesAvailable})`}
            </button>
          )}
          <button
            onClick={() => { setShowPair(true); setShowAdd(false); }}
            className="btn btn-primary"
          >
            <Link2 size={14} strokeWidth={2} />
            Pair Agent
          </button>
        </div>
      </div>

      {/* Pair Agent modal */}
      {showPair && (
        <div className="mx-5 mb-4 glass-card-static p-4 animate-card-enter">
          <div className="flex items-center justify-between mb-3">
            <h3 className="text-sm font-medium text-berth-text-primary">Pair Agent</h3>
            <button
              onClick={() => { setShowPair(false); setPairCode(""); setPairResult(null); }}
              className="btn btn-ghost btn-icon text-berth-text-tertiary"
            >
              &times;
            </button>
          </div>
          {pairResult ? (
            <div className="space-y-3">
              <div className="flex items-center gap-2">
                <div className="w-2 h-2 rounded-full bg-berth-success" />
                <span className="text-sm text-berth-success font-medium">Paired successfully</span>
              </div>
              <div className="grid grid-cols-2 gap-x-6 gap-y-1.5 text-xs">
                <div className="flex justify-between">
                  <span className="text-berth-text-secondary">Host</span>
                  <span className="text-berth-text-primary font-mono">{pairResult.agent_hostname}</span>
                </div>
                <div className="flex justify-between">
                  <span className="text-berth-text-secondary">OS</span>
                  <span className="text-berth-text-primary">{pairResult.agent_os}</span>
                </div>
                <div className="flex justify-between">
                  <span className="text-berth-text-secondary">Version</span>
                  <span className="text-berth-text-primary">v{pairResult.agent_version}</span>
                </div>
                <div className="flex justify-between">
                  <span className="text-berth-text-secondary">Agent ID</span>
                  <span className="text-berth-text-primary font-mono">{pairResult.agent_id}</span>
                </div>
              </div>
              <button
                onClick={() => { setShowPair(false); setPairCode(""); setPairResult(null); }}
                className="btn btn-primary w-full"
              >
                Done
              </button>
            </div>
          ) : (
            <div className="space-y-3">
              <p className="text-xs text-berth-text-secondary">
                Enter the 6-character pairing code shown on your agent.
              </p>
              <input
                type="text"
                value={pairCode}
                onChange={(e) => setPairCode(e.target.value.toUpperCase().replace(/[^ABCDEFGHJKLMNPQRSTUVWXYZ23456789]/g, "").slice(0, 6))}
                placeholder="K7M4XN"
                maxLength={6}
                className="input text-center text-lg font-mono tracking-[0.3em] uppercase"
                autoFocus
                onKeyDown={(e) => { if (e.key === "Enter") handlePair(); }}
              />
              <button
                onClick={handlePair}
                disabled={pairCode.trim().length !== 6 || pairing}
                className="btn btn-primary w-full"
              >
                {pairing ? (
                  <span className="flex items-center gap-2">
                    <div className="w-3 h-3 border-2 border-white/30 border-t-white rounded-full animate-spin" />
                    {pairStatus === "discovering" ? "Discovering agent..." : "Pairing..."}
                  </span>
                ) : (
                  "Pair"
                )}
              </button>
              <button
                onClick={() => { setShowPair(false); setShowAdd(true); }}
                className="text-xs text-berth-text-tertiary hover:text-berth-text-secondary w-full text-center"
              >
                Add target manually instead
              </button>
            </div>
          )}
        </div>
      )}

      {/* Add form (manual) */}
      {showAdd && (
        <div className="mx-5 mb-4 glass-card-static p-4 animate-card-enter">
          <div className="flex items-center justify-between mb-3">
            <h3 className="text-sm font-medium text-berth-text-primary">Add Target Manually</h3>
            <button
              onClick={() => setShowAdd(false)}
              className="btn btn-ghost btn-icon text-berth-text-tertiary"
            >
              &times;
            </button>
          </div>
          <div className="grid grid-cols-[1fr_1fr_80px] gap-3 mb-3">
            <div>
              <label className="text-xs text-berth-text-secondary block mb-1">
                Name
              </label>
              <input
                type="text"
                value={name}
                onChange={(e) => setName(e.target.value)}
                placeholder="my-vps"
                className="input"
              />
            </div>
            <div>
              <label className="text-xs text-berth-text-secondary block mb-1">
                Host
              </label>
              <input
                type="text"
                value={host}
                onChange={(e) => setHost(e.target.value)}
                placeholder="192.168.1.50"
                className="input"
              />
            </div>
            <div>
              <label className="text-xs text-berth-text-secondary block mb-1">
                Port
              </label>
              <input
                type="text"
                value={port}
                onChange={(e) => setPort(e.target.value)}
                className="input"
              />
            </div>
          </div>
          <div className="flex gap-3 items-end">
            <div className="flex-1">
              <label className="text-xs text-berth-text-secondary block mb-1">
                NATS Agent ID
              </label>
              <input
                type="text"
                value={natsAgentId}
                onChange={(e) => setNatsAgentId(e.target.value)}
                placeholder="hostname or agent ID"
                className="input"
              />
            </div>
            <button
              onClick={handleAdd}
              disabled={!name.trim() || !host.trim() || !natsAgentId.trim()}
              className="btn btn-primary"
            >
              Add
            </button>
          </div>
        </div>
      )}

      {/* Target list */}
      <div className="flex-1 overflow-y-auto px-5 pb-4">
        {/* Built-in local target */}
        <div className="glass-card-static flex items-center justify-between px-4 py-3 mb-3">
          <div className="flex items-center gap-3">
            <div className="w-2 h-2 rounded-full bg-berth-success" />
            <div>
              <span className="text-sm font-medium text-berth-text-primary">
                local
              </span>
              <span className="text-xs text-berth-text-secondary ml-2">
                127.0.0.1:50051
              </span>
            </div>
            <span className="badge badge-success">built-in</span>
          </div>
          <span className="text-xs text-berth-text-tertiary">
            always available
          </span>
        </div>

        {loading ? (
          <div className="space-y-3">
            {[1, 2].map((i) => (
              <div
                key={i}
                className="skeleton h-16 w-full rounded-berth-lg"
              />
            ))}
          </div>
        ) : targets.length === 0 ? (
          <div className="text-center py-16">
            <Server
              size={32}
              strokeWidth={1.5}
              className="text-berth-text-tertiary mx-auto mb-3"
            />
            <p className="text-sm text-berth-text-secondary">
              No remote targets configured
            </p>
            <p className="text-xs text-berth-text-tertiary mt-1">
              Pair an agent to deploy code to remote machines
            </p>
            <button
              onClick={() => { setShowPair(true); setShowAdd(false); }}
              className="btn btn-primary mt-4"
            >
              <Link2 size={14} strokeWidth={2} />
              Pair Agent
            </button>
          </div>
        ) : (
          <div className="space-y-3">
            {targets.map((t) => (
              <div key={t.id} className="glass-card px-4 py-3">
                <div className="flex items-center justify-between">
                  <div className="flex items-center gap-3">
                    <div className="relative flex items-center justify-center w-3 h-3">
                      {liveStatus[t.id] === "checking" ? (
                        <div className="w-2.5 h-2.5 border-[1.5px] border-berth-surface-3 border-t-berth-accent rounded-full animate-spin" />
                      ) : liveStatus[t.id] === "online" ? (
                        <>
                          <div className="absolute w-2.5 h-2.5 rounded-full bg-berth-success animate-pulse-soft opacity-40" />
                          <div className="relative w-2 h-2 rounded-full bg-berth-success" />
                        </>
                      ) : liveStatus[t.id] === "offline" ? (
                        <div className="w-2 h-2 rounded-full bg-berth-error" />
                      ) : (
                        <div className="w-2 h-2 rounded-full bg-berth-text-tertiary" />
                      )}
                    </div>
                    <div>
                      <span className="text-sm font-medium text-berth-text-primary">
                        {t.name}
                      </span>
                      <span className="text-xs text-berth-text-secondary ml-2">
                        {t.host}:{t.port}
                      </span>
                    </div>
                    {liveStatus[t.id] === "online" && (
                      <span className="badge badge-success text-[10px]">Live</span>
                    )}
                    {liveStatus[t.id] === "offline" && (
                      <span className="badge badge-error text-[10px]">Offline</span>
                    )}
                    {t.nats_enabled && (
                      <span className="badge text-[10px] bg-berth-accent/15 text-berth-accent">NATS</span>
                    )}
                    {t.owner_id && (
                      <span className="badge text-[10px] bg-berth-success/10 text-berth-success">
                        <Link2 size={8} />
                        Paired
                      </span>
                    )}
                    {upgradingId === t.id ? (
                      <span className="badge badge-warning flex items-center gap-1">
                        <div className="w-2.5 h-2.5 border-[1.5px] border-berth-warning/30 border-t-berth-warning rounded-full animate-spin" />
                        Upgrading...
                      </span>
                    ) : upgradeChecks[t.id]?.available ? (
                      <span className="badge badge-warning flex items-center gap-1">
                        <ArrowUpCircle size={10} />
                        v{upgradeChecks[t.id].current_version} → v{upgradeChecks[t.id].latest_version}
                      </span>
                    ) : t.agent_version ? (
                      <span className="badge badge-success flex items-center gap-1">
                        v{t.agent_version}
                      </span>
                    ) : null}
                  </div>
                  <div className="flex gap-1">
                    <button
                      onClick={() => handleToggleStats(t)}
                      className={`btn btn-ghost btn-icon ${
                        expandedId === t.id ? "!text-berth-accent !bg-berth-accent-bg" : ""
                      }`}
                      title="Agent stats"
                    >
                      <Activity size={14} />
                    </button>
                    {upgradeChecks[t.id]?.available && (
                      <button
                        onClick={() => handleUpgrade(t)}
                        disabled={upgradingId === t.id}
                        className="btn btn-ghost btn-sm text-berth-warning"
                        title="Upgrade agent"
                      >
                        <ArrowUpCircle size={14} />
                        {upgradingId === t.id ? "..." : "Upgrade"}
                      </button>
                    )}
                    <button
                      onClick={() => handlePing(t)}
                      disabled={pinging === t.id}
                      className="btn btn-ghost btn-sm"
                    >
                      <Wifi size={14} />
                      {pinging === t.id ? "..." : "Ping"}
                    </button>
                    <button
                      onClick={() => handleRemove(t)}
                      className="btn btn-ghost btn-icon hover:!text-berth-error"
                    >
                      <Trash2 size={14} />
                    </button>
                  </div>
                </div>
                {expandedId === t.id &&
                  (loadingStats === t.id && !stats[t.id] ? (
                    <div className="mt-3 pt-3 border-t border-berth-border-subtle">
                      <div className="flex items-center gap-2 text-xs text-berth-text-secondary">
                        <div className="w-3 h-3 border-2 border-berth-surface-3 border-t-berth-accent rounded-full animate-spin" />
                        Connecting to agent...
                      </div>
                    </div>
                  ) : stats[t.id] ? (
                    <div>
                      <StatsPanel stats={stats[t.id]} />
                      <div className="mt-2 flex justify-between items-center">
                        <button
                          onClick={() => handleRollback(t)}
                          className="btn btn-ghost btn-sm text-[10px] text-berth-text-tertiary hover:text-berth-warning"
                          title="Rollback to previous agent version"
                        >
                          <Undo2 size={10} />
                          Rollback
                        </button>
                        <button
                          onClick={() => handleRefreshStats(t)}
                          disabled={loadingStats === t.id}
                          className="btn btn-ghost btn-sm text-[10px]"
                        >
                          {loadingStats === t.id
                            ? "Refreshing..."
                            : "Refresh stats"}
                        </button>
                      </div>
                    </div>
                  ) : null)}
              </div>
            ))}
          </div>
        )}
      </div>
    </div>
  );
}

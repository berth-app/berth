import { useEffect, useState } from "react";
import {
  getSettings,
  updateSetting,
  listTargets,
  type TargetInfo,
} from "../lib/invoke";
import { useToast } from "../components/Toast";

interface Props {
  onBack: () => void;
}

export default function Settings({ onBack }: Props) {
  const [settings, setSettings] = useState<Record<string, string>>({});
  const [targets, setTargets] = useState<TargetInfo[]>([]);
  const [loading, setLoading] = useState(true);
  const { toast } = useToast();

  useEffect(() => {
    Promise.all([getSettings(), listTargets()])
      .then(([s, t]) => {
        setSettings(s);
        setTargets(t);
      })
      .catch((e) => toast(`Failed to load settings: ${e}`, "error"))
      .finally(() => setLoading(false));
  }, []);

  async function save(key: string, value: string) {
    try {
      await updateSetting(key, value);
      setSettings((prev) => ({ ...prev, [key]: value }));

      if (key === "theme") applyTheme(value);
    } catch (e) {
      toast(`Failed to save: ${e}`, "error");
    }
  }

  if (loading) {
    return (
      <div className="h-full flex flex-col">
        <div className="flex items-center gap-3 px-4 py-3 border-b border-runway-border">
          <button onClick={onBack} className="text-runway-accent text-sm hover:underline">&larr; Back</button>
          <h1 className="text-sm font-semibold">Settings</h1>
        </div>
        <div className="p-4 flex flex-col gap-3">
          {[1, 2, 3].map((i) => <div key={i} className="skeleton h-12 w-full rounded-lg" />)}
        </div>
      </div>
    );
  }

  return (
    <div className="h-full flex flex-col">
      <div className="flex items-center gap-3 px-4 py-3 border-b border-runway-border">
        <button onClick={onBack} className="text-runway-accent text-sm hover:underline">&larr; Back</button>
        <h1 className="text-sm font-semibold">Settings</h1>
      </div>

      <div className="flex-1 overflow-y-auto p-4 flex flex-col gap-5">
        {/* General */}
        <section>
          <h2 className="text-xs font-semibold text-runway-muted uppercase tracking-wider mb-3">General</h2>

          {/* Default Target */}
          <div className="mb-4">
            <label className="block text-xs font-medium text-runway-muted mb-1">Default Target</label>
            <select
              value={settings.default_target ?? "local"}
              onChange={(e) => save("default_target", e.target.value)}
              className="w-full px-3 py-2 rounded-lg bg-runway-surface border border-runway-border text-sm text-runway-text focus:outline-none focus:border-runway-accent transition-colors appearance-none"
            >
              <option value="local">Local</option>
              {targets.map((t) => (
                <option key={t.id} value={t.id}>{t.name} ({t.host}:{t.port})</option>
              ))}
            </select>
          </div>

          {/* Auto-run on create */}
          <div className="flex items-center justify-between">
            <div>
              <div className="text-sm text-runway-text">Auto-run on create</div>
              <div className="text-xs text-runway-muted">Run projects immediately after creating them</div>
            </div>
            <button
              onClick={() => save("auto_run_on_create", settings.auto_run_on_create === "true" ? "false" : "true")}
              className={`relative w-10 h-6 rounded-full transition-colors ${
                settings.auto_run_on_create === "true" ? "bg-runway-accent" : "bg-runway-border"
              }`}
            >
              <div className={`absolute top-1 w-4 h-4 rounded-full bg-white transition-transform ${
                settings.auto_run_on_create === "true" ? "translate-x-5" : "translate-x-1"
              }`} />
            </button>
          </div>
        </section>

        {/* Display */}
        <section>
          <h2 className="text-xs font-semibold text-runway-muted uppercase tracking-wider mb-3">Display</h2>

          {/* Theme */}
          <div className="mb-4">
            <label className="block text-xs font-medium text-runway-muted mb-1">Theme</label>
            <div className="flex gap-1 p-0.5 rounded-lg bg-runway-surface border border-runway-border">
              {(["system", "dark", "light"] as const).map((t) => (
                <button
                  key={t}
                  onClick={() => save("theme", t)}
                  className={`flex-1 py-1.5 rounded-md text-xs font-medium transition-colors capitalize ${
                    (settings.theme ?? "system") === t
                      ? "bg-runway-accent text-white"
                      : "text-runway-muted hover:text-runway-text"
                  }`}
                >
                  {t}
                </button>
              ))}
            </div>
          </div>

          {/* Log scrollback */}
          <div>
            <label className="block text-xs font-medium text-runway-muted mb-1">Log Scrollback Lines</label>
            <input
              type="number"
              value={settings.log_scrollback_lines ?? "10000"}
              onChange={(e) => save("log_scrollback_lines", e.target.value)}
              min={1000}
              max={100000}
              step={1000}
              className="w-full px-3 py-2 rounded-lg bg-runway-surface border border-runway-border text-sm text-runway-text focus:outline-none focus:border-runway-accent transition-colors"
            />
            <div className="text-[10px] text-runway-muted mt-1">Number of lines kept in the terminal log viewer (1,000–100,000)</div>
          </div>
        </section>

        {/* About */}
        <section>
          <h2 className="text-xs font-semibold text-runway-muted uppercase tracking-wider mb-3">About</h2>
          <div className="px-3 py-2 rounded-lg bg-runway-surface border border-runway-border">
            <div className="text-sm font-medium">Runway</div>
            <div className="text-xs text-runway-muted">v0.1.0 — Deployment control plane for AI-generated code</div>
          </div>
        </section>
      </div>
    </div>
  );
}

export function applyTheme(theme: string) {
  if (theme === "dark") {
    document.documentElement.setAttribute("data-theme", "dark");
  } else if (theme === "light") {
    document.documentElement.setAttribute("data-theme", "light");
  } else {
    document.documentElement.removeAttribute("data-theme");
  }
}

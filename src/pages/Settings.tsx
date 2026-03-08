import { useEffect, useState } from "react";
import {
  getSettings,
  updateSetting,
  listTargets,
  type TargetInfo,
} from "../lib/invoke";
import { useToast } from "../components/Toast";
import {
  setTheme,
  loadThemeManifest,
  type ThemeManifestEntry,
} from "../lib/theme";

export default function Settings() {
  const [settings, setSettings] = useState<Record<string, string>>({});
  const [targets, setTargets] = useState<TargetInfo[]>([]);
  const [themes, setThemes] = useState<ThemeManifestEntry[]>([]);
  const [loading, setLoading] = useState(true);
  const { toast } = useToast();

  useEffect(() => {
    Promise.all([getSettings(), listTargets(), loadThemeManifest()])
      .then(([s, t, th]) => {
        setSettings(s);
        setTargets(t);
        setThemes(th);
      })
      .catch((e) => toast(`Failed to load settings: ${e}`, "error"))
      .finally(() => setLoading(false));
  }, []);

  async function save(key: string, value: string) {
    try {
      await updateSetting(key, value);
      const next = { ...settings, [key]: value };
      setSettings(next);
      if (key === "theme" || key === "theme_palette") {
        await setTheme(
          next.theme_palette ?? "default",
          next.theme ?? "system"
        );
      }
    } catch (e) {
      toast(`Failed to save: ${e}`, "error");
    }
  }

  if (loading) {
    return (
      <div className="h-full flex flex-col p-5">
        <h1 className="text-lg font-semibold text-runway-text-primary mb-5">
          Settings
        </h1>
        <div className="flex flex-col gap-3">
          {[1, 2, 3].map((i) => (
            <div key={i} className="skeleton h-14 w-full rounded-runway-lg" />
          ))}
        </div>
      </div>
    );
  }

  const activePalette = settings.theme_palette ?? "default";

  return (
    <div className="h-full flex flex-col animate-page-enter">
      <div className="flex-1 overflow-y-auto p-5">
        <h1 className="text-lg font-semibold text-runway-text-primary mb-6">
          Settings
        </h1>

        <div className="flex flex-col gap-6 max-w-lg">
          {/* General */}
          <section>
            <h2 className="text-[11px] font-semibold text-runway-text-tertiary uppercase tracking-wider mb-3">
              General
            </h2>
            <div className="glass-card-static overflow-hidden">
              {/* Default Target */}
              <div className="flex items-center justify-between px-4 py-3 border-b border-runway-border-subtle">
                <label className="text-sm text-runway-text-primary">
                  Default Target
                </label>
                <select
                  value={settings.default_target ?? "local"}
                  onChange={(e) => save("default_target", e.target.value)}
                  className="input !w-auto !py-1 !px-2 !text-sm min-w-[120px]"
                >
                  <option value="local">Local</option>
                  {targets.map((t) => (
                    <option key={t.id} value={t.id}>
                      {t.name}
                    </option>
                  ))}
                </select>
              </div>
              {/* Auto-run on create */}
              <div className="flex items-center justify-between px-4 py-3">
                <div>
                  <div className="text-sm text-runway-text-primary">
                    Auto-run on create
                  </div>
                  <div className="text-xs text-runway-text-tertiary mt-0.5">
                    Run projects immediately after creating them
                  </div>
                </div>
                <button
                  onClick={() =>
                    save(
                      "auto_run_on_create",
                      settings.auto_run_on_create === "true" ? "false" : "true"
                    )
                  }
                  className="toggle"
                  data-checked={settings.auto_run_on_create === "true"}
                />
              </div>
            </div>
          </section>

          {/* Display */}
          <section>
            <h2 className="text-[11px] font-semibold text-runway-text-tertiary uppercase tracking-wider mb-3">
              Display
            </h2>
            <div className="glass-card-static overflow-hidden">
              {/* Mode */}
              <div className="flex items-center justify-between px-4 py-3 border-b border-runway-border-subtle">
                <label className="text-sm text-runway-text-primary">
                  Mode
                </label>
                <div className="segmented-control">
                  {(["system", "dark", "light"] as const).map((t) => (
                    <button
                      key={t}
                      onClick={() => save("theme", t)}
                      data-active={(settings.theme ?? "system") === t}
                      className="capitalize"
                    >
                      {t}
                    </button>
                  ))}
                </div>
              </div>

              {/* Color Theme Gallery */}
              <div className="px-4 py-3 border-b border-runway-border-subtle">
                <label className="text-sm text-runway-text-primary block mb-3">
                  Color Theme
                </label>
                <div className="grid grid-cols-5 gap-2">
                  {themes.map((t) => (
                    <button
                      key={t.id}
                      onClick={() => save("theme_palette", t.id)}
                      className={`group flex flex-col items-center gap-1.5 p-1.5 rounded-runway-md transition-all ${
                        activePalette === t.id
                          ? "ring-2 ring-runway-accent bg-runway-accent-bg"
                          : "hover:bg-runway-surface-1"
                      }`}
                    >
                      <div
                        className="w-full aspect-[3/2] rounded-runway-sm flex items-end justify-center gap-1 pb-1.5"
                        style={{ backgroundColor: t.preview.bg }}
                      >
                        <span
                          className="w-2.5 h-2.5 rounded-full"
                          style={{ backgroundColor: t.preview.surface }}
                        />
                        <span
                          className="w-2.5 h-2.5 rounded-full"
                          style={{ backgroundColor: t.preview.accent }}
                        />
                        <span
                          className="w-2.5 h-2.5 rounded-full"
                          style={{ backgroundColor: t.preview.success }}
                        />
                        <span
                          className="w-2.5 h-2.5 rounded-full"
                          style={{ backgroundColor: t.preview.error }}
                        />
                      </div>
                      <span
                        className={`text-[10px] leading-tight ${
                          activePalette === t.id
                            ? "text-runway-accent font-medium"
                            : "text-runway-text-secondary"
                        }`}
                      >
                        {t.name}
                      </span>
                    </button>
                  ))}
                </div>
              </div>

              {/* Log scrollback */}
              <div className="flex items-center justify-between px-4 py-3">
                <div>
                  <div className="text-sm text-runway-text-primary">
                    Log Scrollback
                  </div>
                  <div className="text-xs text-runway-text-tertiary mt-0.5">
                    Lines kept in terminal (1K-100K)
                  </div>
                </div>
                <input
                  type="number"
                  value={settings.log_scrollback_lines ?? "10000"}
                  onChange={(e) =>
                    save("log_scrollback_lines", e.target.value)
                  }
                  min={1000}
                  max={100000}
                  step={1000}
                  className="input !w-24 !py-1 !text-sm text-right"
                />
              </div>
            </div>
          </section>

          {/* Advanced */}
          <section>
            <h2 className="text-[11px] font-semibold text-runway-text-tertiary uppercase tracking-wider mb-3">
              Advanced
            </h2>
            <div className="glass-card-static divide-y divide-runway-border-subtle">
              <div className="flex items-center justify-between px-4 py-3">
                <div>
                  <div className="text-sm text-runway-text-primary">
                    GitHub Token
                  </div>
                  <div className="text-xs text-runway-text-tertiary mt-0.5">
                    Required for agent upgrades (private repo access)
                  </div>
                </div>
                <input
                  type="password"
                  placeholder="ghp_..."
                  value={settings.github_token ?? ""}
                  onChange={(e) => save("github_token", e.target.value)}
                  className="input !w-48 !py-1 !text-sm"
                />
              </div>
            </div>
          </section>

          {/* About */}
          <section>
            <h2 className="text-[11px] font-semibold text-runway-text-tertiary uppercase tracking-wider mb-3">
              About
            </h2>
            <div className="glass-card-static px-4 py-3">
              <div className="text-sm font-medium text-runway-text-primary">
                Runway
              </div>
              <div className="text-xs text-runway-text-secondary mt-0.5">
                v0.1.5 — Deployment control plane for AI-generated code
              </div>
            </div>
          </section>
        </div>
      </div>
    </div>
  );
}

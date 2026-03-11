import { useEffect, useState } from "react";
import { getVersion } from "@tauri-apps/api/app";
import {
  getSettings,
  updateSetting,
  listTargets,
  saveNatsCredentials,
  clearNatsCredentials,
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
  const [natsPaste, setNatsPaste] = useState("");
  const [natsSaving, setNatsSaving] = useState(false);
  const [appVersion, setAppVersion] = useState("");
  const { toast } = useToast();

  useEffect(() => {
    getVersion().then(setAppVersion);
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
        <h1 className="text-lg font-semibold text-berth-text-primary mb-5">
          Settings
        </h1>
        <div className="flex flex-col gap-3">
          {[1, 2, 3].map((i) => (
            <div key={i} className="skeleton h-14 w-full rounded-berth-lg" />
          ))}
        </div>
      </div>
    );
  }

  const natsConfigured = !!(settings.nats_creds && settings.nats_creds.length > 0);

  async function handleSaveNatsCreds() {
    if (!natsPaste.trim()) return;
    setNatsSaving(true);
    try {
      await saveNatsCredentials(natsPaste);
      const s = await getSettings();
      setSettings(s);
      setNatsPaste("");
      toast("NATS credentials saved", "success");
    } catch (e) {
      toast(`Failed to save credentials: ${e}`, "error");
    } finally {
      setNatsSaving(false);
    }
  }

  async function handleClearNatsCreds() {
    try {
      await clearNatsCredentials();
      const s = await getSettings();
      setSettings(s);
      toast("NATS credentials cleared", "success");
    } catch (e) {
      toast(`Failed to clear credentials: ${e}`, "error");
    }
  }

  const activePalette = settings.theme_palette ?? "default";

  return (
    <div className="h-full flex flex-col animate-page-enter">
      <div className="flex-1 overflow-y-auto p-5">
        <h1 className="text-lg font-semibold text-berth-text-primary mb-6">
          Settings
        </h1>

        <div className="flex flex-col gap-6 max-w-lg">
          {/* General */}
          <section>
            <h2 className="text-[11px] font-semibold text-berth-text-tertiary uppercase tracking-wider mb-3">
              General
            </h2>
            <div className="glass-card-static overflow-hidden">
              {/* Default Target */}
              <div className="flex items-center justify-between px-4 py-3 border-b border-berth-border-subtle">
                <label className="text-sm text-berth-text-primary">
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
                  <div className="text-sm text-berth-text-primary">
                    Auto-run on create
                  </div>
                  <div className="text-xs text-berth-text-tertiary mt-0.5">
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
            <h2 className="text-[11px] font-semibold text-berth-text-tertiary uppercase tracking-wider mb-3">
              Display
            </h2>
            <div className="glass-card-static overflow-hidden">
              {/* Mode */}
              <div className="flex items-center justify-between px-4 py-3 border-b border-berth-border-subtle">
                <label className="text-sm text-berth-text-primary">
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
              <div className="px-4 py-3 border-b border-berth-border-subtle">
                <label className="text-sm text-berth-text-primary block mb-3">
                  Color Theme
                </label>
                <div className="grid grid-cols-5 gap-2">
                  {themes.map((t) => (
                    <button
                      key={t.id}
                      onClick={() => save("theme_palette", t.id)}
                      className={`group flex flex-col items-center gap-1.5 p-1.5 rounded-berth-md transition-all ${
                        activePalette === t.id
                          ? "ring-2 ring-berth-accent bg-berth-accent-bg"
                          : "hover:bg-berth-surface-1"
                      }`}
                    >
                      <div
                        className="w-full aspect-[3/2] rounded-berth-sm flex items-end justify-center gap-1 pb-1.5"
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
                            ? "text-berth-accent font-medium"
                            : "text-berth-text-secondary"
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
                  <div className="text-sm text-berth-text-primary">
                    Log Scrollback
                  </div>
                  <div className="text-xs text-berth-text-tertiary mt-0.5">
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

          {/* NATS Relay */}
          <section>
            <h2 className="text-[11px] font-semibold text-berth-text-tertiary uppercase tracking-wider mb-3">
              NATS Relay
            </h2>
            <div className="glass-card-static divide-y divide-berth-border-subtle">
              <div className="px-4 py-3">
                <div className="text-xs text-berth-text-tertiary mb-3">
                  Connect via Synadia Cloud for relay-based agent communication.
                  Not required for direct connections.
                </div>

                {/* Step-by-step instructions */}
                <div className="text-xs text-berth-text-secondary mb-3 space-y-1">
                  <div className="font-medium text-berth-text-primary">Setup:</div>
                  <ol className="list-decimal list-inside space-y-0.5 text-[11px]">
                    <li>Sign up at{" "}
                      <a href="https://cloud.synadia.com" target="_blank" rel="noopener noreferrer" className="text-berth-accent hover:underline">cloud.synadia.com</a>
                    </li>
                    <li>Create an account &rarr; copy your credentials</li>
                    <li>Paste the full credentials block below</li>
                  </ol>
                </div>

                <div className="flex flex-col gap-3">
                  <div>
                    <label className="text-sm text-berth-text-primary block mb-1">
                      NATS URL
                    </label>
                    <input
                      type="text"
                      placeholder="tls://connect.ngs.global"
                      value={settings.nats_url ?? ""}
                      onChange={(e) => save("nats_url", e.target.value)}
                      className="input !py-1.5 !text-sm w-full"
                    />
                  </div>

                  {/* Credentials: show paste area or configured status */}
                  <div>
                    <label className="text-sm text-berth-text-primary block mb-1">
                      Credentials
                    </label>
                    {natsConfigured ? (
                      <div className="flex items-center justify-between">
                        <div className="flex items-center gap-1.5">
                          <span className="w-1.5 h-1.5 rounded-full bg-green-500" />
                          <span className="text-xs text-berth-text-secondary">
                            Credentials configured
                          </span>
                        </div>
                        <button
                          onClick={handleClearNatsCreds}
                          className="text-xs text-red-400 hover:text-red-300 transition-colors"
                        >
                          Clear
                        </button>
                      </div>
                    ) : (
                      <>
                        <textarea
                          placeholder={"-----BEGIN NATS USER JWT-----\neyJ0eX...\n------END NATS USER JWT------\n\n-----BEGIN USER NKEY SEED-----\nSUANP...\n------END USER NKEY SEED------"}
                          value={natsPaste}
                          onChange={(e) => setNatsPaste(e.target.value)}
                          rows={6}
                          className="input !py-1.5 !text-sm w-full font-mono resize-none"
                        />
                        <div className="text-[10px] text-red-400 mt-1">
                          These credentials are sensitive. Never share them with anyone.
                        </div>
                        {natsPaste.trim() && (
                          <button
                            onClick={handleSaveNatsCreds}
                            disabled={natsSaving}
                            className="btn-primary mt-2 !py-1 !px-3 !text-xs"
                          >
                            {natsSaving ? "Saving..." : "Save Credentials"}
                          </button>
                        )}
                      </>
                    )}
                  </div>
                </div>

                {natsConfigured && settings.nats_url && (
                  <div className="mt-3 flex items-center gap-1.5">
                    <span className="w-1.5 h-1.5 rounded-full bg-green-500" />
                    <span className="text-xs text-berth-text-secondary">
                      Ready — enable NATS per target in Targets page
                    </span>
                  </div>
                )}
              </div>
            </div>
          </section>

          {/* Direct Connection (mTLS) */}
          <section>
            <h2 className="text-[11px] font-semibold text-berth-text-tertiary uppercase tracking-wider mb-3">
              Direct Connection (mTLS)
            </h2>
            <div className="glass-card-static divide-y divide-berth-border-subtle">
              <div className="px-4 py-3">
                <div className="text-xs text-berth-text-tertiary mb-3">
                  For direct gRPC connections without Synadia Cloud. Import the
                  certificates generated by <code className="text-[10px]">berth-agent init-tls</code> on your server.
                </div>
                <div className="flex flex-col gap-3">
                  <div>
                    <label className="text-sm text-berth-text-primary block mb-1">
                      CA Certificate
                    </label>
                    <input
                      type="text"
                      placeholder="/path/to/ca.crt"
                      value={settings.tls_ca ?? ""}
                      onChange={(e) => save("tls_ca", e.target.value)}
                      className="input !py-1.5 !text-sm w-full"
                    />
                  </div>
                  <div>
                    <label className="text-sm text-berth-text-primary block mb-1">
                      Client Certificate
                    </label>
                    <input
                      type="text"
                      placeholder="/path/to/client.crt"
                      value={settings.tls_client_cert ?? ""}
                      onChange={(e) => save("tls_client_cert", e.target.value)}
                      className="input !py-1.5 !text-sm w-full"
                    />
                  </div>
                  <div>
                    <label className="text-sm text-berth-text-primary block mb-1">
                      Client Key
                    </label>
                    <input
                      type="text"
                      placeholder="/path/to/client.key"
                      value={settings.tls_client_key ?? ""}
                      onChange={(e) => save("tls_client_key", e.target.value)}
                      className="input !py-1.5 !text-sm w-full"
                    />
                  </div>
                </div>
                {settings.tls_ca && settings.tls_client_cert && settings.tls_client_key && (
                  <div className="mt-3 flex items-center gap-1.5">
                    <span className="w-1.5 h-1.5 rounded-full bg-green-500" />
                    <span className="text-xs text-berth-text-secondary">
                      Configured — add targets with host:port in Targets page
                    </span>
                  </div>
                )}
              </div>
            </div>
          </section>

          {/* About */}
          <section>
            <h2 className="text-[11px] font-semibold text-berth-text-tertiary uppercase tracking-wider mb-3">
              About
            </h2>
            <div className="glass-card-static px-4 py-3">
              <div className="text-sm font-medium text-berth-text-primary">
                Berth
              </div>
              <div className="text-xs text-berth-text-secondary mt-0.5">
                v{appVersion} — Deployment control plane for AI-generated code
              </div>
              <div className="text-[10px] text-berth-text-tertiary mt-1">
                Licensed under Apache 2.0
              </div>
            </div>
          </section>
        </div>
      </div>
    </div>
  );
}

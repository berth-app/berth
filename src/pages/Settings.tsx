import { useEffect, useState } from "react";
import { listen } from "@tauri-apps/api/event";
import {
  getSettings,
  updateSetting,
  listTargets,
  authGetState,
  authSendMagicLink,
  authHandleCallback,
  authRefresh,
  authLogout,
  type TargetInfo,
  type AuthInfo,
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
  const [auth, setAuth] = useState<AuthInfo>({ tier: "anonymous", email: null, user_id: null });
  const [authEmail, setAuthEmail] = useState("");
  const [authLoading, setAuthLoading] = useState(false);
  const [authPending, setAuthPending] = useState(false);
  const [callbackUrl, setCallbackUrl] = useState("");
  const [loading, setLoading] = useState(true);
  const { toast } = useToast();

  useEffect(() => {
    Promise.all([getSettings(), listTargets(), loadThemeManifest(), authGetState()])
      .then(([s, t, th, a]) => {
        setSettings(s);
        setTargets(t);
        setThemes(th);
        setAuth(a);
      })
      .catch((e) => toast(`Failed to load settings: ${e}`, "error"))
      .finally(() => setLoading(false));

    // Listen for deep link auth callback (berth://auth/callback)
    const unlisten = listen<string>("auth-callback", async (event) => {
      try {
        const url = new URL(event.payload);
        const accessToken = url.searchParams.get("access_token");
        const refreshToken = url.searchParams.get("refresh_token");
        if (accessToken && refreshToken) {
          const result = await authHandleCallback(accessToken, refreshToken);
          setAuth(result);
          setAuthPending(false);
          setAuthEmail("");
          toast("Signed in successfully", "success");
        }
      } catch (err) {
        toast(`Auth callback failed: ${err}`, "error");
      }
    });

    // Try refreshing session on mount
    authRefresh()
      .then((result) => setAuth(result))
      .catch(() => {}); // silently ignore if no session

    return () => {
      unlisten.then((fn) => fn());
    };
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

  const activePalette = settings.theme_palette ?? "default";

  return (
    <div className="h-full flex flex-col animate-page-enter">
      <div className="flex-1 overflow-y-auto p-5">
        <h1 className="text-lg font-semibold text-berth-text-primary mb-6">
          Settings
        </h1>

        <div className="flex flex-col gap-6 max-w-lg">
          {/* Account */}
          <section>
            <h2 className="text-[11px] font-semibold text-berth-text-tertiary uppercase tracking-wider mb-3">
              Account
            </h2>
            <div className="glass-card-static overflow-hidden">
              {auth.tier === "anonymous" && !authPending ? (
                /* State 1: Anonymous — email input + Send Magic Link */
                <div className="px-4 py-4">
                  <div className="text-sm text-berth-text-primary mb-1">
                    Sign in to sync settings and unlock features
                  </div>
                  <div className="text-xs text-berth-text-tertiary mb-3">
                    We'll send a magic link to your email — no password needed
                  </div>
                  <form
                    className="flex gap-2"
                    onSubmit={async (e) => {
                      e.preventDefault();
                      if (!authEmail.trim()) return;
                      setAuthLoading(true);
                      try {
                        await authSendMagicLink(authEmail);
                        setAuthPending(true);
                        toast("Magic link sent — check your email", "success");
                      } catch (err) {
                        toast(`Failed to send magic link: ${err}`, "error");
                      } finally {
                        setAuthLoading(false);
                      }
                    }}
                  >
                    <input
                      type="email"
                      placeholder="you@example.com"
                      value={authEmail}
                      onChange={(e) => setAuthEmail(e.target.value)}
                      className="input !py-1.5 !text-sm flex-1"
                      disabled={authLoading}
                    />
                    <button
                      type="submit"
                      disabled={authLoading || !authEmail.trim()}
                      className="px-4 py-1.5 bg-berth-accent text-white text-sm font-medium rounded-berth-md hover:opacity-90 transition-opacity disabled:opacity-50"
                    >
                      {authLoading ? "Sending\u2026" : "Send Magic Link"}
                    </button>
                  </form>
                </div>
              ) : auth.tier === "anonymous" && authPending ? (
                /* State 2: Pending — waiting for magic link click */
                <div className="px-4 py-4">
                  <div className="text-sm text-berth-text-primary mb-1">
                    Check your email for a magic link
                  </div>
                  <div className="text-xs text-berth-text-tertiary mb-3">
                    We sent a sign-in link to{" "}
                    <span className="font-medium text-berth-text-secondary">
                      {authEmail}
                    </span>
                    . Click it, then paste the URL you land on below.
                  </div>
                  <form
                    className="mb-3"
                    onSubmit={async (e) => {
                      e.preventDefault();
                      if (!callbackUrl.trim()) return;
                      setAuthLoading(true);
                      try {
                        // Extract tokens from hash fragment (#access_token=...&refresh_token=...)
                        const hashIndex = callbackUrl.indexOf("#");
                        const fragment = hashIndex >= 0 ? callbackUrl.substring(hashIndex + 1) : "";
                        const params = new URLSearchParams(fragment);
                        const accessToken = params.get("access_token");
                        const refreshToken = params.get("refresh_token");
                        if (!accessToken || !refreshToken) {
                          toast("Could not find tokens in the URL. Make sure you copied the full URL from your browser.", "error");
                          return;
                        }
                        const result = await authHandleCallback(accessToken, refreshToken);
                        setAuth(result);
                        setAuthPending(false);
                        setAuthEmail("");
                        setCallbackUrl("");
                        toast("Signed in successfully", "success");
                      } catch (err) {
                        toast(`Sign in failed: ${err}`, "error");
                      } finally {
                        setAuthLoading(false);
                      }
                    }}
                  >
                    <input
                      type="text"
                      placeholder="Paste the callback URL here"
                      value={callbackUrl}
                      onChange={(e) => setCallbackUrl(e.target.value)}
                      className="input !py-1.5 !text-sm w-full mb-2"
                      disabled={authLoading}
                    />
                    <button
                      type="submit"
                      disabled={authLoading || !callbackUrl.trim()}
                      className="px-4 py-1.5 bg-berth-accent text-white text-sm font-medium rounded-berth-md hover:opacity-90 transition-opacity disabled:opacity-50"
                    >
                      {authLoading ? "Signing in\u2026" : "Complete Sign In"}
                    </button>
                  </form>
                  <div className="flex gap-2">
                    <button
                      onClick={async () => {
                        setAuthLoading(true);
                        try {
                          await authSendMagicLink(authEmail);
                          toast("Magic link resent", "success");
                        } catch (err) {
                          toast(`Failed to resend: ${err}`, "error");
                        } finally {
                          setAuthLoading(false);
                        }
                      }}
                      disabled={authLoading}
                      className="text-xs text-berth-text-secondary hover:text-berth-text-primary transition-colors"
                    >
                      Resend link
                    </button>
                    <span className="text-xs text-berth-text-tertiary">|</span>
                    <button
                      onClick={() => {
                        setAuthPending(false);
                        setAuthEmail("");
                        setCallbackUrl("");
                      }}
                      className="text-xs text-berth-text-secondary hover:text-berth-text-primary transition-colors"
                    >
                      Cancel
                    </button>
                  </div>
                </div>
              ) : (
                /* State 3: Signed in — email + tier badge + Sign Out */
                <div className="flex items-center justify-between px-4 py-3">
                  <div className="flex items-center gap-3">
                    <div className="w-8 h-8 rounded-full bg-berth-accent/20 flex items-center justify-center text-berth-accent text-sm font-semibold">
                      {auth.email?.[0]?.toUpperCase() ?? "?"}
                    </div>
                    <div>
                      <div className="text-sm text-berth-text-primary">
                        {auth.email}
                      </div>
                      <div className="text-xs mt-0.5 flex items-center gap-1.5">
                        {auth.tier === "early_adopter" ? (
                          <>
                            <span className="inline-block w-1.5 h-1.5 rounded-full bg-purple-500" />
                            <span className="text-purple-400">Early Adopter</span>
                            <span className="text-berth-text-tertiary">- All Pro features included</span>
                          </>
                        ) : (
                          <>
                            <span className="inline-block w-1.5 h-1.5 rounded-full bg-green-500" />
                            <span className="text-berth-text-tertiary">
                              {auth.tier.charAt(0).toUpperCase() + auth.tier.slice(1)} plan
                            </span>
                          </>
                        )}
                      </div>
                    </div>
                  </div>
                  <button
                    onClick={async () => {
                      try {
                        const result = await authLogout();
                        setAuth(result);
                        toast("Signed out", "success");
                      } catch (err) {
                        toast(`Sign out failed: ${err}`, "error");
                      }
                    }}
                    className="text-xs text-berth-text-secondary hover:text-berth-text-primary transition-colors px-2 py-1"
                  >
                    Sign Out
                  </button>
                </div>
              )}
            </div>
          </section>

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

          {/* Advanced */}
          <section>
            <h2 className="text-[11px] font-semibold text-berth-text-tertiary uppercase tracking-wider mb-3">
              Advanced
            </h2>
            <div className="glass-card-static divide-y divide-berth-border-subtle">
              <div className="flex items-center justify-between px-4 py-3">
                <div>
                  <div className="text-sm text-berth-text-primary">
                    GitHub Token
                  </div>
                  <div className="text-xs text-berth-text-tertiary mt-0.5">
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
            <h2 className="text-[11px] font-semibold text-berth-text-tertiary uppercase tracking-wider mb-3">
              About
            </h2>
            <div className="glass-card-static px-4 py-3">
              <div className="text-sm font-medium text-berth-text-primary">
                Berth
              </div>
              <div className="text-xs text-berth-text-secondary mt-0.5">
                v0.1.9 — Deployment control plane for AI-generated code
              </div>
            </div>
          </section>
        </div>
      </div>
    </div>
  );
}

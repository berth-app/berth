export interface ThemePreview {
  bg: string;
  surface: string;
  accent: string;
  success: string;
  error: string;
}

export interface ThemeManifestEntry {
  id: string;
  name: string;
  file: string;
  preview: ThemePreview;
}

export interface ThemeData {
  id: string;
  name: string;
  dark: Record<string, string>;
  light: Record<string, string>;
  preview: ThemePreview;
}

const themeCache = new Map<string, ThemeData>();
let currentPalette = "default";
let currentMode = "system";

const THEME_VARS = [
  "--runway-bg",
  "--runway-surface-0",
  "--runway-surface-1",
  "--runway-surface-2",
  "--runway-surface-3",
  "--runway-text-primary",
  "--runway-text-secondary",
  "--runway-text-tertiary",
  "--runway-border",
  "--runway-border-subtle",
  "--runway-border-strong",
  "--runway-accent",
  "--runway-accent-light",
  "--runway-accent-dark",
  "--runway-accent-bg",
  "--runway-accent-border",
  "--runway-success",
  "--runway-success-bg",
  "--runway-warning",
  "--runway-warning-bg",
  "--runway-error",
  "--runway-error-bg",
  "--runway-shadow-sm",
  "--runway-shadow-md",
  "--runway-shadow-lg",
  "--runway-shadow-glow",
  "--runway-glass-bg",
  "--runway-glass-border",
];

export async function loadThemeManifest(): Promise<ThemeManifestEntry[]> {
  const res = await fetch("/themes/_index.json");
  return res.json();
}

export async function loadTheme(id: string): Promise<ThemeData> {
  const cached = themeCache.get(id);
  if (cached) return cached;
  const res = await fetch(`/themes/${id}.json`);
  const data: ThemeData = await res.json();
  themeCache.set(id, data);
  return data;
}

export function resolveMode(setting: string): "dark" | "light" {
  if (setting === "light") return "light";
  if (setting === "dark") return "dark";
  return window.matchMedia("(prefers-color-scheme: light)").matches
    ? "light"
    : "dark";
}

function clearThemeVars() {
  const el = document.documentElement;
  for (const v of THEME_VARS) {
    el.style.removeProperty(v);
  }
}

function applyMode(modeSetting: string) {
  if (modeSetting === "dark") {
    document.documentElement.setAttribute("data-theme", "dark");
  } else if (modeSetting === "light") {
    document.documentElement.setAttribute("data-theme", "light");
  } else {
    document.documentElement.removeAttribute("data-theme");
  }
}

function applyThemeVars(vars: Record<string, string>) {
  const el = document.documentElement;
  clearThemeVars();
  for (const [key, value] of Object.entries(vars)) {
    el.style.setProperty(key, value);
  }
}

export async function setTheme(
  paletteId: string,
  modeSetting: string
): Promise<void> {
  currentPalette = paletteId;
  currentMode = modeSetting;

  applyMode(modeSetting);

  if (paletteId === "default") {
    clearThemeVars();
    localStorage.removeItem("runway_theme_cache");
    return;
  }

  const theme = await loadTheme(paletteId);
  const mode = resolveMode(modeSetting);
  const vars = theme[mode];
  applyThemeVars(vars);

  localStorage.setItem(
    "runway_theme_cache",
    JSON.stringify({ palette: paletteId, mode: modeSetting, vars })
  );
}

export function initThemeListener() {
  const mql = window.matchMedia("(prefers-color-scheme: light)");
  mql.addEventListener("change", () => {
    if (currentMode === "system") {
      if (currentPalette === "default") return;
      loadTheme(currentPalette).then((theme) => {
        const mode = resolveMode("system");
        applyThemeVars(theme[mode]);
        localStorage.setItem(
          "runway_theme_cache",
          JSON.stringify({
            palette: currentPalette,
            mode: currentMode,
            vars: theme[mode],
          })
        );
      });
    }
  });
}

export function applyThemeCacheFromLocalStorage() {
  const raw = localStorage.getItem("runway_theme_cache");
  if (!raw) return;
  try {
    const { palette, mode, vars } = JSON.parse(raw);
    if (palette && palette !== "default" && vars) {
      applyMode(mode);
      applyThemeVars(vars);
      currentPalette = palette;
      currentMode = mode;
    }
  } catch {
    // ignore corrupt cache
  }
}

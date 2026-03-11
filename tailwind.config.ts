import type { Config } from "tailwindcss";

export default {
  content: ["./index.html", "./src/**/*.{ts,tsx}"],
  theme: {
    extend: {
      fontFamily: {
        sans: [
          "-apple-system",
          "BlinkMacSystemFont",
          "SF Pro Text",
          "Helvetica Neue",
          "sans-serif",
        ],
        mono: ["SF Mono", "Menlo", "Monaco", "Consolas", "monospace"],
      },
      colors: {
        berth: {
          bg: "var(--berth-bg)",
          "surface-0": "var(--berth-surface-0)",
          "surface-1": "var(--berth-surface-1)",
          "surface-2": "var(--berth-surface-2)",
          "surface-3": "var(--berth-surface-3)",
          "text-primary": "var(--berth-text-primary)",
          "text-secondary": "var(--berth-text-secondary)",
          "text-tertiary": "var(--berth-text-tertiary)",
          border: "var(--berth-border)",
          "border-subtle": "var(--berth-border-subtle)",
          "border-strong": "var(--berth-border-strong)",
          accent: "var(--berth-accent)",
          "accent-light": "var(--berth-accent-light)",
          "accent-dark": "var(--berth-accent-dark)",
          "accent-bg": "var(--berth-accent-bg)",
          "accent-border": "var(--berth-accent-border)",
          success: "var(--berth-success)",
          "success-bg": "var(--berth-success-bg)",
          warning: "var(--berth-warning)",
          "warning-bg": "var(--berth-warning-bg)",
          error: "var(--berth-error)",
          "error-bg": "var(--berth-error-bg)",
        },
      },
      boxShadow: {
        "berth-sm": "var(--berth-shadow-sm)",
        "berth-md": "var(--berth-shadow-md)",
        "berth-lg": "var(--berth-shadow-lg)",
        "berth-glow": "var(--berth-shadow-glow)",
      },
      borderRadius: {
        "berth-sm": "var(--berth-radius-sm)",
        "berth-md": "var(--berth-radius-md)",
        "berth-lg": "var(--berth-radius-lg)",
        "berth-xl": "var(--berth-radius-xl)",
      },
    },
  },
  plugins: [],
} satisfies Config;

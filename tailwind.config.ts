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
        runway: {
          bg: "var(--runway-bg)",
          "surface-0": "var(--runway-surface-0)",
          "surface-1": "var(--runway-surface-1)",
          "surface-2": "var(--runway-surface-2)",
          "surface-3": "var(--runway-surface-3)",
          "text-primary": "var(--runway-text-primary)",
          "text-secondary": "var(--runway-text-secondary)",
          "text-tertiary": "var(--runway-text-tertiary)",
          border: "var(--runway-border)",
          "border-subtle": "var(--runway-border-subtle)",
          "border-strong": "var(--runway-border-strong)",
          accent: "var(--runway-accent)",
          "accent-light": "var(--runway-accent-light)",
          "accent-dark": "var(--runway-accent-dark)",
          "accent-bg": "var(--runway-accent-bg)",
          "accent-border": "var(--runway-accent-border)",
          success: "var(--runway-success)",
          "success-bg": "var(--runway-success-bg)",
          warning: "var(--runway-warning)",
          "warning-bg": "var(--runway-warning-bg)",
          error: "var(--runway-error)",
          "error-bg": "var(--runway-error-bg)",
        },
      },
      boxShadow: {
        "runway-sm": "var(--runway-shadow-sm)",
        "runway-md": "var(--runway-shadow-md)",
        "runway-lg": "var(--runway-shadow-lg)",
        "runway-glow": "var(--runway-shadow-glow)",
      },
      borderRadius: {
        "runway-sm": "var(--runway-radius-sm)",
        "runway-md": "var(--runway-radius-md)",
        "runway-lg": "var(--runway-radius-lg)",
        "runway-xl": "var(--runway-radius-xl)",
      },
    },
  },
  plugins: [],
} satisfies Config;

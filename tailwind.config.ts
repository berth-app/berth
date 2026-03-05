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
          surface: "var(--runway-surface)",
          border: "var(--runway-border)",
          text: "var(--runway-text)",
          muted: "var(--runway-muted)",
          accent: "var(--runway-accent)",
          success: "var(--runway-success)",
          error: "var(--runway-error)",
        },
      },
    },
  },
  plugins: [],
} satisfies Config;

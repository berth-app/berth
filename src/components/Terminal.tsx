import { useEffect, useRef } from "react";
import { Terminal as XTerm } from "@xterm/xterm";
import { FitAddon } from "@xterm/addon-fit";
import { WebLinksAddon } from "@xterm/addon-web-links";
import "@xterm/xterm/css/xterm.css";

interface Props {
  logs: Array<{ stream: string; text: string }>;
  flush?: boolean;
}

export default function Terminal({ logs, flush }: Props) {
  const containerRef = useRef<HTMLDivElement>(null);
  const termRef = useRef<XTerm | null>(null);
  const fitRef = useRef<FitAddon | null>(null);
  const writtenRef = useRef(0);

  useEffect(() => {
    if (!containerRef.current) return;

    const term = new XTerm({
      fontFamily: "'SF Mono', Menlo, Monaco, Consolas, monospace",
      fontSize: 12,
      lineHeight: 1.4,
      cursorStyle: "bar",
      cursorBlink: false,
      disableStdin: true,
      scrollback: 10000,
      convertEol: true,
      theme: {
        background: "#232326",
        foreground: "#f5f5f7",
        cursor: "#f5f5f7",
        selectionBackground: "#0a84ff44",
        black: "#1a1a1c",
        red: "#ff453a",
        green: "#30d158",
        yellow: "#ffd60a",
        blue: "#0a84ff",
        magenta: "#bf5af2",
        cyan: "#64d2ff",
        white: "#f5f5f7",
        brightBlack: "#636366",
        brightRed: "#ff6961",
        brightGreen: "#4ade80",
        brightYellow: "#ffe566",
        brightBlue: "#409cff",
        brightMagenta: "#da8fff",
        brightCyan: "#99e9f2",
        brightWhite: "#ffffff",
      },
    });

    const fit = new FitAddon();
    term.loadAddon(fit);
    term.loadAddon(new WebLinksAddon());
    term.open(containerRef.current);
    fit.fit();

    termRef.current = term;
    fitRef.current = fit;
    writtenRef.current = 0;

    const observer = new ResizeObserver(() => fit.fit());
    observer.observe(containerRef.current);

    return () => {
      observer.disconnect();
      term.dispose();
      termRef.current = null;
      fitRef.current = null;
    };
  }, []);

  useEffect(() => {
    const term = termRef.current;
    if (!term) return;

    for (let i = writtenRef.current; i < logs.length; i++) {
      const line = logs[i];
      if (line.stream === "stderr") {
        term.write(`\x1b[31m${line.text}\x1b[0m\r\n`);
      } else {
        term.write(`${line.text}\r\n`);
      }
    }
    writtenRef.current = logs.length;
  }, [logs]);

  return (
    <div
      ref={containerRef}
      className={`flex-1 overflow-hidden ${flush ? "" : "rounded-berth-lg glass-card-static"}`}
    />
  );
}

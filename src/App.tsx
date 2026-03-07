import { useState, useEffect } from "react";
import { listen } from "@tauri-apps/api/event";
import ProjectList from "./pages/ProjectList";
import ProjectDetail from "./pages/ProjectDetail";
import PasteAndDeploy from "./pages/PasteAndDeploy";
import Targets from "./pages/Targets";
import Settings, { applyTheme } from "./pages/Settings";
import { ToastProvider } from "./components/Toast";
import { getSettings } from "./lib/invoke";

type View = "list" | "detail" | "paste" | "targets" | "settings";

export default function App() {
  const [view, setView] = useState<View>("list");
  const [selectedProjectId, setSelectedProjectId] = useState<string | null>(null);

  // Apply theme on startup
  useEffect(() => {
    getSettings()
      .then((s) => applyTheme(s.theme ?? "system"))
      .catch(() => {});
  }, []);

  // Listen for tray navigation events
  useEffect(() => {
    const unlistenNav = listen<string>("navigate", (event) => {
      setView(event.payload as View);
    });
    const unlistenProject = listen<string>("navigate-project", (event) => {
      setSelectedProjectId(event.payload);
      setView("detail");
    });
    return () => {
      unlistenNav.then((fn) => fn());
      unlistenProject.then((fn) => fn());
    };
  }, []);

  return (
    <ToastProvider>
      <div className="h-screen flex flex-col">
        {/* Titlebar drag region */}
        <div
          data-tauri-drag-region
          className="h-8 flex items-center justify-center bg-runway-surface border-b border-runway-border shrink-0"
        >
          <span className="text-xs font-medium text-runway-muted">Runway</span>
        </div>

        {/* Main content */}
        <div className="flex-1 overflow-hidden">
          {view === "list" && (
            <ProjectList
              onSelect={(id) => {
                setSelectedProjectId(id);
                setView("detail");
              }}
              onNewProject={() => setView("paste")}
              onTargets={() => setView("targets")}
              onSettings={() => setView("settings")}
            />
          )}
          {view === "detail" && selectedProjectId && (
            <ProjectDetail
              projectId={selectedProjectId}
              onBack={() => setView("list")}
            />
          )}
          {view === "paste" && (
            <PasteAndDeploy
              onBack={() => setView("list")}
              onCreated={(id) => {
                setSelectedProjectId(id);
                setView("detail");
              }}
            />
          )}
          {view === "targets" && (
            <Targets onBack={() => setView("list")} />
          )}
          {view === "settings" && (
            <Settings onBack={() => setView("list")} />
          )}
        </div>
      </div>
    </ToastProvider>
  );
}

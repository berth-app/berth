import { useState } from "react";
import ProjectList from "./pages/ProjectList";
import ProjectDetail from "./pages/ProjectDetail";
import PasteAndDeploy from "./pages/PasteAndDeploy";
import Targets from "./pages/Targets";
import { ToastProvider } from "./components/Toast";

type View = "list" | "detail" | "paste" | "targets";

export default function App() {
  const [view, setView] = useState<View>("list");
  const [selectedProjectId, setSelectedProjectId] = useState<string | null>(null);

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
        </div>
      </div>
    </ToastProvider>
  );
}

import { useState } from "react";
import ProjectList from "./pages/ProjectList";
import ProjectDetail from "./pages/ProjectDetail";
import PasteAndDeploy from "./pages/PasteAndDeploy";

type View = "list" | "detail" | "paste";

export default function App() {
  const [view, setView] = useState<View>("list");
  const [selectedProjectId, setSelectedProjectId] = useState<string | null>(null);

  return (
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
      </div>
    </div>
  );
}

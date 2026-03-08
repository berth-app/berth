import { useState, useEffect } from "react";
import { listen } from "@tauri-apps/api/event";
import ProjectList from "./pages/ProjectList";
import ProjectDetail from "./pages/ProjectDetail";
import PasteAndDeploy from "./pages/PasteAndDeploy";
import Targets from "./pages/Targets";
import Settings from "./pages/Settings";
import { ToastProvider } from "./components/Toast";
import Sidebar from "./components/Sidebar";
import { getSettings } from "./lib/invoke";
import { setTheme, initThemeListener } from "./lib/theme";

type View = "list" | "detail" | "paste" | "targets" | "settings";

export default function App() {
  const [view, setView] = useState<View>("list");
  const [selectedProjectId, setSelectedProjectId] = useState<string | null>(
    null
  );

  useEffect(() => {
    getSettings()
      .then((s) =>
        setTheme(s.theme_palette ?? "default", s.theme ?? "system")
      )
      .catch(() => {});
    initThemeListener();
  }, []);

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

  function handleSelectProject(id: string) {
    setSelectedProjectId(id);
    setView("detail");
  }

  return (
    <ToastProvider>
      <div className="h-screen flex">
        <Sidebar
          view={view}
          setView={setView}
          selectedProjectId={selectedProjectId}
          onSelectProject={handleSelectProject}
          onNewProject={() => setView("paste")}
        />
        <main className="flex-1 overflow-hidden animate-page-enter bg-berth-bg">
          {view === "list" && (
            <ProjectList
              onSelect={handleSelectProject}
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
          {view === "targets" && <Targets />}
          {view === "settings" && <Settings />}
        </main>
      </div>
    </ToastProvider>
  );
}

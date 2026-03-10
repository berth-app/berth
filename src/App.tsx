import { useState, useEffect } from "react";
import { listen } from "@tauri-apps/api/event";
import ProjectList from "./pages/ProjectList";
import ProjectDetail from "./pages/ProjectDetail";
import PasteAndDeploy from "./pages/PasteAndDeploy";
import Targets from "./pages/Targets";
import Settings from "./pages/Settings";
import TemplateStore from "./pages/TemplateStore";
import Onboarding from "./pages/Onboarding";
import { ToastProvider } from "./components/Toast";
import Sidebar from "./components/Sidebar";
import { getSettings, updateSetting } from "./lib/invoke";
import { setTheme, initThemeListener } from "./lib/theme";

type View = "list" | "detail" | "paste" | "targets" | "settings" | "store";

export default function App() {
  const [view, setView] = useState<View>("list");
  const [selectedProjectId, setSelectedProjectId] = useState<string | null>(
    null
  );
  const [showOnboarding, setShowOnboarding] = useState<boolean | null>(null);

  useEffect(() => {
    getSettings()
      .then((s) => {
        setTheme(s.theme_palette ?? "default", s.theme ?? "system");
        if (s.onboarding_completed !== "true") {
          setShowOnboarding(true);
        } else {
          setShowOnboarding(false);
        }
      })
      .catch(() => {
        setShowOnboarding(true);
      });
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

  async function handleOnboardingComplete(deployedProjectId?: string) {
    await updateSetting("onboarding_completed", "true").catch(() => {});
    setShowOnboarding(false);
    if (deployedProjectId) {
      setSelectedProjectId(deployedProjectId);
      setView("detail");
    }
  }

  // Don't render until we know whether to show onboarding
  if (showOnboarding === null) return null;

  if (showOnboarding) {
    return (
      <ToastProvider>
        <Onboarding onComplete={handleOnboardingComplete} />
      </ToastProvider>
    );
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
          {view === "store" && (
            <TemplateStore
              onInstalled={(id) => {
                setSelectedProjectId(id);
                setView("detail");
              }}
            />
          )}
          {view === "settings" && <Settings />}
        </main>
      </div>
    </ToastProvider>
  );
}

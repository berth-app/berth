import { useEffect, useState } from "react";
import { listProjects, type Project } from "../lib/invoke";

interface Props {
  onSelect: (id: string) => void;
  onNewProject: () => void;
}

const STATUS_COLORS: Record<string, string> = {
  idle: "bg-runway-muted",
  running: "bg-runway-success",
  stopped: "bg-runway-border",
  failed: "bg-runway-error",
};

export default function ProjectList({ onSelect, onNewProject }: Props) {
  const [projects, setProjects] = useState<Project[]>([]);
  const [loading, setLoading] = useState(true);

  useEffect(() => {
    listProjects()
      .then(setProjects)
      .catch(console.error)
      .finally(() => setLoading(false));
  }, []);

  if (loading) {
    return (
      <div className="flex items-center justify-center h-full text-runway-muted">
        Loading...
      </div>
    );
  }

  if (projects.length === 0) {
    return (
      <div className="flex flex-col items-center justify-center h-full gap-4">
        <div className="text-lg font-medium text-runway-text">
          No projects yet
        </div>
        <p className="text-sm text-runway-muted max-w-xs text-center">
          Paste code from Claude Code, Cursor, or any AI tool and deploy it
          anywhere.
        </p>
        <button
          onClick={onNewProject}
          className="px-4 py-2 rounded-lg bg-runway-accent text-white text-sm font-medium hover:opacity-90 transition-opacity"
        >
          Paste &amp; Deploy
        </button>
      </div>
    );
  }

  return (
    <div className="h-full flex flex-col">
      <div className="flex items-center justify-between px-4 py-3 border-b border-runway-border">
        <h1 className="text-sm font-semibold">Projects</h1>
        <button
          onClick={onNewProject}
          className="px-3 py-1.5 rounded-md bg-runway-accent text-white text-xs font-medium hover:opacity-90 transition-opacity"
        >
          + New
        </button>
      </div>
      <div className="flex-1 overflow-y-auto">
        {projects.map((project) => (
          <button
            key={project.id}
            onClick={() => onSelect(project.id)}
            className="w-full flex items-center gap-3 px-4 py-3 border-b border-runway-border hover:bg-runway-surface transition-colors text-left"
          >
            <div
              className={`w-2 h-2 rounded-full shrink-0 ${STATUS_COLORS[project.status] ?? STATUS_COLORS.idle}`}
            />
            <div className="flex-1 min-w-0">
              <div className="text-sm font-medium truncate">{project.name}</div>
              <div className="text-xs text-runway-muted truncate">
                {project.runtime} &middot; {project.path}
              </div>
            </div>
            <div className="text-xs text-runway-muted">{project.status}</div>
          </button>
        ))}
      </div>
    </div>
  );
}

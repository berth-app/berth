interface Props {
  projectId: string;
  onBack: () => void;
}

export default function ProjectDetail({ projectId, onBack }: Props) {
  return (
    <div className="h-full flex flex-col">
      <div className="flex items-center gap-3 px-4 py-3 border-b border-runway-border">
        <button
          onClick={onBack}
          className="text-runway-accent text-sm hover:underline"
        >
          &larr; Back
        </button>
        <h1 className="text-sm font-semibold">Project Detail</h1>
      </div>

      <div className="flex-1 flex flex-col gap-4 p-4">
        {/* Status bar */}
        <div className="flex items-center gap-2 px-3 py-2 rounded-lg bg-runway-surface">
          <div className="w-2 h-2 rounded-full bg-runway-muted" />
          <span className="text-sm">Idle</span>
          <span className="text-xs text-runway-muted ml-auto">
            ID: {projectId.slice(0, 8)}...
          </span>
        </div>

        {/* Actions */}
        <div className="flex gap-2">
          <button className="px-4 py-2 rounded-lg bg-runway-accent text-white text-sm font-medium hover:opacity-90 transition-opacity">
            Run
          </button>
          <button className="px-4 py-2 rounded-lg bg-runway-surface text-runway-text text-sm border border-runway-border hover:bg-runway-border transition-colors">
            Stop
          </button>
        </div>

        {/* Log viewer placeholder */}
        <div className="flex-1 rounded-lg bg-runway-surface border border-runway-border p-3 font-mono text-xs overflow-y-auto">
          <div className="text-runway-muted">
            Logs will appear here when the project is running.
          </div>
          <div className="text-runway-muted mt-1">
            xterm.js integration coming in Phase 1 iteration.
          </div>
        </div>
      </div>
    </div>
  );
}

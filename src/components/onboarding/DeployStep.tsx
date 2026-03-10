import { useState } from "react";
import { Check, Clipboard, Bot, Store } from "lucide-react";
import { savePasteCode, createProject, runProject } from "../../lib/invoke";
import { useToast } from "../Toast";

interface Props {
  onNext: () => void;
  onDeployed: (projectId: string) => void;
}

const DEMO_CODE = `import platform
import datetime

print("=" * 40)
print("  Hello from Berth!")
print("=" * 40)
print()
print(f"  Host:    {platform.node()}")
print(f"  Python:  {platform.python_version()}")
print(f"  OS:      {platform.system()} {platform.release()}")
print(f"  Time:    {datetime.datetime.now().strftime('%H:%M:%S')}")
print()
print("  Your first project is running.")
print("  Edit the code, add env vars, or")
print("  schedule it from the project view.")
print()
print("=" * 40)
`;

export default function DeployStep({ onNext, onDeployed }: Props) {
  const [creating, setCreating] = useState(false);
  const [deployed, setDeployed] = useState(false);
  const { toast } = useToast();

  async function handleDeploy() {
    setCreating(true);
    try {
      const projectPath = await savePasteCode("hello-world", DEMO_CODE);
      const project = await createProject("hello-world", projectPath);
      if (project.entrypoint) {
        await runProject(project.id);
      }
      setDeployed(true);
      onDeployed(project.id);
    } catch (e) {
      toast(`Failed to create project: ${e}`, "error");
    } finally {
      setCreating(false);
    }
  }

  if (deployed) {
    return (
      <div className="flex flex-col items-center text-center max-w-md">
        <div className="w-14 h-14 rounded-full bg-berth-success/15 flex items-center justify-center mb-6">
          <Check size={28} className="text-berth-success" />
        </div>
        <h2 className="text-xl font-semibold text-berth-text-primary mb-2">
          Project Created
        </h2>
        <p className="text-sm text-berth-text-secondary mb-6">
          "hello-world" is running locally. You can view its logs after setup.
        </p>

        <div className="glass-card-static w-full p-4 mb-8 text-left">
          <div className="text-[11px] font-semibold text-berth-text-tertiary uppercase tracking-wider mb-3">
            What's next
          </div>
          <div className="flex flex-col gap-3">
            <div className="flex items-start gap-3">
              <Clipboard size={16} className="text-berth-accent mt-0.5 shrink-0" />
              <div>
                <div className="text-sm text-berth-text-primary">Paste & Deploy</div>
                <div className="text-xs text-berth-text-tertiary">
                  Paste code from Claude Code, Cursor, or any AI tool
                </div>
              </div>
            </div>
            <div className="flex items-start gap-3">
              <Bot size={16} className="text-berth-accent mt-0.5 shrink-0" />
              <div>
                <div className="text-sm text-berth-text-primary">MCP Server</div>
                <div className="text-xs text-berth-text-tertiary">
                  Connect Claude Code to deploy and monitor via AI
                </div>
              </div>
            </div>
            <div className="flex items-start gap-3">
              <Store size={16} className="text-berth-accent mt-0.5 shrink-0" />
              <div>
                <div className="text-sm text-berth-text-primary">Template Store</div>
                <div className="text-xs text-berth-text-tertiary">
                  Browse ready-made projects to install in one click
                </div>
              </div>
            </div>
          </div>
        </div>

        <button onClick={onNext} className="btn btn-primary btn-lg">
          Continue
        </button>
      </div>
    );
  }

  return (
    <div className="flex flex-col items-center text-center max-w-md">
      <h2 className="text-xl font-semibold text-berth-text-primary mb-2">
        Your First Project
      </h2>
      <p className="text-sm text-berth-text-secondary mb-6">
        We'll create a sample project to show you how Berth works.
      </p>

      <div className="glass-card-static w-full p-4 mb-6 text-left">
        <div className="flex items-center justify-between mb-2">
          <span className="text-xs font-medium text-berth-text-secondary">
            hello-world.py
          </span>
          <span className="badge badge-accent">Python</span>
        </div>
        <pre className="text-[11px] font-mono text-berth-text-secondary leading-relaxed overflow-x-auto whitespace-pre">
{`print("Hello from Berth!")
print(f"Host:   {platform.node()}")
print(f"Python: {platform.python_version()}")
print(f"Time:   {datetime.now():%H:%M:%S}")`}
        </pre>
      </div>

      <button
        onClick={handleDeploy}
        disabled={creating}
        className="btn btn-primary btn-lg w-full"
      >
        {creating ? (
          <span className="flex items-center gap-2">
            <div className="w-3 h-3 border-2 border-white/30 border-t-white rounded-full animate-spin" />
            Creating project...
          </span>
        ) : (
          "Create & Run"
        )}
      </button>

      <button onClick={onNext} className="btn btn-ghost text-sm mt-3">
        Skip
      </button>
    </div>
  );
}

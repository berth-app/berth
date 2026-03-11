import { Rocket, Activity, Bot } from "lucide-react";

interface Props {
  onNext: () => void;
}

const features = [
  {
    icon: Rocket,
    title: "Deploy",
    description:
      "Paste code or select a directory. Berth detects the runtime and runs it instantly.",
  },
  {
    icon: Activity,
    title: "Monitor",
    description:
      "Live logs, CPU/memory stats, cron scheduling, and macOS notifications.",
  },
  {
    icon: Bot,
    title: "AI-Controlled",
    description:
      "Built-in MCP server lets Claude Code deploy, monitor, and manage for you.",
  },
];

export default function FeaturesStep({ onNext }: Props) {
  return (
    <div className="flex flex-col items-center w-full max-w-2xl">
      <h2 className="text-xl font-semibold text-berth-text-primary mb-2">
        What Berth Does
      </h2>
      <p className="text-sm text-berth-text-secondary mb-8">
        A deployment control plane for AI-generated code
      </p>

      <div className="grid grid-cols-3 gap-4 w-full mb-10">
        {features.map((f) => (
          <div
            key={f.title}
            className="glass-card-static p-5 flex flex-col items-center text-center"
          >
            <div className="w-10 h-10 rounded-berth-md bg-berth-accent/10 flex items-center justify-center mb-3">
              <f.icon size={20} className="text-berth-accent" />
            </div>
            <h3 className="text-sm font-semibold text-berth-text-primary mb-1.5">
              {f.title}
            </h3>
            <p className="text-xs text-berth-text-secondary leading-relaxed">
              {f.description}
            </p>
          </div>
        ))}
      </div>

      <button onClick={onNext} className="btn btn-primary btn-lg">
        Continue
      </button>
    </div>
  );
}

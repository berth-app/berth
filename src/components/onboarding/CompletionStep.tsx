import { Check, X } from "lucide-react";

interface CompletedItems {
  nats: boolean;
  target: boolean;
  deploy: boolean;
}

interface Props {
  completedItems: CompletedItems;
  onEnter: () => void;
}

const items = [
  { key: "nats" as const, label: "NATS credentials configured" },
  { key: "target" as const, label: "Remote agent paired" },
  { key: "deploy" as const, label: "Sample project created" },
];

export default function CompletionStep({ completedItems, onEnter }: Props) {
  const anyCompleted = Object.values(completedItems).some(Boolean);

  return (
    <div className="flex flex-col items-center text-center max-w-md">
      <h2 className="text-2xl font-bold text-berth-text-primary mb-2 onboarding-celebration">
        You're all set!
      </h2>
      <p className="text-sm text-berth-text-secondary mb-8">
        {anyCompleted
          ? "Here's what you configured during setup."
          : "You can configure everything from the app whenever you're ready."}
      </p>

      <div className="glass-card-static w-full p-4 mb-8">
        <div className="flex flex-col gap-3">
          {items.map((item) => (
            <div key={item.key} className="flex items-center gap-3">
              {completedItems[item.key] ? (
                <div className="w-5 h-5 rounded-full bg-berth-success/15 flex items-center justify-center flex-shrink-0">
                  <Check size={12} className="text-berth-success" />
                </div>
              ) : (
                <div className="w-5 h-5 rounded-full bg-berth-surface-2 flex items-center justify-center flex-shrink-0">
                  <X size={10} className="text-berth-text-tertiary" />
                </div>
              )}
              <span
                className={`text-sm ${
                  completedItems[item.key]
                    ? "text-berth-text-primary"
                    : "text-berth-text-tertiary"
                }`}
              >
                {item.label}
              </span>
            </div>
          ))}
        </div>
      </div>

      <button onClick={onEnter} className="btn btn-primary btn-lg px-10">
        Enter Berth
      </button>
    </div>
  );
}

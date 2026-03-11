import { BarChart3 } from "lucide-react";
import { setTelemetryEnabled } from "../../lib/invoke";

interface Props {
  onNext: () => void;
}

export default function TelemetryStep({ onNext }: Props) {
  async function handleEnable() {
    try {
      await setTelemetryEnabled(true);
    } catch {
      // Non-blocking — continue even if this fails
    }
    onNext();
  }

  return (
    <div className="flex flex-col items-center max-w-sm">
      <div className="w-14 h-14 rounded-berth-lg bg-berth-accent/10 flex items-center justify-center mb-5">
        <BarChart3 size={28} className="text-berth-accent" />
      </div>

      <h2 className="text-xl font-semibold text-berth-text-primary mb-2">
        Help Improve Berth
      </h2>

      <p className="text-sm text-berth-text-secondary text-center mb-2 leading-relaxed">
        Share anonymous usage statistics to help us improve the app.
      </p>

      <p className="text-xs text-berth-text-tertiary text-center mb-8 leading-relaxed">
        No personal data is ever collected. No file paths, project names,
        code content, or environment variables. You can change this
        anytime in Settings.
      </p>

      <div className="flex flex-col gap-2 w-full">
        <button onClick={handleEnable} className="btn btn-primary btn-lg w-full">
          Enable Analytics
        </button>
        <button
          onClick={onNext}
          className="btn btn-ghost btn-lg w-full text-berth-text-secondary"
        >
          No Thanks
        </button>
      </div>
    </div>
  );
}

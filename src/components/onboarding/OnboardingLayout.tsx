import { ArrowLeft } from "lucide-react";
import StepIndicator from "./StepIndicator";

interface Props {
  step: number;
  totalSteps: number;
  onBack?: () => void;
  onSkip: () => void;
  animDirection: "forward" | "backward";
  children: React.ReactNode;
  wide?: boolean;
}

export default function OnboardingLayout({
  step,
  totalSteps,
  onBack,
  onSkip,
  animDirection,
  children,
  wide,
}: Props) {
  return (
    <div className="h-screen w-screen flex flex-col bg-berth-bg">
      {/* Top bar — back button */}
      <div className="h-12 flex items-center px-5 shrink-0" data-tauri-drag-region>
        {onBack && (
          <button onClick={onBack} className="btn btn-ghost btn-sm gap-1">
            <ArrowLeft size={14} />
            Back
          </button>
        )}
      </div>

      {/* Main content */}
      <div className="flex-1 flex items-center justify-center px-5 overflow-y-auto">
        <div
          key={step}
          className={`${wide ? "w-full max-w-2xl" : "w-full max-w-lg"} flex flex-col items-center ${
            animDirection === "forward"
              ? "onboarding-step-enter-forward"
              : "onboarding-step-enter-backward"
          }`}
        >
          {children}
        </div>
      </div>

      {/* Bottom bar — dots + skip */}
      <div className="h-16 flex items-center justify-center gap-6 shrink-0">
        <StepIndicator current={step} total={totalSteps} />
        <button
          onClick={onSkip}
          className="text-xs text-berth-text-tertiary hover:text-berth-text-secondary transition-colors"
        >
          Skip setup
        </button>
      </div>
    </div>
  );
}

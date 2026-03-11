import { useState, useCallback } from "react";
import OnboardingLayout from "../components/onboarding/OnboardingLayout";
import WelcomeStep from "../components/onboarding/WelcomeStep";
import FeaturesStep from "../components/onboarding/FeaturesStep";
import NatsStep from "../components/onboarding/NatsStep";
import TargetStep from "../components/onboarding/TargetStep";
import DeployStep from "../components/onboarding/DeployStep";
import TelemetryStep from "../components/onboarding/TelemetryStep";
import CompletionStep from "../components/onboarding/CompletionStep";
import type { PairingResult } from "../lib/invoke";

interface Props {
  onComplete: (deployedProjectId?: string) => void;
}

const TOTAL_STEPS = 6;
const COMPLETION_STEP = 6;

export default function Onboarding({ onComplete }: Props) {
  const [step, setStep] = useState(0);
  const [animDirection, setAnimDirection] = useState<"forward" | "backward">(
    "forward"
  );
  const [completedItems, setCompletedItems] = useState({
    nats: false,
    target: false,
    deploy: false,
    deployedProjectId: null as string | null,
  });

  const goForward = useCallback(() => {
    setAnimDirection("forward");
    setStep((s) => s + 1);
  }, []);

  const goBack = useCallback(() => {
    setAnimDirection("backward");
    setStep((s) => Math.max(0, s - 1));
  }, []);

  const goToCompletion = useCallback(() => {
    setAnimDirection("forward");
    setStep(COMPLETION_STEP);
  }, []);

  function handleNatsConfigured(success: boolean) {
    setCompletedItems((prev) => ({ ...prev, nats: success }));
  }

  function handlePaired(_result: PairingResult) {
    setCompletedItems((prev) => ({ ...prev, target: true }));
  }

  const TELEMETRY_STEP = 5;

  function handleDeployed(projectId: string) {
    setCompletedItems((prev) => ({
      ...prev,
      deploy: true,
      deployedProjectId: projectId,
    }));
    // Auto-advance to telemetry opt-in after deploy
    setAnimDirection("forward");
    setStep(TELEMETRY_STEP);
  }

  function handleEnter() {
    onComplete(completedItems.deployedProjectId ?? undefined);
  }

  // Completion screen — no layout wrapper (no skip/back/dots)
  if (step === COMPLETION_STEP) {
    return (
      <div className="h-screen w-screen flex items-center justify-center bg-berth-bg">
        <div className="onboarding-step-enter-forward">
          <CompletionStep
            completedItems={completedItems}
            onEnter={handleEnter}
          />
        </div>
      </div>
    );
  }

  const showBack = step > 0;

  return (
    <OnboardingLayout
      step={step}
      totalSteps={TOTAL_STEPS}
      onBack={showBack ? goBack : undefined}
      onSkip={goToCompletion}
      animDirection={animDirection}
      wide={step === 1}
    >
      {step === 0 && <WelcomeStep onNext={goForward} />}
      {step === 1 && <FeaturesStep onNext={goForward} />}
      {step === 2 && (
        <NatsStep onNext={goForward} onConfigured={handleNatsConfigured} />
      )}
      {step === 3 && (
        <TargetStep onNext={goForward} onPaired={handlePaired} />
      )}
      {step === 4 && (
        <DeployStep onNext={goForward} onDeployed={handleDeployed} />
      )}
      {step === 5 && <TelemetryStep onNext={goForward} />}
    </OnboardingLayout>
  );
}

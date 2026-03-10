import { Anchor } from "lucide-react";

interface Props {
  onNext: () => void;
}

export default function WelcomeStep({ onNext }: Props) {
  return (
    <div className="flex flex-col items-center text-center">
      <div className="w-20 h-20 rounded-[22px] bg-berth-accent/15 flex items-center justify-center mb-8">
        <Anchor size={40} className="text-berth-accent" />
      </div>

      <h1 className="text-3xl font-bold text-berth-text-primary mb-3 tracking-tight">
        Welcome to Berth
      </h1>

      <p className="text-lg text-berth-text-secondary mb-2">
        Paste code. Pick a target. It's running.
      </p>

      <p className="text-sm text-berth-text-tertiary max-w-sm mb-10">
        Deploy and manage AI-generated code to local machines, remote servers,
        and cloud targets — all from one app.
      </p>

      <button onClick={onNext} className="btn btn-primary btn-lg px-10">
        Get Started
      </button>
    </div>
  );
}

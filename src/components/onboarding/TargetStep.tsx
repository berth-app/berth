import { useState } from "react";
import { Check, Server, Link2 } from "lucide-react";
import { pairAgent, type PairingResult } from "../../lib/invoke";
import { useToast } from "../Toast";

interface Props {
  onNext: () => void;
  onPaired: (result: PairingResult) => void;
}

export default function TargetStep({ onNext, onPaired }: Props) {
  const [showPair, setShowPair] = useState(false);
  const [pairCode, setPairCode] = useState("");
  const [pairing, setPairing] = useState(false);
  const [pairStatus, setPairStatus] = useState<
    "idle" | "discovering" | "pairing"
  >("idle");
  const [pairResult, setPairResult] = useState<PairingResult | null>(null);
  const { toast } = useToast();

  async function handlePair() {
    if (pairCode.trim().length !== 8) return;
    setPairing(true);
    setPairResult(null);
    setPairStatus("discovering");
    try {
      const pairTimer = setTimeout(() => setPairStatus("pairing"), 3000);
      const result = await pairAgent(pairCode.trim());
      clearTimeout(pairTimer);
      setPairResult(result);
      onPaired(result);
      toast(`Paired with ${result.agent_hostname}`, "success");
    } catch (e) {
      toast(`Pairing failed: ${e}`, "error");
    } finally {
      setPairing(false);
      setPairStatus("idle");
    }
  }

  if (pairResult) {
    return (
      <div className="flex flex-col items-center text-center max-w-md">
        <div className="w-14 h-14 rounded-full bg-berth-success/15 flex items-center justify-center mb-6">
          <Check size={28} className="text-berth-success" />
        </div>
        <h2 className="text-xl font-semibold text-berth-text-primary mb-2">
          Agent Paired
        </h2>
        <div className="glass-card-static p-4 w-full mb-8">
          <div className="grid grid-cols-2 gap-x-6 gap-y-1.5 text-xs">
            <div className="flex justify-between">
              <span className="text-berth-text-secondary">Host</span>
              <span className="text-berth-text-primary font-mono">
                {pairResult.agent_hostname}
              </span>
            </div>
            <div className="flex justify-between">
              <span className="text-berth-text-secondary">OS</span>
              <span className="text-berth-text-primary">
                {pairResult.agent_os}
              </span>
            </div>
            <div className="flex justify-between">
              <span className="text-berth-text-secondary">Version</span>
              <span className="text-berth-text-primary">
                v{pairResult.agent_version}
              </span>
            </div>
            <div className="flex justify-between">
              <span className="text-berth-text-secondary">Agent ID</span>
              <span className="text-berth-text-primary font-mono">
                {pairResult.agent_id}
              </span>
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
        Deploy Targets
      </h2>
      <p className="text-sm text-berth-text-secondary mb-8">
        Choose where your code runs. Local is always available — pair a remote
        agent to deploy to your server.
      </p>

      {/* Local target — always ready */}
      <div className="glass-card-static p-4 w-full mb-4 flex items-center gap-3">
        <div className="w-2 h-2 rounded-full bg-berth-success" />
        <div className="flex-1 text-left">
          <span className="text-sm font-medium text-berth-text-primary">
            Local
          </span>
          <span className="text-xs text-berth-text-secondary ml-2">
            127.0.0.1
          </span>
        </div>
        <span className="badge badge-success">Ready</span>
      </div>

      {!showPair ? (
        <div className="flex flex-col gap-3 w-full">
          <button
            onClick={() => setShowPair(true)}
            className="btn btn-secondary btn-lg w-full"
          >
            <Link2 size={14} />
            Pair a Remote Agent
          </button>
          <button onClick={onNext} className="btn btn-ghost text-sm">
            Just use local for now
          </button>
        </div>
      ) : (
        <div className="w-full">
          <div className="glass-card-static p-4 mb-4">
            <div className="flex items-center gap-2 mb-3">
              <Server size={14} className="text-berth-text-secondary" />
              <span className="text-sm font-medium text-berth-text-primary">
                Pair Remote Agent
              </span>
            </div>
            <p className="text-xs text-berth-text-secondary mb-3 text-left">
              Enter the 8-character pairing code shown on your agent.
            </p>
            <input
              type="text"
              value={pairCode}
              onChange={(e) =>
                setPairCode(
                  e.target.value
                    .toUpperCase()
                    .replace(/[^ABCDEFGHJKLMNPQRSTUVWXYZ23456789]/g, "")
                    .slice(0, 8)
                )
              }
              placeholder="K7M4XN2B"
              maxLength={8}
              className="input text-center text-lg font-mono tracking-[0.15em] uppercase mb-3"
              autoFocus
              onKeyDown={(e) => {
                if (e.key === "Enter") handlePair();
              }}
            />
            <button
              onClick={handlePair}
              disabled={pairCode.trim().length !== 8 || pairing}
              className="btn btn-primary w-full"
            >
              {pairing ? (
                <span className="flex items-center gap-2">
                  <div className="w-3 h-3 border-2 border-white/30 border-t-white rounded-full animate-spin" />
                  {pairStatus === "discovering"
                    ? "Discovering agent..."
                    : "Pairing..."}
                </span>
              ) : (
                "Pair"
              )}
            </button>
          </div>

          <button
            onClick={() => {
              setShowPair(false);
              setPairCode("");
            }}
            className="btn btn-ghost text-sm w-full"
          >
            Just use local for now
          </button>
        </div>
      )}
    </div>
  );
}

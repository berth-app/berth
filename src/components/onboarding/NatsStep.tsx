import { useState } from "react";
import { Check, ExternalLink } from "lucide-react";
import { saveNatsCredentials, getSettings } from "../../lib/invoke";
import { useToast } from "../Toast";

interface Props {
  onNext: () => void;
  onConfigured: (success: boolean) => void;
}

export default function NatsStep({ onNext, onConfigured }: Props) {
  const [showSetup, setShowSetup] = useState(false);
  const [credentials, setCredentials] = useState("");
  const [saving, setSaving] = useState(false);
  const [saved, setSaved] = useState(false);
  const { toast } = useToast();

  async function handleSave() {
    if (!credentials.trim()) return;
    setSaving(true);
    try {
      await saveNatsCredentials(credentials);
      // Verify it was saved
      const s = await getSettings();
      if (s.nats_creds) {
        setSaved(true);
        onConfigured(true);
        toast("NATS credentials saved", "success");
      }
    } catch (e) {
      toast(`Failed to save credentials: ${e}`, "error");
    } finally {
      setSaving(false);
    }
  }

  function handleSkip() {
    onNext();
  }

  if (saved) {
    return (
      <div className="flex flex-col items-center text-center max-w-md">
        <div className="w-14 h-14 rounded-full bg-berth-success/15 flex items-center justify-center mb-6">
          <Check size={28} className="text-berth-success" />
        </div>
        <h2 className="text-xl font-semibold text-berth-text-primary mb-2">
          NATS Connected
        </h2>
        <p className="text-sm text-berth-text-secondary mb-8">
          Your credentials are saved. You can manage remote agents through
          Synadia Cloud.
        </p>
        <button onClick={onNext} className="btn btn-primary btn-lg">
          Continue
        </button>
      </div>
    );
  }

  return (
    <div className="flex flex-col items-center text-center max-w-md">
      <h2 className="text-xl font-semibold text-berth-text-primary mb-2">
        Connect to Synadia Cloud
      </h2>
      <p className="text-sm text-berth-text-secondary mb-8">
        NATS relay enables communication with remote agents — no inbound ports
        needed. Free tier available.
      </p>

      {!showSetup ? (
        <div className="flex flex-col gap-3 w-full">
          <button
            onClick={() => setShowSetup(true)}
            className="btn btn-primary btn-lg w-full"
          >
            Set Up Now
          </button>
          <button
            onClick={handleSkip}
            className="btn btn-ghost text-sm"
          >
            I'll do this later
          </button>
          <p className="text-xs text-berth-text-tertiary">
            You can always configure NATS in Settings
          </p>
        </div>
      ) : (
        <div className="w-full text-left">
          <div className="glass-card-static p-4 mb-4">
            <div className="text-xs text-berth-text-secondary space-y-1 mb-4">
              <div className="font-medium text-berth-text-primary">Setup:</div>
              <ol className="list-decimal list-inside space-y-0.5 text-[11px]">
                <li>
                  Sign up at{" "}
                  <a
                    href="https://cloud.synadia.com"
                    target="_blank"
                    rel="noopener noreferrer"
                    className="text-berth-accent hover:underline inline-flex items-center gap-0.5"
                  >
                    cloud.synadia.com
                    <ExternalLink size={9} />
                  </a>
                </li>
                <li>Create an account and copy your credentials</li>
                <li>Paste the full credentials block below</li>
              </ol>
            </div>

            <textarea
              placeholder={
                "-----BEGIN NATS USER JWT-----\neyJ0eX...\n------END NATS USER JWT------\n\n-----BEGIN USER NKEY SEED-----\nSUANP...\n------END USER NKEY SEED------"
              }
              value={credentials}
              onChange={(e) => setCredentials(e.target.value)}
              rows={6}
              className="input !py-1.5 !text-sm w-full font-mono resize-none"
            />
            <div className="text-[10px] text-berth-error mt-1">
              These credentials are sensitive. Never share them.
            </div>
          </div>

          <div className="flex gap-3">
            <button
              onClick={() => setShowSetup(false)}
              className="btn btn-ghost flex-1"
            >
              Back
            </button>
            <button
              onClick={handleSave}
              disabled={!credentials.trim() || saving}
              className="btn btn-primary flex-1"
            >
              {saving ? "Saving..." : "Save Credentials"}
            </button>
          </div>

          <button
            onClick={handleSkip}
            className="btn btn-ghost text-xs w-full mt-2"
          >
            Skip for now
          </button>
        </div>
      )}
    </div>
  );
}

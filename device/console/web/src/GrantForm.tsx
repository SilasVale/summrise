import { useEffect, useState } from "react";
import { api } from "./api";
import { ToolPicker } from "./ToolPicker";
import { TOOL_NAMES, useToolNames } from "./useToolNames";

export function GrantForm({
  deviceId,
  onGranted
}: {
  deviceId: string;
  onGranted: (envelope: unknown, tools: string[]) => void;
}) {
  // Everything selected to begin with: the operator is the only user, the grant expires, and a
  // half-selected list is the reason a first attempt fails with "tool not permitted".
  const names = useToolNames();
  const [chosen, setChosen] = useState<Set<string>>(new Set(TOOL_NAMES));
  const [lifetime, setLifetime] = useState(3600);
  const [destructive, setDestructive] = useState(true);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);

  // The fallback list is already the initial selection. When the served catalogue replaces it,
  // everything it offers starts selected too; a failed or empty answer leaves the list unchanged, so
  // it leaves the operator's selection alone.
  useEffect(() => {
    setChosen(new Set(names));
  }, [names]);

  const mint = async () => {
    if (chosen.size === 0) {
      setError("Pick at least one tool.");
      return;
    }
    setBusy(true);
    setError(null);
    try {
      // The whole envelope goes to the device — the grant facts AND the console's signature over
      // them. Sending `envelope.grant` alone is a grant nobody signed, and the device says so.
      const envelope = await api.grant(deviceId, [...chosen], lifetime, destructive);
      onGranted(envelope, [...chosen]);
    } catch (cause) {
      setError(cause instanceof Error ? cause.message : String(cause));
    } finally {
      setBusy(false);
    }
  };

  return (
    <div className="grant">
      <div className="grant-row">
        <label htmlFor="lifetime">Lifetime (seconds)</label>
        <input id="lifetime" type="number" min={60} max={604800} value={lifetime} onChange={(event) => setLifetime(Number(event.target.value))} />
        <div className="grant-consent">
          <label className="check">
            <input type="checkbox" checked={destructive} onChange={(event) => setDestructive(event.target.checked)} />
            Allow destructive tools
          </label>
          {/* Unticking is a real policy, not a dead end: the device refuses a destructive call with
              a sentence that names the fix, the device's own audit records the ask, and the console's
              audit keeps the refused exchange. The console has no per-call approval, and the sentence
              must not imply one. */}
          {!destructive && (
            <p className="hint">
              Without this, the device refuses a destructive call. Its refusal asks for the grant to be
              minted again with this ticked, and the console's audit keeps the refused exchange
              with the device's error code. The console cannot approve one call at a time.
            </p>
          )}
        </div>
      </div>

      <ToolPicker names={names} chosen={chosen} onChange={setChosen} />

      <button type="button" className="primary" disabled={busy} onClick={() => void mint()}>
        {busy ? "Signing…" : "Grant access"}
      </button>
      {error && <p className="error">{error}</p>}
    </div>
  );
}

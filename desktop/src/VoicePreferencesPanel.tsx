// Y9-C — spoken preference rules, with an explicit Apply step.
//
// Say (or type) one rule. The polish model on this Mac turns it into a rule
// and a place; Yap shows it back — "I heard: never use exclamation marks. I'll
// apply it to: email" — and NOTHING changes until Apply. Every rule stays in
// the history below, including the ones you retract.
import { invoke } from "@tauri-apps/api/core";
import { useEffect, useState } from "react";
import { useAppCtx } from "./appShell";
import {
  describeFilter,
  heardLine,
  preferenceStatus,
  type VoicePreference,
} from "./voicePreferences";

export default function VoicePreferencesPanel() {
  const { toast, toastError } = useAppCtx();
  const [spoken, setSpoken] = useState("");
  const [busy, setBusy] = useState(false);
  const [proposal, setProposal] = useState<VoicePreference | null>(null);
  const [rejection, setRejection] = useState<string | null>(null);
  const [rows, setRows] = useState<VoicePreference[]>([]);

  async function refresh() {
    try {
      setRows(await invoke<VoicePreference[]>("list_voice_preferences"));
    } catch (e) {
      toastError(e);
    }
  }
  useEffect(() => {
    void refresh();
  }, []);

  async function derive() {
    if (!spoken.trim() || busy) return;
    setBusy(true);
    setRejection(null);
    setProposal(null);
    try {
      setProposal(await invoke<VoicePreference>("derive_voice_preference", { spoken: spoken.trim() }));
      setSpoken("");
      await refresh();
    } catch (e) {
      // Rejected and shown back — nothing was stored.
      setRejection(String(e));
    } finally {
      setBusy(false);
    }
  }
  async function apply(id: string) {
    try {
      await invoke("apply_voice_preference", { id });
      setProposal(null);
      toast("Rule applied — it takes effect from your next dictation");
      await refresh();
    } catch (e) {
      toastError(e);
    }
  }
  async function retract(id: string) {
    try {
      await invoke("retract_voice_preference", { id });
      if (proposal?.id === id) setProposal(null);
      await refresh();
    } catch (e) {
      toastError(e);
    }
  }

  return (
    <div className="panel" data-row data-voice-preferences>
      <h3>Rules you've said</h3>
      <div className="snip-add">
        <input
          placeholder="e.g. never use exclamation marks in email"
          value={spoken}
          onChange={(e) => setSpoken(e.target.value)}
          onKeyDown={(e) => {
            if (e.key === "Enter") void derive();
          }}
          aria-label="Say or type a rule"
          disabled={busy}
        />
        <button className="primary" onClick={() => void derive()} disabled={!spoken.trim() || busy}>
          {busy ? "Reading…" : "Read rule"}
        </button>
      </div>
      {rejection && (
        <p className="settings-hint" role="alert">
          {rejection}
        </p>
      )}
      {proposal && (
        <div className="profile-row" role="group" aria-label="Confirm rule">
          <p>{heardLine(proposal)}</p>
          <button className="primary" onClick={() => void apply(proposal.id)}>
            Apply
          </button>
          <button onClick={() => void retract(proposal.id)}>Discard</button>
        </div>
      )}
      <p className="settings-hint">
        Read on this Mac by your polish model. A rule changes nothing until you press Apply, and
        you can retract it any time — everything you've said stays listed here.
      </p>
      {rows.length > 0 && (
        <ul className="snip-list">
          {rows.map((p) => {
            const status = preferenceStatus(p);
            return (
              <li key={p.id} data-status={status}>
                <span>
                  “{p.spoken}” → {p.rule} <em>({describeFilter(p.filter)} · {status})</em>
                </span>
                {status === "active" && <button onClick={() => void retract(p.id)}>Retract</button>}
                {status === "pending" && (
                  <>
                    <button className="primary" onClick={() => void apply(p.id)}>
                      Apply
                    </button>
                    <button onClick={() => void retract(p.id)}>Discard</button>
                  </>
                )}
              </li>
            );
          })}
        </ul>
      )}
    </div>
  );
}

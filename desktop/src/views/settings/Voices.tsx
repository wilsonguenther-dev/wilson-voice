/**
 * yap-1106-fb-22b-who-was-this-cards — Settings → Voices (the Speakers list of
 * yap-1106-voiceprints, renamed "Voices" by the approved design). Every saved
 * voiceprint with Delete, plus "Forget all voices". A one-line consent note
 * shows the first time. Voiceprints never leave this Mac and are never sent
 * through Social; the webview never receives one (only names and counts).
 *
 * Lifecycle: loading, empty, error (with a retry), confirm before forgetting
 * everything, keyboard-reachable buttons only.
 */
import { invoke } from "@tauri-apps/api/core";
import { useEffect, useState } from "react";
import { useAppCtx } from "../../appShell";
import "../../meetings/meetings.css";

/** `speaker_profiles::SpeakerProfile` as the webview gets it (no voiceprint). */
export interface Voice {
  id: number;
  displayName: string;
  isMe: boolean;
  nSamples: number;
  lastHeardAt: string | null;
}

export const VOICES_CONSENT_KEY = "yap.voices.consentSeen";
export const VOICES_CONSENT = "Voiceprints stay on this Mac and can be deleted at any time";

/** "3 samples · last heard Nov 6" — what a row says about a voice. */
export function voiceDetail(v: Pick<Voice, "nSamples" | "lastHeardAt" | "isMe">): string {
  const parts: string[] = [];
  if (v.isMe) parts.push("Your voice, from dictation");
  parts.push(v.nSamples === 0 ? "Name only, no voiceprint yet" : `${v.nSamples} ${v.nSamples === 1 ? "sample" : "samples"}`);
  if (v.lastHeardAt) {
    const d = new Date(v.lastHeardAt);
    if (!Number.isNaN(d.getTime())) parts.push(`last heard ${d.toLocaleDateString(undefined, { month: "short", day: "numeric" })}`);
  }
  return parts.join(" · ");
}

function logVoices(event: string, fields: Record<string, string | number | boolean> = {}): void {
  const kv = Object.entries(fields)
    .map(([k, v]) => `${k}=${v}`)
    .join(" ");
  console.info(`settings voices ${event}${kv ? " " + kv : ""}`);
}

export default function Voices() {
  const { toast } = useAppCtx();
  const [voices, setVoices] = useState<Voice[] | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [busy, setBusy] = useState<number | "all" | null>(null);
  const [confirmAll, setConfirmAll] = useState(false);
  const [showConsent] = useState(() => {
    try {
      const seen = window.localStorage.getItem(VOICES_CONSENT_KEY) === "1";
      if (!seen) window.localStorage.setItem(VOICES_CONSENT_KEY, "1");
      return !seen;
    } catch {
      return true;
    }
  });

  async function load() {
    setError(null);
    try {
      const v = await invoke<Voice[]>("list_voices");
      setVoices(v);
      logVoices("list", { n: v.length });
    } catch (e) {
      setError(String(e));
      logVoices("list_failed");
    }
  }

  useEffect(() => {
    void load();
  }, []);

  async function remove(v: Voice) {
    setBusy(v.id);
    try {
      await invoke<boolean>("delete_voice", { id: v.id });
      setVoices((cur) => (cur ?? []).filter((x) => x.id !== v.id));
      logVoices("delete", { ok: true });
      toast(`Deleted ${v.displayName}'s voiceprint. Past meetings keep the name as text.`);
    } catch {
      logVoices("delete_failed");
      toast("Couldn't delete that voice. Try again.");
    } finally {
      setBusy(null);
    }
  }

  async function forgetAll() {
    setBusy("all");
    try {
      const n = await invoke<number>("forget_all_voices");
      setVoices([]);
      setConfirmAll(false);
      logVoices("forget_all", { n });
      toast(n === 1 ? "Forgot 1 voice." : `Forgot ${n} voices.`);
    } catch {
      logVoices("forget_all_failed");
      toast("Couldn't forget the voices. Try again.");
    } finally {
      setBusy(null);
    }
  }

  return (
    <section className="settings-panel" data-settings-pane="voices">
      <div className="panel" data-row>
        <h3>Voices</h3>
        <p className="tiny muted">
          Yap learns a voice when you name it after a meeting, so the next meeting labels that person without asking.
          Leaves this Mac: No.
        </p>
        {showConsent && (
          <p className="tiny" role="note" data-voices-consent>
            {VOICES_CONSENT}.
          </p>
        )}
        {error ? (
          <div role="alert" className="actions wrap">
            <span className="tiny">Couldn't read your saved voices.</span>
            <button type="button" onClick={() => void load()}>
              Try again
            </button>
          </div>
        ) : voices === null ? (
          <p className="tiny muted" aria-busy>
            Reading your saved voices…
          </p>
        ) : voices.length === 0 ? (
          <p className="tiny muted" data-empty-state="voices">
            No voices yet. Name someone on a "Who was this?" card after a meeting and they show up here.
          </p>
        ) : (
          <ul className="voices-list" aria-label="Saved voices">
            {voices.map((v) => (
              <li key={v.id} className="voices-row">
                <span className="voices-name">
                  <strong>{v.displayName}</strong>
                  <span className="tiny muted">{voiceDetail(v)}</span>
                </span>
                <button
                  type="button"
                  className="ghost"
                  disabled={busy !== null}
                  aria-label={`Delete ${v.displayName}'s voiceprint`}
                  onClick={() => void remove(v)}
                >
                  Delete
                </button>
              </li>
            ))}
          </ul>
        )}
        {voices && voices.length > 0 && (
          <div className="actions wrap">
            {confirmAll ? (
              <>
                <span className="tiny">Forget every saved voice? Past meetings keep their names as text.</span>
                <button type="button" className="danger" disabled={busy !== null} onClick={() => void forgetAll()}>
                  Forget all voices
                </button>
                <button type="button" className="ghost" disabled={busy !== null} onClick={() => setConfirmAll(false)}>
                  Keep them
                </button>
              </>
            ) : (
              <button type="button" className="ghost" disabled={busy !== null} onClick={() => setConfirmAll(true)}>
                Forget all voices
              </button>
            )}
          </div>
        )}
      </div>
    </section>
  );
}

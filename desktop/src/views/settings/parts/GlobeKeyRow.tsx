import { useCallback, useEffect, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import StatusDot from "../../../StatusDot";

/**
 * yap-1107-globe-key-conflict-guard — ONE plain line on the Permissions page
 * when the fn shortcut collides with what macOS does on a lone Fn tap (emoji
 * picker or macOS Dictation). "Turn it off" is the consent: only that press
 * runs the write (globe_key.rs `turn_off`), and "Put it back" restores the
 * value Yap recorded first. Mirrors `globe_key::GlobeStatus`.
 */
export type GlobeStatus = {
  pref: number | null;
  conflict: { kind: "emoji_picker" | "dictation"; pref: number | null; line: string } | null;
  line: string | null;
  canTurnOff: boolean;
  canPutBack: boolean;
  turnOffLabel: string;
  putBackLabel: string;
};

export type GlobeBusy = "turn_off" | "put_back" | null;

/** Pure view: nothing to say renders nothing. */
export function GlobeKeyRowView({
  status,
  busy,
  onTurnOff,
  onPutBack,
}: {
  status: GlobeStatus | null;
  busy: GlobeBusy;
  onTurnOff: () => void;
  onPutBack: () => void;
}) {
  if (!status?.line) return null;
  const ok = !status.conflict;
  return (
    <li data-row data-globe-key={ok ? "off" : "conflict"} className={ok ? "ok" : "bad"}>
      <StatusDot ok={ok} />
      <div>
        <p>{status.line}</p>
        {status.canTurnOff && (
          <button className="primary" disabled={busy !== null} onClick={onTurnOff}>
            {busy === "turn_off" ? "Turning it off…" : status.turnOffLabel}
          </button>
        )}
        {status.canPutBack && !status.canTurnOff && (
          <button disabled={busy !== null} onClick={onPutBack}>
            {busy === "put_back" ? "Putting it back…" : status.putBackLabel}
          </button>
        )}
      </div>
    </li>
  );
}

/** Reads on mount (read-only); writes only on a button press. */
export default function GlobeKeyRow({ toastError }: { toastError: (e: unknown) => void }) {
  const [status, setStatus] = useState<GlobeStatus | null>(null);
  const [busy, setBusy] = useState<GlobeBusy>(null);
  useEffect(() => {
    let live = true;
    invoke<GlobeStatus>("globe_key_status")
      .then((s) => live && setStatus(s))
      // A failed read hides the line: there is nothing the user can act on.
      .catch(() => live && setStatus(null));
    return () => {
      live = false;
    };
  }, []);
  const run = useCallback(
    async (which: Exclude<GlobeBusy, null>) => {
      setBusy(which);
      try {
        setStatus(
          await invoke<GlobeStatus>(which === "turn_off" ? "globe_key_turn_off" : "globe_key_put_back"),
        );
      } catch (e) {
        toastError(e);
      } finally {
        setBusy(null);
      }
    },
    [toastError],
  );
  return (
    <GlobeKeyRowView
      status={status}
      busy={busy}
      onTurnOff={() => void run("turn_off")}
      onPutBack={() => void run("put_back")}
    />
  );
}

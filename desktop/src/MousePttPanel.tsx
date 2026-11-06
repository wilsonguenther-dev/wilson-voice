/**
 * WF-19 — Settings › Shortcuts: a mouse button as push-to-talk or hands-free.
 *
 * The recorder binds the next middle / side press; left and right click say
 * why they can't (and are refused again in Rust on save); Esc or the
 * recorder button cancels. Its own component so the dev preview
 * (dev/mouse-ptt-preview.html) can render every state without the app.
 */
import { useEffect, useState } from "react";
import { adviceFor } from "./errors";
import type { AppSettings } from "./appTypes";
import {
  captureMouse, MOUSE_PRIMARY_REFUSED, mouseBindingLine, parseMouseBinding, toStored, withMode,
  type MouseMode,
} from "./mouseBinding";

export const MOUSE_CAPTURE_PROMPT = "Press the middle button or a side button (Esc to cancel).";

export default function MousePttPanel({
  settings,
  save,
  initialCapturing = false,
  initialHint = null,
}: {
  settings: AppSettings;
  save: (next: AppSettings) => Promise<void> | void;
  /** Dev preview only: open in the listening state. */
  initialCapturing?: boolean;
  /** Dev preview only: open showing this hint. */
  initialHint?: string | null;
}) {
  const [capturing, setCapturing] = useState(initialCapturing);
  const [hint, setHint] = useState<string | null>(initialHint);
  const [error, setError] = useState<string | null>(null);

  const saveMouse = async (pttMouseBinding: string) => {
    setError(null);
    try {
      await save({ ...settings, pttMouseBinding });
    } catch (e) {
      setError(adviceFor(e, "Yap could not save the mouse button.").line);
    }
  };

  const parsed = parseMouseBinding(settings.pttMouseBinding);
  const bound = parsed.ok && parsed.binding !== null;
  const mode: MouseMode = parsed.ok && parsed.binding ? parsed.binding.mode : "hold";

  useEffect(() => {
    if (!capturing) return;
    const stop = () => {
      setCapturing(false);
      setHint(null);
    };
    const onDown = (e: MouseEvent) => {
      if (e.button === 0) {
        // The recorder button's own click cancels; any other left click is a
        // press that can't be bound.
        const own = (e.target as Element | null)?.closest?.("[data-mouse-capture]");
        if (!own) setHint(MOUSE_PRIMARY_REFUSED);
        return;
      }
      e.preventDefault();
      e.stopPropagation();
      const r = captureMouse(e.button, mode);
      if (!r.ok) {
        setHint(r.why);
        return;
      }
      stop();
      void saveMouse(toStored(r.binding));
    };
    const block = (e: Event) => e.preventDefault();
    const onKey = (e: KeyboardEvent) => {
      if (e.code === "Escape") {
        e.preventDefault();
        stop();
      }
    };
    window.addEventListener("mousedown", onDown, true);
    window.addEventListener("auxclick", block, true);
    window.addEventListener("contextmenu", block, true);
    window.addEventListener("keydown", onKey, true);
    return () => {
      window.removeEventListener("mousedown", onDown, true);
      window.removeEventListener("auxclick", block, true);
      window.removeEventListener("contextmenu", block, true);
      window.removeEventListener("keydown", onKey, true);
    };
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [capturing, settings, mode]);

  return (
    <div className="panel" data-row data-testid="mouse-ptt">
      <h3>Mouse button</h3>
      <div style={{ display: "flex", gap: 8, alignItems: "center", flexWrap: "wrap" }}>
        <button
          type="button"
          data-mouse-capture
          className={capturing ? "primary" : "ghost"}
          aria-pressed={capturing}
          onClick={() => {
            setHint(capturing ? null : MOUSE_CAPTURE_PROMPT);
            setCapturing((c) => !c);
          }}
        >
          {capturing ? "Listening… press a mouse button" : bound ? "Change button" : "Set mouse button"}
        </button>
        {bound && !capturing && (
          <button type="button" className="ghost" onClick={() => void saveMouse("")}>
            Remove
          </button>
        )}
        <span className="muted" data-testid="mouse-ptt-line">
          {mouseBindingLine(settings.pttMouseBinding)}
        </span>
      </div>
      {bound && (
        <div className="profile-row" style={{ marginTop: 12 }}>
          {(
            [
              ["hold", "Hold to talk", "Double-click for hands-free"],
              ["hands_free", "Click to toggle", "Click to start, click to stop"],
            ] as const
          ).map(([id, label, blurb]) => (
            <button
              key={id}
              type="button"
              className={mode === id ? "profile active" : "profile"}
              aria-pressed={mode === id}
              onClick={() => void saveMouse(withMode(settings.pttMouseBinding, id))}
            >
              <strong>{label}</strong>
              <span>{blurb}</span>
            </button>
          ))}
        </div>
      )}
      {hint && (
        <p className="muted" style={{ marginTop: 8 }} role="status">
          {hint}
        </p>
      )}
      {error && <p className="error">{error}</p>}
      <p className="settings-hint">
        Left and right click stay yours. While a button is set, Yap keeps it from other apps,
        using the Accessibility access paste already has.
      </p>
    </div>
  );
}

import { useEffect, useState } from "react";
import type { AppSettings } from "./appTypes";
import { formatChord, recorderStep } from "./hotkeyRecorder";
import {
  addBinding, bindingsOf, MAX_BINDINGS_PER_ACTION, removeAt, type EditableAction,
} from "./shortcutEditor";

/**
 * WF-18 — the 2nd … 4th shortcuts of one always-global action, under its main
 * row: each extra is a key cap with a remove button, "Add shortcut" records the
 * next chord, and an overlap with another action shows a warning instead of
 * saving. The main chord is never removable here (the row above changes or
 * resets it), so an action can never end up with no shortcut.
 */
export default function ExtraBindings(props: {
  action: EditableAction;
  title: string;
  settings: AppSettings;
  save: (next: AppSettings) => Promise<void> | void;
}) {
  const { action, title, settings, save } = props;
  const [listening, setListening] = useState(false);
  const [hint, setHint] = useState<string | null>(null);
  const [warning, setWarning] = useState<string | null>(null);
  const list = bindingsOf(settings, action);
  const extras = list.slice(1);
  const full = list.length >= MAX_BINDINGS_PER_ACTION;
  useEffect(() => {
    if (!listening) return;
    const onKey = (e: KeyboardEvent) => {
      e.preventDefault();
      e.stopPropagation();
      const step = recorderStep(e, "chord", false);
      if (step.kind === "cancel") {
        setListening(false);
        setHint(null);
      } else if (step.kind === "hint") {
        setHint(step.text);
      } else if (step.kind === "chord") {
        setListening(false);
        setHint(null);
        const { next, verdict } = addBinding(settings, action, step.chord);
        if (verdict.kind === "ok" && next) {
          setWarning(null);
          void save(next);
        } else if (verdict.kind !== "ok") {
          setWarning(verdict.text);
        }
      }
    };
    window.addEventListener("keydown", onKey, true);
    return () => window.removeEventListener("keydown", onKey, true);
  }, [listening, settings, action, save]);
  return (
    <div className="hotkey-extras" data-testid={`extras-${action}`} data-action={action}>
      {extras.length > 0 && (
        <ul className="hotkey-extras-list" aria-label={`More shortcuts for ${title}`}>
          {extras.map((c, i) => (
            <li key={`${formatChord(c)}-${i}`}>
              <kbd>{formatChord(c)}</kbd>
              <button
                type="button"
                className="ghost"
                aria-label={`Remove ${formatChord(c)} from ${title}`}
                onClick={() => {
                  const r = removeAt(settings, action, i + 1);
                  if (r.next) {
                    setWarning(null);
                    void save(r.next);
                  } else {
                    setWarning(r.error);
                  }
                }}
              >
                Remove
              </button>
            </li>
          ))}
        </ul>
      )}
      <button
        type="button"
        className={listening ? "primary" : "ghost"}
        aria-pressed={listening}
        disabled={full && !listening}
        title={full ? `${title} has ${MAX_BINDINGS_PER_ACTION} shortcuts` : undefined}
        onClick={() => {
          setWarning(null);
          setHint(listening ? null : "Press the new shortcut (Esc to cancel).");
          setListening((l) => !l);
        }}
      >
        {listening ? "Listening… press your keys" : full ? `${MAX_BINDINGS_PER_ACTION} of ${MAX_BINDINGS_PER_ACTION} shortcuts` : "Add shortcut"}
      </button>
      {hint && <p className="muted hotkey-extras-hint">{hint}</p>}
      {warning && (
        <p className="warning hotkey-extras-warning" role="status" aria-live="polite">
          {warning}
        </p>
      )}
    </div>
  );
}

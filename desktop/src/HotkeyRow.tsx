import { useEffect, useState } from "react";
import { chipTone, recorderStep, type BindingRow, type RecorderStep } from "./hotkeyRecorder";

/**
 * focus-2026-10-19 — one hotkey action: its key, a live status chip, a
 * recorder (press the keys, it captures the chord), and Reset.
 */
export default function HotkeyRecorder(props: {
  title: string;
  keyLabel: string;
  defaultLabel: string;
  isDefault: boolean;
  mode: "chord" | "hold";
  allowBare?: boolean;
  row?: BindingRow;
  onCapture: (step: Extract<RecorderStep, { kind: "chord" } | { kind: "hold" }>) => void;
  onReset: () => void;
  initiallyListening?: boolean;
}) {
  const [listening, setListening] = useState(Boolean(props.initiallyListening));
  const [hint, setHint] = useState<string | null>(null);
  useEffect(() => {
    if (!listening) return;
    const onKey = (e: KeyboardEvent) => {
      e.preventDefault();
      e.stopPropagation();
      const step = recorderStep(e, props.mode, props.allowBare);
      if (step.kind === "cancel") {
        setListening(false);
        setHint(null);
      } else if (step.kind === "hint") {
        setHint(step.text);
      } else {
        setListening(false);
        setHint(null);
        props.onCapture(step);
      }
    };
    window.addEventListener("keydown", onKey, true);
    return () => window.removeEventListener("keydown", onKey, true);
  }, [listening, props]);
  const row = props.row;
  return (
    <div className="hotkey-row" data-action={row?.action} style={{ display: "flex", gap: 8, alignItems: "center", flexWrap: "wrap", padding: "6px 0" }}>
      <span style={{ minWidth: 130 }}>{props.title}</span>
      <kbd>{props.keyLabel}</kbd>
      {row && (
        <span className={`chip chip-${chipTone(row.status.state)}`} title={row.status.detail ?? row.chip}>
          {row.chip}
          {row.fallback && row.liveKey ? ` · use ${row.liveKey} now` : ""}
        </span>
      )}
      <button
        type="button"
        className={listening ? "primary" : "ghost"}
        aria-pressed={listening}
        onClick={() => {
          setHint(listening ? null : props.mode === "hold" ? "Press fn or fn + Control (Esc to cancel)." : "Press the new chord (Esc to cancel).");
          setListening((l) => !l);
        }}
      >
        {listening ? "Listening… press your keys" : "Record"}
      </button>
      {!props.isDefault && (
        <button type="button" className="ghost" onClick={props.onReset}>
          Reset to {props.defaultLabel}
        </button>
      )}
      {hint && <span className="muted">{hint}</span>}
    </div>
  );
}

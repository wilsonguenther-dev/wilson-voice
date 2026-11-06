// yap-1106-meeting-export-and-leftovers — the export menu a meeting carries in
// BOTH places it is shown (the list row and the detail header): one trigger,
// three actions, each a Rust command. The action list is data so the vitest
// can pin that both surfaces offer the same three, and that every command
// named here exists in lib.rs.
import { useEffect, useRef, useState } from "react";

export type ExportAction = "export" | "copy" | "reveal";

/** Label + the Tauri command each action invokes. Order = menu order. */
export const EXPORT_ACTIONS: ReadonlyArray<{ action: ExportAction; label: string; command: string }> = [
  { action: "export", label: "Export Markdown…", command: "export_meeting_markdown" },
  { action: "copy", label: "Copy as Markdown", command: "copy_meeting_markdown" },
  { action: "reveal", label: "Show in Finder", command: "reveal_meeting_export" },
];

export const EXPORT_TRIGGER_LABEL = "Export";

interface Props {
  /** Which meeting the actions act on — forwarded to `onAction`. */
  meetingId: string;
  onAction: (action: ExportAction, meetingId: string) => void;
  disabled?: boolean;
  /** The trigger's look: the detail header uses the primary style. */
  primary?: boolean;
  /** Tests only: render the menu already open (no DOM to click in). */
  defaultOpen?: boolean;
}

export default function ExportMenu({ meetingId, onAction, disabled, primary, defaultOpen }: Props) {
  const [open, setOpen] = useState(Boolean(defaultOpen));
  const root = useRef<HTMLDivElement | null>(null);
  // Outside click / Escape closes it. Listeners exist only while open.
  useEffect(() => {
    if (!open) return;
    const onDown = (e: MouseEvent) => {
      if (root.current && !root.current.contains(e.target as Node)) setOpen(false);
    };
    const onKey = (e: KeyboardEvent) => {
      if (e.key === "Escape") setOpen(false);
    };
    document.addEventListener("mousedown", onDown);
    document.addEventListener("keydown", onKey);
    return () => {
      document.removeEventListener("mousedown", onDown);
      document.removeEventListener("keydown", onKey);
    };
  }, [open]);
  return (
    <div className="export-menu" ref={root} data-export-menu={meetingId}>
      <button
        type="button"
        className={primary ? "primary" : "ghost"}
        disabled={disabled}
        aria-haspopup="menu"
        aria-expanded={open}
        onClick={() => setOpen((o) => !o)}
      >
        {EXPORT_TRIGGER_LABEL} ▾
      </button>
      {open && (
        <div className="export-menu-list" role="menu" aria-label="Export this meeting">
          {EXPORT_ACTIONS.map((a) => (
            <button
              key={a.action}
              type="button"
              role="menuitem"
              data-export-action={a.action}
              onClick={() => {
                setOpen(false);
                onAction(a.action, meetingId);
              }}
            >
              {a.label}
            </button>
          ))}
        </div>
      )}
    </div>
  );
}

import { useEffect, useRef, useState } from "react";
import {
  actionExplainer,
  actionLabel,
  describeEntry,
  formatBytes,
  isRedacted,
  type SupportBundlePreview,
} from "./bundle";

/**
 * YV98 — "here is exactly what you are about to send."
 *
 * Overlay grammar follows `MeetingConsentNotice` (same backdrop, same
 * Esc-closes, same focused primary action), because a user who has seen one of
 * Yap's sheets should recognise the next one. What is different is that this
 * sheet has a real Cancel: the consent notice rides alongside a recording that
 * is already running, while nothing here has happened yet — the zip does not
 * exist on disk until the action button is pressed.
 *
 * The list is not a summary. Every row expands to the ACTUAL bytes that were
 * packed, redaction markers and all, which is the point finding #36 was making:
 * a privacy claim you can read is worth more than one you are asked to trust.
 */
export default function SupportBundleSheet({
  preview,
  busy,
  onSend,
  onClose,
  initialOpen = null,
}: {
  /** `null` while the backend is still building it. */
  preview: SupportBundlePreview | null;
  busy: boolean;
  onSend: () => void;
  onClose: () => void;
  /** Entry to start expanded. Only the dev preview passes this — the app opens
   *  the sheet collapsed, because seven open files is not a thing you read. */
  initialOpen?: string | null;
}) {
  const actionRef = useRef<HTMLButtonElement | null>(null);
  const openRef = useRef<HTMLLIElement | null>(null);
  const [open, setOpen] = useState<string | null>(initialOpen);

  useEffect(() => {
    actionRef.current?.focus();
  }, [preview]);

  // Open a row near the bottom of a scrolled list and the contents you asked
  // for land off-screen — the row toggles and, as far as the user can tell,
  // nothing happened.
  useEffect(() => {
    if (open) openRef.current?.scrollIntoView({ block: "nearest" });
  }, [open]);

  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      if (e.key === "Escape" && !busy) onClose();
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, [busy, onClose]);

  return (
    <div
      className="consent-overlay"
      role="dialog"
      aria-modal="true"
      aria-labelledby="support-title"
      onClick={(e) => {
        if (e.target === e.currentTarget && !busy) onClose();
      }}
    >
      <div className="consent-card support-card">
        <p className="consent-eyebrow">Crash report</p>
        <h1 id="support-title">Everything that would be sent</h1>

        {!preview ? (
          <p className="consent-body">Building the report…</p>
        ) : (
          <>
            <p className="consent-body">
              Yap packed {preview.entries.length} text files —{" "}
              {formatBytes(preview.totalBytes)} in total. No recordings, no
              database, no transcripts. Every log line went through a redaction
              pass first: paths, your account name, email addresses, tokens and
              long numbers were replaced, and every line left is one Yap ships
              inside itself, with only values in the gaps — anything else is a
              marker and a count. Open any row to read it.
            </p>

            <ul className="support-list">
              {preview.entries.map((entry) => {
                const isOpen = open === entry.name;
                return (
                  <li
                    key={entry.name}
                    className="support-entry"
                    ref={isOpen ? openRef : undefined}
                  >
                    <button
                      type="button"
                      className="support-row"
                      aria-expanded={isOpen}
                      onClick={() => setOpen(isOpen ? null : entry.name)}
                    >
                      <span className="support-row-main">
                        <code className="support-name">{entry.name}</code>
                        <span className="tiny">{describeEntry(entry.name)}</span>
                      </span>
                      <span className="support-row-meta">
                        {isRedacted(entry.name) && (
                          <span className="support-badge">redacted</span>
                        )}
                        <span className="tiny">{formatBytes(entry.bytes)}</span>
                        <span aria-hidden>{isOpen ? "−" : "+"}</span>
                      </span>
                    </button>
                    {isOpen && (
                      <pre className="support-excerpt">
                        {entry.excerpt}
                        {entry.truncated && (
                          <span className="tiny">
                            {"\n"}… {entry.lines} lines in total; the rest is
                            more of the same.
                          </span>
                        )}
                      </pre>
                    )}
                  </li>
                );
              })}
            </ul>

            <p className="consent-body">{actionExplainer(preview)}</p>
          </>
        )}

        <div className="consent-actions">
          <button type="button" className="ghost" disabled={busy} onClick={onClose}>
            Cancel
          </button>
          <button
            ref={actionRef}
            type="button"
            className="primary"
            disabled={!preview || busy}
            onClick={onSend}
          >
            {busy ? "Working…" : actionLabel(preview)}
          </button>
        </div>

        {/* The fine print has to match the path this Mac is actually on. On a
            machine whose mail client AppKit cannot drive, "handed to your mail
            app" would be the one false sentence in a sheet whose entire job is
            being believable. */}
        <p className="consent-fine">
          {preview && !preview.mailAvailable
            ? "Yap does not upload this, or anything else. The file is written to your Desktop and stays there until you attach it to an email yourself."
            : "Yap does not upload this, or anything else. The file is written to your Desktop and handed to your own mail app — you are the one who presses send."}
        </p>
      </div>
    </div>
  );
}

/**
 * Y10-A — the bar's language picker. Rendered inside the capsule (it shares
 * the pill's one published hitbox, like the Y2-D upgrade mark), and only when
 * the user's language set has a choice in it, so a default English install's
 * bar is unchanged. A click cycles Auto → each language; the tooltip carries
 * the backend's notice when the model cannot do what was picked, so the chip
 * never silently does nothing. A failed write rolls the label back.
 */
import { chipLabel, chipTitle, nextPick, showChip } from "../languagePicker";
import { languageStore, useLanguageStore } from "../languageStore";

export default function LanguageChip() {
  // WF-11 — the window's one language store: a pick from the tray's
  // Languages submenu (or Settings) changes this label at once, and a click
  // here redraws the tray's check through the same backend writer.
  const { opts } = useLanguageStore();
  if (!showChip(opts)) return null;
  const title = chipTitle(opts);
  return (
    <button
      type="button"
      className={`pill-lang${opts.notice ? " has-notice" : ""}`}
      data-mode={opts.mode}
      title={title}
      aria-label={title}
      tabIndex={-1}
      onPointerDown={(e) => e.stopPropagation()}
      onClick={(e) => {
        e.stopPropagation();
        void languageStore.write(nextPick(opts));
      }}
    >
      {chipLabel(opts)}
    </button>
  );
}

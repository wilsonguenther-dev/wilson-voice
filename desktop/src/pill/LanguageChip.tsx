/**
 * Y10-A — the bar's language picker. Rendered inside the capsule (it shares
 * the pill's one published hitbox, like the Y2-D upgrade mark), and only when
 * the user's language set has a choice in it, so a default English install's
 * bar is unchanged. A click cycles Auto → each language; the tooltip carries
 * the backend's notice when the model cannot do what was picked, so the chip
 * never silently does nothing.
 */
import { useCallback, useEffect, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import {
  chipLabel, chipTitle, nextPick, showChip, type LanguageOptions,
} from "../languagePicker";

export default function LanguageChip() {
  const [opts, setOpts] = useState<LanguageOptions | null>(null);
  const refresh = useCallback(() => {
    invoke<LanguageOptions>("language_options").then(setOpts).catch(() => {});
  }, []);
  useEffect(() => {
    refresh();
    let dead = false;
    let unlisten: (() => void) | null = null;
    listen("settings", () => refresh()).then((u) => {
      if (dead) u();
      else unlisten = u;
    });
    return () => {
      dead = true;
      unlisten?.();
    };
  }, [refresh]);
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
        const next = nextPick(opts);
        invoke<unknown>("set_dictation_languages", { ...next })
          .then(refresh)
          .catch(() => {});
      }}
    >
      {chipLabel(opts)}
    </button>
  );
}

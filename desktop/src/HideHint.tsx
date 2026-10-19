// yap-lifecycle DO 2 — the red X hides Yap (it is a menu-bar utility), and the
// FIRST time it ever does, the window's own surface says where Yap went and how
// to quit. No modal: a dismissible note at the top of the main view.
//
// The hint is CLAIMED, never pushed: the backend's red X only makes it pending
// (lifecycle::HideHintGate::note_hide) and nudges with HIDE_HINT_EVENT; the
// once-per-install marker is written by `claim_hide_hint`, which only this
// mounted component calls — on mount and on each nudge. A hide during first-run
// onboarding (when this component is not mounted) therefore does not use the
// hint up: the main view claims it the moment it mounts.
import { useEffect, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";

/** Must equal `lifecycle::HIDE_HINT_EVENT` in src-tauri/src/lifecycle.rs. */
export const HIDE_HINT_EVENT = "window_hidden_hint";
/** The Tauri command that consumes the pending hint (lib.rs `claim_hide_hint`). */
export const CLAIM_HIDE_HINT = "claim_hide_hint";

export interface HideHintDeps {
  claim: () => Promise<boolean>;
  listen: (event: string, cb: () => void) => Promise<() => void>;
}

/**
 * Wire one mounted HideHint: claim once on mount (a hide that happened while
 * nothing was mounted is waiting), and claim again on every nudge. `show` is
 * called only when the backend hands over the hint. Returns the teardown.
 */
export function wireHideHint(deps: HideHintDeps, show: () => void): () => void {
  let dead = false;
  let unlisten: (() => void) | undefined;
  const claim = () => {
    deps
      .claim()
      .then((owed) => {
        if (owed && !dead) show();
      })
      .catch(() => {});
  };
  deps
    .listen(HIDE_HINT_EVENT, claim)
    .then((u) => {
      if (dead) u();
      else unlisten = u;
    })
    .catch(() => {});
  claim();
  return () => {
    dead = true;
    unlisten?.();
  };
}

export default function HideHint() {
  const [shown, setShown] = useState(false);
  useEffect(
    () =>
      wireHideHint(
        {
          claim: () => invoke<boolean>(CLAIM_HIDE_HINT),
          listen: (event, cb) => listen(event, () => cb()),
        },
        () => setShown(true),
      ),
    [],
  );
  if (!shown) return null;
  return (
    <p className="hide-hint" role="status" data-hide-hint>
      Closing the window keeps Yap running in the menu bar, so the hotkey still
      works. Click the Yap icon in the menu bar or the Dock to come back; quit
      with &#8984;Q or Quit Yap in the menu bar.{" "}
      <button type="button" onClick={() => setShown(false)}>
        Got it
      </button>
    </p>
  );
}

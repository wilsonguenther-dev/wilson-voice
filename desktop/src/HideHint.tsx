// yap-lifecycle DO 2 — the red X hides Yap (it is a menu-bar utility), and the
// FIRST time it ever does, the window's own surface says where Yap went and how
// to quit. No modal: a dismissible note at the top of the main view. The
// backend emits HIDE_HINT_EVENT at that first hide (once per install — see
// lifecycle::take_hide_hint); the webview keeps running while the window is
// hidden, so the note is already showing when the window comes back.
import { useEffect, useState } from "react";
import { listen } from "@tauri-apps/api/event";

/** Must equal `lifecycle::HIDE_HINT_EVENT` in src-tauri/src/lifecycle.rs. */
export const HIDE_HINT_EVENT = "window_hidden_hint";

export default function HideHint() {
  const [shown, setShown] = useState(false);
  useEffect(() => {
    let dead = false;
    let unlisten: (() => void) | undefined;
    listen(HIDE_HINT_EVENT, () => setShown(true)).then((u) => {
      if (dead) u();
      else unlisten = u;
    });
    return () => {
      dead = true;
      unlisten?.();
    };
  }, []);
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

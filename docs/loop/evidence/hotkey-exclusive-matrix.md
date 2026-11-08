# yap-1107-hotkey-exclusive-to-yap — evidence

Coder seat (no compile). Written 2026-11-08 on base e966b985 (origin/main). Lines below are
against branch loop3/yap-1107-hotkey-exclusive-to-yap.

## What was built, and which acceptance line each piece proves

- **Active swallow tap** (acceptance 1, 5, 6): `ptt_macos.rs` `ensure_key_swallow_tap` (≈1855) creates
  a SECOND tap — session location, `kCGEventTapOptionDefault`, mask key-down | key-up only
  (`KEY_SWALLOW_MASK` ≈1735). The main fn tap is unchanged and still LISTEN-ONLY; it still feeds
  the machine. This is the WF-19 mouse-swallow pattern (`ensure_mouse_swallow_tap`).
- **Why the mask has no flags-changed:** fn / ⌃ / ⌘ / ⌥ press and release travel as
  flags-changed events. The swallow tap is never even handed one, so it cannot leave Control or
  Fn stuck whatever its decision says. The Fn release is NOT swallowed: the globe-key item's
  evidence says "Not proven, NOT shipped", so it was not treated as proven safe.
- **`swallow_decision(binding, flags, keycode, event_type, take_state)`** (≈1806), pure. The
  only non-modifier key inside a chord the tap acts on is the Globe key's own key event
  (keycode 179, SP-02), posted with the fn edge of Yap's press. Its key-down is swallowed when
  the binding's combo is held (event flags or the machine's `combo_down`). Its key-up is
  swallowed exactly when its key-down was, so no app ever gets half a key. Every other key
  passes: typing during a hold is the user's (it interrupts the take), and the Carbon chords are
  consumed by `RegisterEventHotKey` itself. If the tap swallowed a Carbon key-down, Carbon would
  stop firing.
- **Accessibility refused** (acceptance 5): checked first with `AXIsProcessTrusted` (no prompt).
  A refusal or a failed create logs `ptt key_swallow_unavailable reason=accessibility_off|tap_refused`,
  sets `hotkeys::ExclusiveStatus::ListenOnly`, and the header status chip shows
  `hotkeys::LISTEN_ONLY_LINE` through `degraded_line(..).or_else(exclusive_degraded_line)`
  (lib.rs ≈1734). The hotkeys keep working through the listen-only tap. Every permission edge
  calls `start` again, which re-tries the swallow tap (ptt_macos.rs ≈1212).
- **Tap disabled by macOS** (acceptance 6 / BUILD 4): `on_key_swallow_disabled` (≈1928) drops
  the pairing flag, re-enables the port and logs
  `ptt key_swallow_tap disabled kind=… re_armed=… disables=… swallowed_total=…`. While a tap is
  disabled, macOS passes every event, so a timed-out tap can eat nothing. The main tap's Y1-A
  re-arm path (`on_tap_disabled`, `keep_re_arming`, `recreate_tap`) is untouched, so its existing
  tests (`re_arm_retry_is_single_flight_per_tap_and_bounded` …) are unchanged.
- **Diagnostics line** (acceptance 4): `diagnostics::record_hotkey_swallowed` writes
  `hotkey swallowed chord=<label> phase=down|up front=<bundle id> by=tap|carbon` and bumps
  `hotkey.swallowed`. The tap side logs from a short-lived thread (`spawn_swallow_log`), so
  looking up the front app never slows a callback macOS can time out. The Carbon side
  (`hotkeys::note_carbon_swallowed`, called from the plugin handler at lib.rs ≈15210) also
  logs off-thread, because that handler runs with the shortcut table locked and must only
  route. The line carries no key text beyond the chord label.

## Carbon chords: the exclusive probe (BUILD 2)

The dictate chord's registrar (`TauriCarbonRegistrar::register`, lib.rs ≈13388) runs
`carbon_probe::probe_chord` with `kEventHotKeyExclusive` BEFORE the plugin's non-exclusive
register. A chord another app or macOS holds comes back as `eventHotKeyExistsErr` and is refused,
so a `dictate-toggle registered (carbon)` line means no other process held the chord at
registration. Not recorded live from this seat (no app may run here). INTEGRATOR: on the test
build, `grep -E "dictate-toggle (registered \(carbon\)|exclusive probe refused)" <data>/logs/yap.log`
must show the registered line and no refusal; paste it under this heading.
Limit, stated honestly: the plugin's own register stays non-exclusive (global-hotkey 0.8.0), so
an app that registers the same chord LATER is not blocked by Yap.

## Acceptance 2, 3, 4: matrix run (INTEGRATOR)

Not run from this seat: it needs a compiled test build, Accessibility for that build, and Input
Monitoring + Accessibility for the shell that posts the events. Command (appends its table,
stuck-modifier line and diagnostics excerpt below):

    swift desktop/scripts/hotkey-exclusive-matrix.swift --confirm-test-build \
      --log "<Yap Test data dir>/logs/yap.log" --out docs/loop/evidence/hotkey-exclusive-matrix.md --takes 50

Run the test build with insertion set to copy-only, so a transcript paste does not count as a
leak in TextEdit. The script refuses to post unless the frontmost app is TextEdit (a document it
created through the AppleScript object model and closes without saving at the end) or Yap Test.
It never targets Wilson's Yap or his Terminal. Pass condition: exit 0, meaning every fn⌃ leak
count is 0, the TextEdit text is unchanged, and CGEventSourceFlagsState (HID and combined
session) shows no Control, Fn, Command or Option bit after 50 takes.

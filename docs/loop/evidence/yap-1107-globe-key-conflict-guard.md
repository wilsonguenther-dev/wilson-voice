# yap-1107-globe-key-conflict-guard — evidence

Written 2026-11-08T04:12Z by Coder B2 (loop three). Code only; nothing here was compiled by the coder.

## Acceptance line 2 — dry path against a test domain (real `defaults`, scratch domain)

The real button runs exactly these command lines against `com.apple.HIToolbox`
(globe_key.rs `read_args` / `write_off_args` / `restore_args`):

    defaults read  com.apple.HIToolbox AppleFnUsageType              # status / before / re-read
    defaults write com.apple.HIToolbox AppleFnUsageType -int 0       # Turn it off
    defaults write com.apple.HIToolbox AppleFnUsageType -int <prior> # Put it back, prior was a value
    defaults delete com.apple.HIToolbox AppleFnUsageType             # Put it back, prior was unset

Below, the same verbs run against the scratch domain `com.wilsonguenther.yap.globe-test`
(Wilson's real preference was only READ; it already equals the target 0). The scratch
domain and its plist were deleted afterwards. Note the unset read exits 1, which is what
`read_pref` maps to "unset".

```
## Real preference (READ ONLY, never written)
$ defaults read com.apple.HIToolbox AppleFnUsageType
0
(exit 0)

## Scenario A: test domain unset
### before
$ defaults read com.wilsonguenther.yap.globe-test AppleFnUsageType
2026-11-07 22:12:09.501 defaults[78224:17623400] 
The domain/default pair of (com.wilsonguenther.yap.globe-test, AppleFnUsageType) does not exist
(exit 1)
### Turn it off
$ defaults write com.wilsonguenther.yap.globe-test AppleFnUsageType -int 0
(exit 0)
$ defaults read com.wilsonguenther.yap.globe-test AppleFnUsageType
0
(exit 0)
### Put it back (prior = unset -> delete)
$ defaults delete com.wilsonguenther.yap.globe-test AppleFnUsageType
(exit 0)
$ defaults read com.wilsonguenther.yap.globe-test AppleFnUsageType
2026-11-07 22:12:09.526 defaults[78228:17623414] 
The domain/default pair of (com.wilsonguenther.yap.globe-test, AppleFnUsageType) does not exist
(exit 1)

## Scenario B: test domain = 2 (emoji picker)
$ defaults write com.wilsonguenther.yap.globe-test AppleFnUsageType -int 2
(exit 0)
### before
$ defaults read com.wilsonguenther.yap.globe-test AppleFnUsageType
2
(exit 0)
### Turn it off
$ defaults write com.wilsonguenther.yap.globe-test AppleFnUsageType -int 0
(exit 0)
$ defaults read com.wilsonguenther.yap.globe-test AppleFnUsageType
0
(exit 0)
### Put it back (prior = 2 -> write 2)
$ defaults write com.wilsonguenther.yap.globe-test AppleFnUsageType -int 2
(exit 0)
$ defaults read com.wilsonguenther.yap.globe-test AppleFnUsageType
2
(exit 0)

## Fixture cleanup
$ defaults delete com.wilsonguenther.yap.globe-test
(exit 0)
$ defaults read com.wilsonguenther.yap.globe-test
2026-11-07 22:12:09.575 defaults[78236:17623438] 
Domain com.wilsonguenther.yap.globe-test does not exist
(exit 1)
```

The same sequence is asserted with an injected runner (exact command lines, in order) by
the Rust test `globe_key::tests::globe_turn_off_then_put_back_rechecks_the_line_from_unset`
and, for a recorded value, `globe_put_back_restores_a_recorded_value_and_keeps_the_original`.

## Acceptance line 4 — research results

### Does the change need a logout?
nix-darwin's option page for this exact key, fetched 2026-11-08
(https://mynixos.com/nix-darwin/option/system.defaults.hitoolbox.AppleFnUsageType), says:
"Chooses what happens when you press the Fn key on the keyboard. A restart is required for
this setting to take effect." No fetched source says a logout alone is enough, and none says
it applies live. So the line after "Turn it off" says plainly: "A lone Fn tap now does
nothing. macOS applies this fully after you restart your Mac." (globe_key.rs TURNED_OFF_LINE).
Unverified either way on this Mac: Wilson's value was set to 0 by hand at 17:26Z on
2026-11-07 and nobody recorded whether it applied before a restart.

### Can an ACTIVE tap swallow the lone-Fn release that ends a take?
Not proven, NOT shipped. Yap's fn tap stays listen-only (ptt_macos.rs MAIN_TAP_OPTIONS =
KCG_EVENT_TAP_OPTION_LISTEN_ONLY); no `ptt globe_release swallowed` path exists.
- The keyboardio/Chrysalis maintainer (https://github.com/keyboardio/Chrysalis/issues/1310,
  fetched 2026-11-08) reports that Apple keyboards send Globe through "a custom USB HID device
  descriptor that maps 'globe' to HID page 0xFF key 0x03" and that "Apple isn't exposing
  globe-as-modifier through standard USB peripherals". That points at the Globe action being
  decided from a vendor HID usage, below or beside the CGEvent stream a tap sees. It is his
  inference, not Apple documentation.
- No fetched source shows a CGEventTap (HID or session level, kCGEventTapOptionDefault)
  returning NULL for the fn flags-changed event or keycode 179 and thereby stopping the
  emoji picker. A web search for exactly that found nothing.
- It cannot be proven headless: the experiment needs a physical Globe key press and a
  visible emoji picker on a logged-in session, and the no-compile rule forbids a scratch
  binary in this seat. An active keyboard tap would also need Accessibility, and a wrong
  NULL would eat a user's real modifier release (stuck keys).
- Result: the consented preference fix is the shipped remedy. A follow-up could run the
  experiment by hand on Wilson's Mac (set the value to 2, active HID tap that drops the
  fn-up flags-changed + keycode 179, tap Globe, watch for the picker).

The Wispr docs claim is not re-opened here (per the item).

## Acceptance line 5 — capture
Not producible in a coder seat (needs the built frontend). INTEGRATOR: render
`GlobeKeyRowView` in both states (conflict with "Turn it off"; turned off with "Put it back")
at 2x via scripts/headless-chrome.mjs and save
docs/loop/evidence/yap-1107-globe-key-conflict-guard-{conflict,off}@2x.png.

## Design note
No approved design-2026-11-06 page covers the Permissions list; the row reuses the
existing `perm-list` row markup (StatusDot + text + one button) with no new styles, so
nothing approved changes. Approved sentence kept verbatim: "Your Mac opens the emoji picker
on a lone Fn tap; Yap can turn that off", button "Turn it off", control "Put it back".

## Acceptance line 5 — 2x captures (integrator, 2026-11-08T06:56:32Z, integration-2 at dc9cc169)

`node scripts/globe-key-shots.mjs --out docs/loop/evidence` exit 0 (vite build to a scratch dir, real Chrome --headless=new, Permissions page at 1190x773 scale 2, globe_key_status fixture per state; both lines and their one button checked):

- `yap-1107-globe-key-conflict-guard-conflict@2x.png` and `-conflict-page@2x.png`: "Your Mac opens the emoji picker on a lone Fn tap; Yap can turn that off" with "Turn it off".
- `yap-1107-globe-key-conflict-guard-off@2x.png` and `-off-page@2x.png`: "A lone Fn tap now does nothing. macOS applies this fully after you restart your Mac." with "Put it back".

Integration note: GlobeKeyRow.tsx moved to desktop/src/views/settings/parts/ (views.test.tsx allows one module per Nav value in views/), and the injected-runner test now expects the status read that precedes turn_off's own prior read.

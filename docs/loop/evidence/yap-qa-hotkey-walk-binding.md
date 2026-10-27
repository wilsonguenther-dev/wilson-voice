ROOT-CAUSE-PROBE yap-qa-hotkey-walk-binding

# yap-qa-hotkey-walk-binding — the walk pressed fn; the app listens for fn⌃

Walk finding D2 (P1): `scripts/qa/hotkey-walk.sh` FAILED on a working app.

## Root cause, probed on origin/main (821910c), read-only

The app's default binding, and Wilson's own, is `fn_control`:

```
$ git show origin/main:desktop/src-tauri/src/lib.rs | grep -n -A2 '^fn default_ptt_binding'
473:fn default_ptt_binding() -> String {
474-    "fn_control".into()
475-}
$ plutil -extract pttBinding raw -o - ~/Library/Application\ Support/WilsonVoice/settings.json
fn_control
```

Under `fn_control` a press is only a press when fn AND ⌃ are down:

```
$ git show origin/main:desktop/src-tauri/src/ptt_macos.rs | grep -n -A4 '^fn combo_wanted'
944:fn combo_wanted(binding: PttBinding, fn_down: bool, control: bool) -> bool {
945-    match binding {
946-        PttBinding::Fn => fn_down,
947-        PttBinding::FnControl => fn_down && control,
948-        PttBinding::FnOrFnControl => fn_down,
```

…but the walk only ever posted bare fn, with fn as the ONLY flag:

```
$ git show origin/main:scripts/qa/fn-post.swift | grep -n 'maskSecondaryFn :'
56:    ev.flags = down ? .maskSecondaryFn : []
$ git show origin/main:scripts/qa/hotkey-walk.sh | grep -nF -e '"$fnpost" down' -e '"$fnpost" up'
244:  "$fnpost" down || { rm -f "$seg"; die "fn-post down failed"; }
246:  "$fnpost" up || { rm -f "$seg"; die "fn-post up failed"; }
258:  "$fnpost" down sleep:70 up sleep:150 down sleep:70 up || { rm -f "$seg"; die "fn-post double-tap failed"; }
260:  "$fnpost" down sleep:70 up || { rm -f "$seg"; die "fn-post tap failed"; }
```

So `combo_wanted(FnControl, fn_down=true, control=false)` is false, the machine
never sees a press, "PTT hold armed" never lands, and the walk reports a broken
hotkey on a working app. The same gap is the product bug: a user who holds fn
alone under the default binding gets nothing and no reason.

## Fix

1. `hotkey-walk.sh` reads `pttBinding` from the app's settings.json (the same
   data dir the app resolves: `$YAP_DATA_DIR`, else
   `~/Library/Application Support/WilsonVoice`; `--settings <path>` overrides).
   `fn`/`globe`/`both` → chord `fn`; `fn_control` → chord `ctrl+fn` (fn-post ops
   `ctrl-down down`, released `up ctrl-up`). No key / no file → the app default
   `fn_control`. An unknown value or unreadable JSON → named refusal, exit 2,
   nothing built or posted.
2. `fn-post.swift` gained `ctrl-down` / `ctrl-up`; modifier ops accumulate, so
   every flagsChanged event carries all held modifiers (fn⌃ = maskControl |
   maskSecondaryFn).
3. `--dev-build[=BIN]` walks an unbundled dev binary matched by its target path
   (`pgrep -f '^BIN( |$)'`). `YAP_QA_FAKE_PIDS` is now refused outside
   `--self-test` (exit 2, named).
4. `ptt_macos`: a bare fn press (down+up, or held > 400 ms) under `fn_control`
   emits one `PttEvent::FnWithoutModifierHint`, throttled to once per 30 s; the
   pill strip shows `FN_WITHOUT_MODIFIER_HINT` ("Hold fn⌃ — fn alone does
   nothing") for 3.5 s. fn with another key (fn+arrow, fn+F-key) or another
   modifier is a different shortcut and never hints. Pure decision:
   `fn_hint_due(binding, fn_down, ctrl_down, held_ms, last_hint_ms, now_ms)`.

## What this does NOT prove

No live walk was run against a running Yap in this pass (that posts keys into a
live session and needs YAP_QA_ALLOW_KEYS=1 consent). The hint on the pill was not
observed on screen; it is proven by unit tests of the decision + tracker and by
the vitest that pins the event name, the line and the CSS hook.

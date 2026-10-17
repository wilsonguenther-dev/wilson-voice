# yap-p0-hotkeys-no-permission — evidence

ROOT-CAUSE-PROBE yap-p0-hotkeys-no-permission

Stamped 2026-10-17T08:58:37Z from `date -u`. Branch loop/yap-p0-hotkeys-carbon-primary-zero-tcc, probe run on unmodified origin/main bdddf5201f88a653d71b18f6a57014e598b60b9b.

## Root cause, proven (tests/hotkey_no_tcc.rs, first version, on unmodified main)

The probe asserted the two properties this item needs. On main both FAIL:
no registered chord starts a take while idle (only the fn tap does), and
`hotkey_registered` is written true right after `ptt_macos::start(` returns,
before the tap thread has run `CGEventTapCreate`, with no registration result in between.

```
$ cargo test --features custom-protocol --test hotkey_no_tcc -- --nocapture   # exit 101
    Finished `test` profile [unoptimized + debuginfo] target(s) in 6.34s
     Running tests/hotkey_no_tcc.rs (/Users/wilsonguenther/code/wilson-voice-loop/target-a/debug/deps/hotkey_no_tcc-cc1d094c75832b1c)
running 2 tests
root-cause: chords registered while idle = ["paste_last", "undo_ai_edit", "dictation_toggle_legacy", "meeting_toggle"]
root-cause: unconditional dictation chords = [] (dictation_toggle_legacy is gated on keep_cmd_shift_v, default false)
thread 'zero_grants_dictate_toggle_starts_and_stops' (16715505) panicked at src-tauri/tests/hotkey_no_tcc.rs:17:5:
no registered chord starts a take — only the fn tap does
test zero_grants_dictate_toggle_starts_and_stops ... FAILED
root-cause: hotkey_registered=true is written 3899 bytes after ptt_macos::start( returns — the tap thread has not even run install_tap yet
thread 'failed_registration_never_reported_registered' (16715504) panicked at src-tauri/tests/hotkey_no_tcc.rs:26:5:
hotkey_registered reads true without any registration result
test failed_registration_never_reported_registered ... FAILED
failures:
failures:
    failed_registration_never_reported_registered
    zero_grants_dictate_toggle_starts_and_stops
test result: FAILED. 0 passed; 2 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s
```

## yap.log (read-only grep of ~/Library/Application Support/WilsonVoice/logs/yap.log; never edited)

The probe said "not trusted" while the tap delivered fn and started a take (07:40:34Z-07:40:39Z),
so arming must never be gated on the probe:

```
[2026-10-17T07:40:34Z INFO  wilson_voice_lib] Accessibility not trusted — fn⌃ needs it. Enable Wilson Voice in Privacy → Accessibility
[2026-10-17T07:40:34Z INFO  wilson_voice_lib] PTT hybrid started (fn⌃ · command mode +⌘)
[2026-10-17T07:40:34Z INFO  wilson_voice_lib] ⌃⌘V paste-last registered
[2026-10-17T07:40:34Z INFO  wilson_voice_lib] ⌃⌘M meeting-toggle registered
[2026-10-17T07:40:34Z INFO  wilson_voice_lib] ⌃⌘Z undo-ai-edit registered
[2026-10-17T07:40:34Z INFO  wilson_voice_lib::ptt_macos] FN PTT hybrid tap running (fn⌃ hold / double-tap hands-free)
[2026-10-17T07:40:37Z INFO  wilson_voice_lib::ptt_macos] PTT double-tap → hands-free ON
[2026-10-17T07:40:37Z INFO  wilson_voice_lib] hands-free ON
[2026-10-17T07:40:39Z INFO  wilson_voice_lib::record] capture armed: press→capture_start=1883ms (arm_wait=1883ms)
[2026-10-17T07:40:39Z INFO  wilson_voice_lib] recording started
```

The tap is repeatedly switched off by macOS (kCGEventTapDisabledByUserInput) — fn is a fragile path even with grants:

```
[2026-10-17T07:38:25Z WARN  wilson_voice_lib::ptt_macos] FN PTT: tap disabled by kCGEventTapDisabledByUserInput (a user-input flood took the tap out) — re-arm #1 issued
[2026-10-17T07:38:31Z WARN  wilson_voice_lib::ptt_macos] FN PTT: tap disabled by kCGEventTapDisabledByUserInput (a user-input flood took the tap out) — re-arm #2 issued
[2026-10-17T07:38:38Z WARN  wilson_voice_lib::ptt_macos] FN PTT: tap disabled by kCGEventTapDisabledByUserInput (a user-input flood took the tap out) — re-arm #3 issued
[2026-10-17T07:38:53Z WARN  wilson_voice_lib::ptt_macos] FN PTT: tap disabled by kCGEventTapDisabledByUserInput (a user-input flood took the tap out) — re-arm #4 issued
[2026-10-17T07:40:18Z WARN  wilson_voice_lib::ptt_macos] FN PTT: tap disabled by kCGEventTapDisabledByUserInput (a user-input flood took the tap out) — re-arm #5 issued
[2026-10-17T07:44:31Z WARN  wilson_voice_lib::ptt_macos] FN PTT: tap disabled by kCGEventTapDisabledByUserInput (a user-input flood took the tap out) — re-arm #1 issued
[2026-10-17T07:44:34Z WARN  wilson_voice_lib::ptt_macos] FN PTT: tap disabled by kCGEventTapDisabledByUserInput (a user-input flood took the tap out) — re-arm #2 issued
[2026-10-17T07:44:40Z WARN  wilson_voice_lib::ptt_macos] FN PTT: tap disabled by kCGEventTapDisabledByUserInput (a user-input flood took the tap out) — re-arm #3 issued
```

## The chord is not a macOS system hotkey on this Mac

`defaults read com.apple.symbolichotkeys AppleSymbolicHotKeys` (full output):

```
{
    15 =     {
        enabled = 0;
    };
    16 =     {
        enabled = 0;
    };
    164 =     {
        enabled = 0;
        value =         {
            parameters =             (
                65535,
                65535,
                0
            );
            type = standard;
        };
    };
    17 =     {
        enabled = 0;
    };
    18 =     {
        enabled = 0;
    };
    19 =     {
        enabled = 0;
    };
    20 =     {
        enabled = 0;
    };
    21 =     {
        enabled = 0;
    };
    22 =     {
        enabled = 0;
    };
    23 =     {
        enabled = 0;
    };
    24 =     {
        enabled = 0;
    };
    25 =     {
        enabled = 0;
    };
    26 =     {
        enabled = 0;
    };
    60 =     {
        enabled = 0;
        value =         {
            parameters =             (
                32,
                49,
                262144
            );
            type = standard;
        };
    };
    61 =     {
        enabled = 0;
        value =         {
            parameters =             (
                32,
                49,
                786432
            );
            type = standard;
        };
    };
    64 =     {
        enabled = 1;
        value =         {
            parameters =             (
                65535,
                49,
                1048576
            );
            type = standard;
        };
    };
    65 =     {
        enabled = 1;
        value =         {
            parameters =             (
                65535,
                49,
                1572864
            );
            type = standard;
        };
    };
    79 =     {
        enabled = 1;
    };
    80 =     {
        enabled = 1;
    };
    81 =     {
        enabled = 1;
    };
    82 =     {
        enabled = 1;
    };
}
```

Decoded (parameters = [ascii, keycode, modifier mask]; Space = keycode 49; ⌃=0x40000 ⌥=0x80000 ⌘=0x100000, so ⌃⌥⌘ = 1835008):

```
id 60: enabled=False keycode=49 mask=262144
id 61: enabled=False keycode=49 mask=786432
id 64: enabled=True keycode=49 mask=1048576
id 65: enabled=True keycode=49 mask=1572864
entries with keycode 49 and mask 1835008 (⌃⌥⌘Space): []
```

60 = ⌃Space, 61 = ⌃⌥Space (input sources), 64 = ⌘Space (Spotlight), 65 = ⌘⌥Space (Finder search). None is ⌃⌥⌘Space,
so the default DICTATE_TOGGLE stays ⌃⌥⌘Space. (⌃⌘Space, the emoji viewer, is an app menu item, not a symbolic hotkey, and differs by ⌥.)

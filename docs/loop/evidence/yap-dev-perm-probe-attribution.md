ROOT-CAUSE-PROBE yap-dev-perm-probe-attribution

# yap-dev-perm-probe-attribution — a dev build's permission probe reported the terminal's grants as Yap's

Walk D5, step 2c (P2). The dev build (`npm run desktop:dev`) answered
`accessibility=true input_monitoring=true` and the Permissions screen drew those
as Yap's green rows. They were not Yap's grants.

## Root cause

The dev binary is an unbundled, ad hoc (linker) signed Mach-O. It has no
designated requirement TCC can pin a grant to, and it is launched by a shell, so
TCC attributes it to its RESPONSIBLE process: the terminal that launched it.
`AXIsProcessTrusted`, `CGPreflight*Access` and `IOHIDCheckAccess` all answer
with the terminal's grants. `perm_probe::probe_all` had no notion of "who is
macOS answering about", so it classified those answers as Yap's `Granted`.

## Command-level probe (macOS 26.6.2, arm64), 2026-10-26

The dev binary (lane-a target dir, built by `cargo build`/`cargo test` of the app crate):

    $ codesign -dv --verbose=2 target-a/debug/wilson-voice
    Executable=/Users/wilsonguenther/code/wilson-voice-loop/target-a/debug/wilson-voice
    Identifier=wilson_voice-b8f282cef51aa9fa
    Format=Mach-O thin (arm64)
    CodeDirectory v=20400 size=814230 flags=0x20002(adhoc,linker-signed) hashes=25441+0 location=embedded
    Signature=adhoc
    Info.plist=not bound
    TeamIdentifier=not set

The installed build (team id redacted):

    $ codesign -dv --verbose=2 /Applications/Yap.app
    Executable=/Applications/Yap.app/Contents/MacOS/wilson-voice
    Identifier=com.wilsonguenther.wilson-voice
    Format=app bundle with Mach-O thin (arm64)
    Authority=Developer ID Application: Wilson Guenther (TEAMID)
    Authority=Developer ID Certification Authority
    Authority=Apple Root CA

Three facts distinguish them, and `perm_probe::running_as_bundle(exe_path,
bundle_id_of_main_bundle, code_signature)` checks all three: the executable sits
at `<Name>.app/Contents/MacOS/<exe>`, `NSBundle.mainBundle` has a bundle id, and
the signature's leaf certificate carries a team id (Developer ID / Apple
Development). The bundle id alone is not trusted, because a Tauri dev binary
can embed an Info.plist; the target-a debug binary probed above has no
`__info_plist` section, so on this machine it would also fail that check.

## The fix

- `ProbeState::DevHostAttributed` (new). When `running_as_bundle` is false for
  this process, `probe_all` marks every service DevHostAttributed and sets
  `ProbeReport.dev_host_attributed`. The raw answers are kept: `ServiceProbe::usable()`
  still lets the insert path use a grant the process really has (a dev build can
  paste under its terminal's grant), but no surface may call it Yap's.
- `reconcile_listen_event` leaves a DevHostAttributed row alone (it used to be
  able to flip it to Granted).
- `perm_probe::repair` refuses in a host-attributed process before any
  `tccutil` runs: a reset there would clear an identity macOS never consulted.
- The Permissions screen renders ONE line, "dev build: permissions are the host
  terminal's", with no rows and no Repair. `repairRows` already offers nothing
  for a non-denied state, so the banner's Repair stays hidden too.
- The installed build is unchanged: `/Applications/Yap.app/Contents/MacOS/wilson-voice`
  under Developer ID is `running_as_bundle == true`, and the probe is exactly as before.

## Tests

- Rust (lib): `perm_probe::running_as_bundle_tests::*` (8 tests), including the
  two spec cases, the "needs all three" matrix, every service marked, the raw
  answers kept, `repair` refusing with no commands run in the (ad hoc,
  unbundled) test process, and reconcile keeping the attribution.
- Vitest: `src/views/__tests__/permissions.devHost.test.tsx` renders the
  Permissions view with a DevHostAttributed probe (one line, no `<li>`, no
  Repair) and with a normal probe (rows unchanged).

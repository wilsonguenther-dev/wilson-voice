# Releasing Yap — the mandatory release smoke

Every DMG build runs the release smoke, and a build that fails it is not a release. `npm run desktop:build` is `scripts/sign-local.sh --tauri-build`, and its LAST step — after `tauri build` has signed (and, with the Apple secrets present, notarized and stapled) and after `verify_bundle` has read the signature back — is:

```
bash scripts/qa/release-smoke.sh --app <built Yap.app> --dmg <built .dmg> [--compare <installed or last-release Yap.app>]
```

It is `exec`'d, so its exit status is the build's exit status. In order, it runs:

1. `step=signature` — `codesign --verify --deep --strict` on the `.app` AND on the `.app` inside the DMG (mounted with `hdiutil attach -readonly -nobrowse`, detached after). Both must carry `Authority=Developer ID Application: ...` and the pinned `TeamIdentifier`. An Apple Development (LOCAL profile) build fails here by design: it is not releasable.
2. `step=dr_change` — `scripts/qa/perm-probe.sh --app <old> --compare <new>` (old = `YAP_SMOKE_COMPARE`, default `/Applications/Yap.app`). `dr_change=yes` means every Accessibility, Input Monitoring and Full Disk Access grant for the old build stops matching — the 0.9.0 failure — and it FAILS the build unless BOTH `YAP_ALLOW_DR_CHANGE=1` and `YAP_RELEASE_NOTE="<line>"` are set (`--allow-dr-change --release-note` on the script). The note is printed as `release_note=<line>`.
3. The launch — the new app with `--release-smoke` (`open -n`, a temp data dir holding read-only copies of the host's two signature ledgers), which probes all four services, registers the hotkeys, makes ONE insertion into a TextEdit document the smoke created (only when TextEdit is frontmost), and quits. Its `yap.log` goes through the same parser as `--parse`; a stale-identity grant (`state=StaleIdentity`, or `Denied` with `ledger=granted`), a failed Carbon registration, a Copy card while an insert grant is live, a document that does not read `yap release smoke ok`, or no `step=quit ok=true` within 10 s each fail the release. Quit any running Yap first: the smoke exits 2 rather than signal it.

**When `dr_change=yes` ships** (deliberately, with the two overrides), the release notes MUST carry the `release_note=` line and these Repair instructions: "This update changes Yap's code signature. If dictation stops pasting or the fn key stops working, open Yap → Settings → Permissions and click Repair on each row marked stale (or remove Yap from System Settings → Privacy & Security → Accessibility and Input Monitoring, re-add it, and relaunch)."

Read-only checks safe on any machine: `release-smoke.sh --signature-only --app <app> [--dmg <dmg>]`, `--dr-only --app <new> --compare <old>`, `--parse <yap.log>`, `--self-test [--force-fail STEP]`. The full runbook is [RELEASE.md](RELEASE.md).

# yap-p0-release-smoke — evidence (REQUIRE, captured before the gate was wired)

ROOT-CAUSE-PROBE yap-p0-release-smoke

Captured 2026-10-19T14:06:20Z on macOS 26.6.2, lane-a worktree, branch loop/yap-p0-release-smoke-signed-app-gate off origin/main 7e3276c. Read-only: no Yap was launched, TCC was not read or reset, DMGs were mounted -readonly -nobrowse and detached.

## 1. Where the .app/.dmg is produced (unmodified main 7e3276c)

`desktop/package.json`: `"desktop:build": "npm run sidecar && bash ../scripts/sign-local.sh --tauri-build"`. In `scripts/sign-local.sh --tauri-build` on main the bundle is produced, signed (and notarized when the Apple secrets are set) by `npx tauri build`, and the last step is `verify_bundle` (a `codesign -dv` read-back) — nothing launched it, so a build whose every grant was dead passed:

```
if [[ "$MODE" == "build" ]]; then
  export APPLE_SIGNING_IDENTITY="$IDENTITY"
  echo "${PROFILE} profile — tauri build will sign with: ${IDENTITY_CLASS}"
  cd "$DESKTOP"
  npx tauri build "$@"
  APP="$(ls -d "$SRC/target/release/bundle/macos/"*.app 2>/dev/null | head -1 || true)"
  [[ -n "$APP" ]] || APP="$(ls -d "${CARGO_TARGET_DIR:-$SRC/target}/release/bundle/macos/"*.app 2>/dev/null | head -1 || true)"
  [[ -n "$APP" ]] || die "tauri build produced no .app to verify."
  verify_bundle "$APP"
  exit 0
fi
```

`docs/RELEASING.md` on main (the whole file — a manual step, no gate):

```
# Releasing Yap — permission-identity check

Before publishing any build, the release seat runs `bash scripts/qa/perm-probe.sh --app /Applications/Yap.app --compare <the new Yap.app>` and, when it prints `dr_change=yes`, copies its `warning:` line verbatim into the release notes (every Accessibility, Input Monitoring and Full Disk Access grant for the old build stops matching — the 0.9.0 failure). The full runbook is [RELEASE.md](RELEASE.md).
```

The smoke hooks in right after `verify_bundle` in that block: `exec bash scripts/qa/release-smoke.sh --app <app> --dmg <dmg>` (signature → dr_change → launch).

## 2. perm-probe.sh --compare — built app vs /Applications/Yap.app (dr_change, the 0.9.0 signal)

No locally built bundle exists on this host (`target-a/release/bundle` and `wilson-voice/desktop/src-tauri/target/release/bundle` are absent; this seat does not run `desktop:build`). The nearest real built artifacts are the shipped DMGs in ~/Desktop/_Organized/Archives. The 0.9.0 DMG's app vs the installed 0.9.2 (designated requirements elided to the team-masked form):

```
$ bash scripts/qa/perm-probe.sh --app /Applications/Yap.app --compare <Yap-0.9.0-arm64.dmg mounted>/Yap.app
app_path=/Applications/Yap.app
compare_path=/private$TMPDIR/yapev.XXXX/Yap.app
dr_change=no
exit=0
```

dr_change=no: 0.9.0 and 0.9.2 are both Developer ID / same team, so the requirement is identical. The 0.9.0 break was against the Apple Development builds before it (no such bundle survives on this host to re-probe), which is exactly the dr_change=yes path the release smoke now refuses without --allow-dr-change + --release-note. The yes path is proven against two differently-signed bundles in the self-test (Calculator vs TextEdit).

## 3. The new gate's read-only steps on the real shipped artifacts (positive controls)

```
$ bash scripts/qa/release-smoke.sh --signature-only --app /Applications/Yap.app --dmg ~/Desktop/_Organized/Archives/Yap-0.9.2-arm64.dmg
signature app: ok (strict verify, Developer ID Application, team pinned)
signature dmg-app: ok (strict verify, Developer ID Application, team pinned)
step=signature ok
release-smoke: OK
exit=0

$ bash scripts/qa/release-smoke.sh --signature-only --app <0.9.0 DMG>/Yap.app
signature app: ok (strict verify, Developer ID Application, team pinned)
signature dmg: not given (app only)
step=signature ok
release-smoke: OK
exit=0

$ bash scripts/qa/release-smoke.sh --dr-only --app <0.9.0 DMG>/Yap.app --compare /Applications/Yap.app
dr_change=no (old=/Applications/Yap.app new=$TMPDIR/yapev.XXXX/Yap.app)
step=dr_change ok
release-smoke: OK
exit=0
```

## 4. The 0.9.0 log, through the new parser

```
$ bash scripts/qa/release-smoke.sh --parse scripts/qa/fixtures/release-smoke-negative-2026-10-17.log
step=probe fail (accessibility:StaleIdentity(ledger=granted) listenevent:StaleIdentity(ledger=granted) postevent:StaleIdentity(ledger=none))
step=hotkeys ok (registered fn_tap=armed_no_delivery)
step=insert fail (path=CopyCard ok=true with an insert grant the ledger saw working)
step=quit ok
release-smoke: FAIL step=probe
exit=1
```

## 5. Fix round 2 — acceptance deviations (for the harness owner)

The item acceptance block was re-run line by line on the fix-round-2 tree (raw rc lines are in the PR body). One line fails, on purpose:

- `bash scripts/qa/release-smoke.sh --parse scripts/qa/fixtures/release-smoke-matrix/absent-grants-copy-card.log` exits **1**, and the spec expects 0. This comes from the round-0 Independent B2 fail-closed fix. A host with no grants plus a Copy card cannot be told apart from the 0.9.0 class, so the parser refuses it. The spec rows "Denied/NotDetermined AND ledger=none -> ok" and "absent-grants-copy-card.log (exit 0)" are fail-open. The proposed amendment is `! bash scripts/qa/release-smoke.sh --parse scripts/qa/fixtures/release-smoke-matrix/absent-grants-copy-card.log`.
- `awk '/codesign/ {c=NR} /release-smoke.sh/ {r=NR} ...' scripts/sign-local.sh` used to exit 1, because the rebase onto main put the default sign mode's `codesign --force` after the build mode's `exec release-smoke.sh`. The fix was not a comment line. The default sign mode now runs, and exits, before the build mode, so the `exec` of the smoke is the last statement in the file. It exits 0 now.

The release paths in docs/RELEASE.md (Tagged, Manual, and "any other DMG") now go through the smoke as well. CI runs `--signature-only` after tauri-action, and a tag draft is unsmoked and unpublishable until the full smoke has run on its downloaded DMG.

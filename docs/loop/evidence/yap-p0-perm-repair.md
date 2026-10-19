# yap-p0-perm-repair — evidence (REQUIRE, captured on unmodified main b5b446c before any code)

ROOT-CAUSE-PROBE yap-p0-perm-repair

Captured 2026-10-19T00:19:56Z on macOS 26.6.2 (25G83), lane-a worktree, branch detached at origin/main b5b446c.

## 1. bash scripts/qa/perm-probe.sh (read-only, unmodified main) — exit 0

```
app_path=/Applications/Yap.app
bundle_id=com.wilsonguenther.wilson-voice
version=0.9.1
team_id=VHYV8C2JNU
authority_leaf=Developer ID Application: Wilson Guenther (VHYV8C2JNU)
cdhash=ca745694f2cc18904b90ea372ab28b8204dffd1f
designated_requirement=identifier "com.wilsonguenther.wilson-voice" and anchor apple generic and certificate 1[field.1.2.840.113635.100.6.2.6] /* exists */ and certificate leaf[field.1.2.840.113635.100.6.1.13] /* exists */ and certificate leaf[subject.OU] = VHYV8C2JNU
same_id_bundles=/Users/wilsonguenther/code/wilson-voice/desktop/target/release/bundle/macos/Yap.app,/Users/wilsonguenther/Desktop/_Organized/Archives/Yap-0.8.0-backup.app
lsregister_entry_1=/Users/wilsonguenther/Desktop/_Organized/Archives/Yap-0.8.0-backup.app stale=0
lsregister_entry_2=/Users/wilsonguenther/code/wilson-voice/desktop/target/release/bundle/macos/Yap.app stale=0
lsregister_entry_3=/private/tmp/yap-release-20261017T0747Z/desktop/target/release/bundle/macos/Yap.app stale=0
lsregister_entry_4=/Applications/Yap.app stale=0
lsregister_count=4
running_pids=49157
# helper values describe the QA helper, NOT Yap (an ad-hoc-signed binary; macOS answers for the terminal that launched it)
helper_ax_trusted=true
helper_listen_event=true
```

The installed /Applications/Yap.app runs under `Developer ID Application: Wilson Guenther (VHYV8C2JNU)`. 0.8.0 and earlier ran under Apple Development; TCC pins every grant to the designated requirement above, so a grant made for the old identity does not match this one while System Settings still lists Yap as ON. Note `same_id_bundles` also lists an archived Yap-0.8.0-backup.app — another identity that may hold the visible switch.
The helper_* lines describe the ad-hoc QA helper (answered for the launching terminal), NOT Yap.

## 2. A cargo test that FAILS on main: no ledger entry + denied probe -> stale_grants() is empty

CORRECTION (fix round 1): the original text here said this was "Wilson's real state". It is not, and the evidence never read the ledger. A read-only `cat ~/Library/Application Support/WilsonVoice/permission-signature.json` (mtime 17 Oct 02:51) shows accessibility AND inputMonitoring both recorded under `Developer ID Application: Wilson Guenther (VHYV8C2JNU)`, cdhash `ca745694f2cc18904b90ea372ab28b8204dffd1f`, version 0.9.1 — the installed /Applications/Yap.app in section 1. `last-run-signature.json` does not exist there: this PR introduces it. So the Apple Development -> Developer ID change that already happened can NEVER be classified StaleIdentity on his machine; on this branch `classify(Denied, None, None, running)` is still `Denied` (the red test below is NOT turned green). What this PR does for his machine: any denied grant reads Denied and gets the same one-click Repair (`canRepair` includes `denied`). StaleIdentity covers only identity changes that happen AFTER a build with this PR has launched once (last-run file), or a ledger entry under a different authority / team. A same-team Developer ID update (new cdhash / version) is NOT a different identity: `ledger_same_team_update_denied_is_denied_not_stale` pins that, using his real ledger values.

Original (incorrect) premise: the perm_truth ledger (#211) shipped after 0.9.0, so it never saw a grant work under the old signature. Scratch test `tests/zz_red_on_main.rs` (run on main, then deleted; never committed):

```rust
let stale = stale_grants(&TrustLedger::default(), false, GrantState::Denied, &running, "/Applications/Yap.app");
assert!(!stale.is_empty(), "stale_grants() is EMPTY on main: the stale identity is invisible");
```

`cargo test --features custom-protocol --test zz_red_on_main -- --nocapture` -> exit 101:

```
running 1 test
main: ledger=none probe=denied stale_grants=[]

thread 'stale_state_visible_with_no_ledger_and_denied_probe' (17559093) panicked at src-tauri/tests/zz_red_on_main.rs:16:5:
stale_grants() is EMPTY on main: the stale identity is invisible
note: run with `RUST_BACKTRACE=1` environment variable to display a backtrace
test stale_state_visible_with_no_ledger_and_denied_probe ... FAILED

failures:

failures:
    stale_state_visible_with_no_ledger_and_denied_probe

test result: FAILED. 0 passed; 1 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s

error: test failed, to rerun pass `--test zz_red_on_main`
```

On this branch `stale_identity_without_ledger_after_signature_change` in tests/perm_repair.rs is NOT the same situation: it adds `last_run=changed`, a state that needs a last-run file an earlier build of this PR wrote, so it covers future identity changes only. It prints `perm_repair: ledger=none last_run=changed probe=denied state=StaleIdentity` and passes.

## 3. tccutil service names on this macOS (tccutil itself was NEVER run with reset)

`tccutil` with no arguments (usage only, changes nothing):

```
tccutil: Usage: tccutil reset SERVICE [BUNDLE_ID]
tccutil_noargs_exit=64
```

`man tccutil` documents the syntax (`tccutil reset SERVICE [BUNDLE_ID]`) but lists no service names, so the names were read out of tccd itself:

```
# strings -a /System/Library/PrivateFrameworks/TCC.framework/Support/tccd | grep kTCCService(Accessibility|ListenEvent|PostEvent|Microphone)
kTCCServiceAccessibility
kTCCServiceListenEvent
kTCCServiceMicrophone
kTCCServicePostEvent
# plutil on Resources plists mentioning the services:
```

tccutil takes the name without the `kTCCService` prefix: Accessibility, ListenEvent (Input Monitoring), PostEvent, Microphone. These are exactly `perm_probe::Service::tcc_name()`.

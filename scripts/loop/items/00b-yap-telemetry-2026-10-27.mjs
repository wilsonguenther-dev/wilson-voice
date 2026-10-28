// yap-telemetry-2026-10-27 — Wilson 2026-10-27 (verbatim): "Just add telemetry where we need to so we can update".
// Written 2026-10-27T17:20Z (date -u) by the harness seat. FIX/FEATURE, class STANDARD, builder OPUS (no
// builder:'sonnet' tag), NO panel. BUILD-ONLY run (args.landBar 'ci'): the PR merges on GitHub CI green.
//
// DEPENDENCY ROOT. Every later item's builder prompt carries the TELEMETRY (Wilson 2026-10-27) block, which tells it
// to record through desktop/src-tauri/src/diagnostics.rs once this item is on origin/main. The harness has NO
// `after:` / dependsOn edge (build order = file order; lanes = files, resolved at run time against args.lanes), so
// this item is (a) in the 00b file, straight after the harness (00) and the 2026-10-26 fixes (00a), and (b) PINNED
// to lane A with `lane: 'A'` (build.mjs stamps the pin; with lanes:3 the 00b file would otherwise round-robin onto
// lane C). Items in the same part on lanes B/C do not wait for it — they log through the `log` crate as the
// TELEMETRY block says, and the drain lands this PR at the part boundary (or a builder's LAND step earlier, on CI
// green), so items in later parts build on top of it.
//
// HAZARD (same as 07/07b/08/00a): no agent launches the GUI app, posts events into, signals or quits Wilson's running
// /Applications/Yap.app, or writes TCC on this machine. Proof is cargo test + vitest; the on-device look goes in the
// PR body as WILSON'S WALK. SHARED PREAMBLE + STANDARD GATE: 00-y0-harness-and-gates.mjs.

ITEMS.push({
  id: 'yap-telemetry-diagnostics', prompt: 'yap-telemetry-2026-10-27', branch: 'loop/yap-telemetry-diagnostics', gated: null,
  lane: 'A',
  title: 'One Diagnostics surface — structured event ring buffer fed by the logger, a Settings panel with a live health line, and a redacted "Export diagnostics" bundle',
  preflight: `
    test -f desktop/src-tauri/src/diagnostics.rs && grep -q 'mod diagnostics' desktop/src-tauri/src/lib.rs && test -f desktop/src/diagnostics.ts && grep -q 'export_diagnostics' desktop/src-tauri/src/lib.rs && grep -q 'diag_warn!' desktop/src-tauri/src/paste.rs && grep -q 'diag_warn!' desktop/src-tauri/src/meeting_pipeline.rs
  `,
  spec: `
    Panel: not required — class STANDARD, builder opus. Wilson 2026-10-27: "Just add telemetry where we need to
    so we can update". Today a failure on Wilson's machine is a grep through yap.log: there is no in-app view of
    what just went wrong, no single health line, and no one-click bundle he can hand back to the loop.

    WHAT EXISTS (reuse it — do not build a second one of anything):
      - desktop/src-tauri/src/logging.rs: env_logger teed to <data_dir>/logs/yap.log (TeeWriter) + panic hook.
      - desktop/src-tauri/src/latency.rs: PipelineSpans, the key=value latency line per dictation.
      - desktop/src-tauri/src/support.rs: the support bundle; redact / redact_line (username scrub),
        username_from_home, permissions_block, models_block. Its redaction is the redaction — call it.
      - Error surfaces: Result<_, String> / CommandError {code, message} -> toast() in desktop/src/appShell.ts;
        status.lastError via app.emit("status", build_status(..)) in lib.rs -> desktop/src/pill/live.ts.
      - Settings views live in desktop/src/views/settings/ (Advanced.tsx is the debug-ish one).
    There is NO metrics sink and NO ring buffer on main (pre-flight proves diagnostics.rs is absent).

    DO (names are contract)
      1. desktop/src-tauri/src/diagnostics.rs, declared "mod diagnostics;" in lib.rs:
         - pub struct DiagEvent { ts_ms: i64, level: String, module: String, msg: String, fields: BTreeMap<String, String> }
           (serde Serialize).
         - pub struct DiagRing — fixed capacity (pub const DIAG_RING_CAPACITY: usize = 500), oldest evicted first,
           Mutex-guarded global (OnceLock) so any thread records without plumbing state.
         - FED BY THE EXISTING LOGGER: logging.rs's tee pushes every record at INFO and above into the ring
           (level, target as module, message). No second logger, no new crate.
         - macros diag_error! / diag_warn! / diag_info! (exported from diagnostics.rs) taking a module, a message and
           key = value fields: they record the event WITH its fields AND emit the same line through log::error! /
           log::warn! / log::info! (so yap.log is unchanged). The tee must not record that line a second time
           (e.g. a dedicated log target the tee skips) — one event per occurrence, proven by a test.
         - pub fn record_duration(module, name, ms) and pub fn bump(module, counter): counters/durations kept beside
           the ring (last value + count + max), included in the snapshot and the export.
      2. HEALTH LINE — #[tauri::command] diagnostics_snapshot -> { events: last N, counters, health } where health
         is: permissions state (the existing permissions/perm_truth report: microphone, accessibility, input
         monitoring, post-event), model loaded (selected model + loaded yes/no), hotkey registered (yes/no + the
         chord), meeting pipeline state (idle/recording/processing/failed + last stage), insert bridge state (last
         insert path + last outcome from paste.rs). Registered in tauri::generate_handler!.
      3. EXPORT — #[tauri::command] export_diagnostics writes TWO files to ~/Downloads:
         yap-diagnostics-<UTC stamp>.json (snapshot) and .txt (human-readable: health line, counters, events).
         REDACTED: run every string through support.rs redact with the username; drop any field whose key is
         text / transcript / content / snippet; reduce any absolute path to its basename. Returns both paths;
         the UI reveals them (support.rs reveal_in_finder). Failure -> Err(String) that the panel shows.
      4. FRONTEND — desktop/src/diagnostics.ts (types + invoke wrappers + pure formatters: formatHealth,
         formatEvent) and a "Diagnostics" panel desktop/src/views/settings/Diagnostics.tsx reached from the Settings
         view (its own tab/section, not a modal): the health line at the top, the last events (newest first, level
         coloured with existing tokens.css variables), counters, a Refresh button and an "Export diagnostics" button
         that shows the written paths or the error via the existing toast. Same visual language as the other
         settings sections — no new design system.
      5. ROUTE THE ERROR SITES THROUGH IT — every existing log::error! and log::warn! in these files becomes
         diag_error! / diag_warn! with its values as fields (no behaviour change otherwise): dictation insert path
         desktop/src-tauri/src/paste.rs and paste_tx.rs; meeting pipeline desktop/src-tauri/src/meeting_pipeline.rs;
         model download desktop/src-tauri/src/models.rs; permissions desktop/src-tauri/src/mic_auth.rs,
         perm_truth.rs and permissions.rs. And record: dictation insert latency (record_duration "insert" ms at the
         end of each insert), meeting pipeline stage durations (one per stage), model download duration + bytes.
      6. Tests: Rust unit tests in diagnostics.rs — ring_buffer_evicts_oldest_at_capacity,
         ring_buffer_keeps_order, macro_records_once_not_twice, export_redacts_username_and_transcript_fields,
         export_reduces_paths_to_basename; vitest desktop/src/diagnostics.test.ts for the formatters.

    DO NOT: send anything off the machine (no network, no upload — the bundle is a local file Wilson attaches
    himself); log transcript text or audio; rename the bundle identifier or the data directory; add a crate.
  `,
  acceptance: `
    test -f desktop/src-tauri/src/diagnostics.rs
    grep -q 'mod diagnostics' desktop/src-tauri/src/lib.rs
    grep -q 'export_diagnostics' desktop/src-tauri/src/lib.rs
    grep -q 'diagnostics_snapshot' desktop/src-tauri/src/lib.rs
    test -f desktop/src/diagnostics.ts && test -f desktop/src/views/settings/Diagnostics.tsx
    test "$(grep -c 'log::\\(error\\|warn\\)!' desktop/src-tauri/src/paste.rs desktop/src-tauri/src/paste_tx.rs desktop/src-tauri/src/meeting_pipeline.rs desktop/src-tauri/src/models.rs desktop/src-tauri/src/mic_auth.rs desktop/src-tauri/src/perm_truth.rs desktop/src-tauri/src/permissions.rs | awk -F: '{s+=$2} END {print s}')" = 0
    test "$(grep -c 'diag_\\(error\\|warn\\)!' desktop/src-tauri/src/paste.rs desktop/src-tauri/src/paste_tx.rs desktop/src-tauri/src/meeting_pipeline.rs desktop/src-tauri/src/models.rs desktop/src-tauri/src/mic_auth.rs desktop/src-tauri/src/perm_truth.rs | awk -F: '{s+=$2} END {print s}')" -ge 15
    grep -q 'record_duration' desktop/src-tauri/src/paste.rs && grep -q 'record_duration' desktop/src-tauri/src/meeting_pipeline.rs && grep -q 'record_duration' desktop/src-tauri/src/models.rs
    (cd desktop && npm ci)
    (cd desktop && npm run build)
    (cd desktop && npx vitest run src/diagnostics)
    (cd desktop/src-tauri && cargo test --features custom-protocol --lib diagnostics)
  `,
})

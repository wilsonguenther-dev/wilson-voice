// yap24-X — EXPANSIONS: the things that make Yap better than Wispr Flow rather than equal to it.
// Written 2026-09-26 by the architecture audit (docs/ARCHITECTURE-AUDIT-2026-09-26.md §7). Wispr is
// a cloud write-buffer that "literally cannot work on a plane" (Notes/Wispr-Full-Parity-Research-
// 2026-08-09.md); Yap's moat is local. Each item keeps every byte on the Mac.
// All gated on the panel; none starts before the notetaker chain (01) closes.
// SHARED PREAMBLE + STANDARD GATE: 00-y0-harness-and-gates.mjs.

ITEMS.push({
  id: 'yap24-X1', prompt: 'yap24-X', branch: 'loop/yap24-x1-notes-mirror-to-a-markdown-folder', gated: 'panel',
  title: 'Your notes outlive the app: every meeting and (optionally) every dictation mirrors to Markdown in a folder you choose',
  preflight: `
    grep -q "notes_mirror_dir" desktop/src-tauri/src/lib.rs
  `,
  spec: `
    Panel: APPROVED-WITH-CHANGES 2026-10-26, DEFERRED to the next release (not in panelApproved).
    DEPENDS: yap24-NT4 (notes exist), yap24-NT9

    EVIDENCE: Wilson 2026-08-09 (memory project_yap_build_state): "full transcript/file management
    layer so AIs can always retrieve transcripts even if Yap breaks". Today the only copy is the
    SQLite DB under Application Support.

    DO: Settings -> Notes folder (default off). When set, each completed meeting writes
    <folder>/Yap Meetings/<date> <title>.md (the NT4 export) and re-writes it when notes change;
    an optional daily dictation log <folder>/Yap Dictations/<date>.md. Atomic writes; never
    deletes a user file; a missing folder pauses the mirror with one sentence in Settings.
    Obsidian-friendly front matter (date, duration, kind, attendees if known).

    Panel revisions 2026-10-26 (Senior Panel plan-audit, two seats + synthesis; applied as preflight/acceptance GATES, not only prose):
      - DUEL, seat B (DEFER next release) vs seat A (rescope onto #250's Markdown export). WINNER: both, and not by averaging: the scope is seat A's, the
        timing is seat B's. Evidence: this item builds on the "NT4 export", but the notes export it mirrors is PR #250's (Markdown sections in
        meetings.summary; not on main yet, 11 tests red at audit time), so it cannot be built today; and a mirror folder is a nicety for a launch
        with zero users beyond Wilson (a manual Markdown export already exists).
      - RESCOPED: DEPENDS is now PR #250's stable Markdown export (export_meeting_markdown plus the summary sections), NOT yap24-NT4 rows (NT4 no
        longer creates rows) and not NT9. Mirror writes exactly what that export produces. DEFER: next release.
  `,
  acceptance: `
    cd desktop && npm ci && cd src-tauri
    cargo test --features custom-protocol --lib notes_mirror
    cargo clippy --all-targets --features custom-protocol
  `,
})

// yap24-X2 — CUT 2026-10-26 by the Senior Panel plan-audit (object removed; the harness has no retired/status field, removal is the mechanism).
// Why: it duplicates the "Set up MCP" row of the ungated yap-notetaker-onboard (a stdio MCP server over read-only SQLite), and it contradicts that item's
// rule that dictation history is never exposed. MCP as a whole is DEFERRED to after the first 100 users: the yap-notetaker-onboard MCP step was struck
// today (the row shows as Coming), so there is still exactly ONE owner of the MCP deliverable, and it is not built in v1. Spec is in git history.

ITEMS.push({
  id: 'yap24-X3', prompt: 'yap24-X', branch: 'loop/yap24-x3-ask-your-meeting-locally', gated: 'panel',
  title: 'Ask your meeting: "what did I miss / what did we decide" answered by the local model, every answer citing its transcript lines',
  preflight: `
    test -f desktop/src-tauri/tests/meeting_ask_cites_segments.rs
  `,
  spec: `
    Panel: APPROVED-WITH-CHANGES 2026-10-26, DEFERRED to after the first 100 users (not in panelApproved).
    DEPENDS: yap24-NT4, yap24-NT6

    EVIDENCE: Wispr's 2026 notetaker ships "What did I miss" and a notetaker chat (parity note
    §2.7, lines 214-215) — cloud. Yap has the yap-polish sidecar and GBNF-constrained output
    (summarize.rs) already.

    DO: an Ask box on a meeting; retrieval = FTS5 over that meeting's segments (no embeddings in
    v1); the sidecar answers with JSON {answer, citations:[segment ids]}; an answer without a
    valid citation is refused and shown as "not in this meeting". Runs on the summary worker,
    never concurrently with dictation.

    Panel revisions 2026-10-26 (Senior Panel plan-audit, two seats + synthesis; applied as preflight/acceptance GATES, not only prose):
      - DEFER: after the first 100 users. Wispr's "What did I miss" is cloud; here a 1.5B model on an 8 GB floor machine is conditional on serialization
        (see NT4 R4). DEPENDS: yap24-NT4 (rescoped: the measured notes eval and RSS ceiling) and the #250 notes; no rows exist to retrieve from until then.
  `,
  acceptance: `
    cd desktop && npm ci && cd src-tauri
    cargo test --features custom-protocol --test meeting_ask_cites_segments
    cargo clippy --all-targets --features custom-protocol
  `,
})

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
    Panel: pending
    DEPENDS: yap24-NT4 (notes exist), yap24-NT9

    EVIDENCE: Wilson 2026-08-09 (memory project_yap_build_state): "full transcript/file management
    layer so AIs can always retrieve transcripts even if Yap breaks". Today the only copy is the
    SQLite DB under Application Support.

    DO: Settings -> Notes folder (default off). When set, each completed meeting writes
    <folder>/Yap Meetings/<date> <title>.md (the NT4 export) and re-writes it when notes change;
    an optional daily dictation log <folder>/Yap Dictations/<date>.md. Atomic writes; never
    deletes a user file; a missing folder pauses the mirror with one sentence in Settings.
    Obsidian-friendly front matter (date, duration, kind, attendees if known).
  `,
  acceptance: `
    cd desktop && npm ci && cd src-tauri
    cargo test --features custom-protocol --lib notes_mirror
    cargo clippy --all-targets --features custom-protocol
  `,
})

ITEMS.push({
  id: 'yap24-X2', prompt: 'yap24-X', branch: 'loop/yap24-x2-local-mcp-server-over-yap-history', gated: 'panel',
  title: 'A local, read-only MCP server so Claude and other agents can search your dictations and meeting notes — nothing leaves the Mac',
  preflight: `
    test -d desktop/yap-mcp
  `,
  spec: `
    Panel: pending (new surface: a local server; the panel decides stdio-only vs a socket)
    DEPENDS: yap24-NT4

    EVIDENCE: parity teardown epic list ("Yap MCP server"); Wispr ships MCP ("AI Tools") in its
    settings (parity note §2.7 and line 235). Yap already has FTS5 over transcripts and meeting
    segments (db.rs).

    DO: a small stdio MCP binary (workspace member desktop/yap-mcp) that opens the SQLite DB
    read-only (SQLITE_OPEN_READ_ONLY, WAL-safe) and exposes search_dictations, search_meetings,
    get_meeting_notes, list_recent. No network listener. Settings shows the one-line config to
    paste into an MCP client. Tests over a fixture DB.
  `,
  acceptance: `
    test -d desktop/yap-mcp
    cd desktop && cargo test -p yap-mcp
  `,
})

ITEMS.push({
  id: 'yap24-X3', prompt: 'yap24-X', branch: 'loop/yap24-x3-ask-your-meeting-locally', gated: 'panel',
  title: 'Ask your meeting: "what did I miss / what did we decide" answered by the local model, every answer citing its transcript lines',
  preflight: `
    test -f desktop/src-tauri/tests/meeting_ask_cites_segments.rs
  `,
  spec: `
    Panel: pending
    DEPENDS: yap24-NT4, yap24-NT6

    EVIDENCE: Wispr's 2026 notetaker ships "What did I miss" and a notetaker chat (parity note
    §2.7, lines 214-215) — cloud. Yap has the yap-polish sidecar and GBNF-constrained output
    (summarize.rs) already.

    DO: an Ask box on a meeting; retrieval = FTS5 over that meeting's segments (no embeddings in
    v1); the sidecar answers with JSON {answer, citations:[segment ids]}; an answer without a
    valid citation is refused and shown as "not in this meeting". Runs on the summary worker,
    never concurrently with dictation.
  `,
  acceptance: `
    cd desktop && npm ci && cd src-tauri
    cargo test --features custom-protocol --test meeting_ask_cites_segments
    cargo clippy --all-targets --features custom-protocol
  `,
})

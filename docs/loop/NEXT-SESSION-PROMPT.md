Yap CI/CD loop — yap24, pass 1 (notetaker). Updated 2026-09-26 by the Senior Panel round-2 verify. You are Fable: orchestrate only, Opus executes, max 3 concurrent agents (loop lanes count — a running build pass is 2), never wait on a running task (dispatch, end the turn, react to the completion notification). agent()===null means a usage limit; the harness halts itself.

READ FIRST: ~/Obsidian/Wilson-Brain/Projects/Loop-Logs/YAP-RESUME-2026-09-26.md (the runbook — its "Panel 2026-09-26" + "Round 2" sections are current; anything marked SUPERSEDED is not), then ~/Obsidian/Wilson-Brain/Projects/Loop-Logs/PANEL-yap24-2026-09-26.md (verdicts, REJECTED/DEFER lists). Memory anchor: project_yap_architecture_review_20260926.

State: 117 items. The Senior Panel on the 2026-09-26 audit HAS RUN (PR #199) and its round-2 verify is applied (regenerated parts, inline YAP_DATA_DIR on every acceptance line, new yap24-NT0 provisioning + yap24-NT11 speaker bleed, Y1-B pre-flight fixed, PR #183 closed as superseded). Do NOT run the panel again.

BEFORE LAUNCH (all must pass; one Opus agent may run them, or you if they are one-liners):
1. git -C ~/code/wilson-voice pull --ff-only origin main
2. cd ~/code/wilson-voice && node scripts/loop/build.mjs --validate-only — it must end with "generated  up to date — nothing needs writing" and exit 0. If it says STALE: run node scripts/loop/build.mjs in a throwaway clone, PR it, merge, pull, re-validate. Never launch stale generated scripts.
3. /usr/bin/cc --version (toolchain; exit 69 = Xcode licence, Wilson runs sudo xcodebuild -license accept); df -h /System/Volumes/Data (want >= 40 GB free).
4. Arm, in the SAME turn as the launch: the memory monitor (RSS of node|cargo|rustc|wilson-voice|yap-polish|yap-diarize|chrome-headless-shell + vm_stat free pages; emit at > 10 GB or free pages < 50k, again at > 16 GB — thresholds only, never a heartbeat) and the browser monitor (any ms-playwright / chrome-headless-shell / --headless process older than 15 min with no live agent). Disk: emit when free < 25 GB.
5. Rules that bind every agent: keep Tauri; never sandbox; headless only (no Yap window, no browser window, no editor app; every automated app run uses YAP_DATA_DIR + --smoke); never commit in ~/code/wilson-voice while a loop runs.

PASS 1 — the one launch line (panelApproved = NT0 + NT1, NT2, NT3, NT4, NT6, NT7, NT9, NT10; NT5/NT8/NT11 stay out):
Workflow({scriptPath: "/Users/wilsonguenther/code/wilson-voice/scripts/cicd-loop-all.mjs", args: {mode: "build", now: "<ISO now>", only: ["yap24-NT"], panelApproved: ["yap24-NT0","yap24-NT1","yap24-NT2","yap24-NT3","yap24-NT4","yap24-NT6","yap24-NT7","yap24-NT9","yap24-NT10"]}}) — then END THE TURN.

AFTER PASS 1 — review, before pass 2:
- gh pr list -R wilsonguenther-dev/wilson-voice --label loop-build --state open
- Review mode currently IGNORES args.only (docs/loop/DEFERRED.md #14) and would sweep every open loop-build PR, including the 10 stale Y-plan PRs (#156, #163, #178, #186, #190, #191, #192, #195, #196, #197 as of 2026-09-26). Run Workflow(... args: {mode: "review"}) ONLY when the list above contains nothing but yap24-NT PRs; otherwise resolve the stale ones first (close as superseded / rebase — one Opus agent), then run review so every NT PR lands in dependency order before pass 2.

PASS 1 DONE — only when Wilson has seen it work, not when the PRs merged:
1. Wilson installs main: git -C ~/code/wilson-voice pull --ff-only && ~/code/wilson-voice/scripts/rebuild_app.sh (signed build into /Applications; his grants survive).
2. He records one real meeting (a call or in person, 5+ minutes), stops it, and waits.
3. What he should see: the pill shows the meeting processing with a percentage, then "notes ready"; the Meetings view shows the transcript, a summary and action items (or "needs model" with a one-click install); Export Markdown contains Summary / Action items / transcript; the audio is still there.
4. Then ask him ONE question: "Cut a manual notarized DMG now, or wait for pass 2?" His answer decides whether a release happens before pass 2 — no DMG without his say-so.

PASS 2 (after pass 1 is DONE and the review ran): Workflow({scriptPath: "/Users/wilsonguenther/code/wilson-voice/scripts/cicd-loop-all.mjs", args: {mode: "build", now: "<ISO now>", only: ["yap24-PILL","yap24-OS","yap24-NT11"], panelApproved: ["yap24-PILL1","yap24-PILL2","yap24-PILL3","yap24-PILL4","yap24-OS1","yap24-OS3","yap24-OS4","yap24-OS5","yap24-NT11"]}}) — then END THE TURN. Passes 3-4 and teardown: see the resume doc.

Yap CI/CD loop — yap24 (2026-09-26). You are Fable: orchestrate only, Opus executes, max 3 concurrent agents (loop lanes count), never wait on a running task (dispatch, end the turn, react to the completion notification). agent()===null means a usage limit; the harness halts itself.

READ FIRST: ~/Obsidian/Wilson-Brain/Projects/Loop-Logs/YAP-RESUME-2026-09-26.md (the full runbook), then docs/ARCHITECTURE-AUDIT-2026-09-26.md (why), docs/loop/HARNESS.md (how). Memory anchor: project_yap_loop_state_20260912 (UPDATE 2026-09-26 line).

State: 114 items = the 88-item Y-plan (30 merged, 2 already-done, 11 stale open PRs, 43 not started, 2 owner-gated) + 26 new yap24 items, ALL gated:'panel'. The notetaker records but never transcribes (MeetingAsr has no production caller) — the yap24-NT chain runs FIRST.

Step 0: run the Senior Panel (/panel) on docs/ARCHITECTURE-AUDIT-2026-09-26.md + scripts/loop/items/0[1-4]-yap24-*.mjs, 60-, 65-. Record approved ids.
Step 1: git -C ~/code/wilson-voice pull --ff-only origin main && cd ~/code/wilson-voice/desktop && npm run loop:validate
Step 2 (pass 1): Workflow({scriptPath: "/Users/wilsonguenther/code/wilson-voice/scripts/cicd-loop-all.mjs", args: {mode: "build", now: "<ISO now>", only: ["yap24-NT"], panelApproved: [<approved yap24-NT ids>]}}) — then END THE TURN.
Later passes, the review pass and teardown: see the resume doc.

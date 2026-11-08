# Capabilities the landing page claims (yap-1107-landing-page-tells-the-truth)

Built 2026-11-08T04:39:01Z against origin/main `e18a12d61c7f4916335c841093e23a0d9ae878b7`.

Source of "landed": docs/loop/ITEMS-2026-11-07-loop3.json is not tracked on origin/main (it lives untracked in the
loop's main checkout, and its loop-three statuses lag the merges). So an item counts as landed only when the item file
says `landed` OR its `Merge <id> (loop3)` commit is on origin/main with no later `Revert "Merge <id> ..."`
(checked with `git log origin/main --format=%s`). Items whose newest main commit is a revert are excluded:
yap-1107-parity-pill, yap-1107-parity-home, yap-1107-parity-news, yap-1106-main-character-switch.
Deliberately excluded although landed: yap-1106-friend-visits (Social / visiting stays off the page, Wilson).

| Item id | One-line claim | Section |
|---|---|---|
| yap-1106-fb-03-pill-over-every-app | One pill floats over every app | 1 The pill |
| yap-1106-one-pill, yap-1107-one-pill-during-meeting | The meeting clock lives inside the same pill, next to Yappy | 1 The pill |
| yap-wf-01-click-the-pill-to | Click the pill to start and stop | 1 The pill |
| yap-1106-pill-vertical-dock-world, yap-wf-02-the-pill-shrinks-to | Drag to the bottom or either side | 1 The pill |
| yap-1106-yappy-detail-pass, yap-1106-pill-props-detail | Yappy at his typewriter in the pill | 1 The pill |
| yap-1106-sal-and-mochi | Sal lights the furnace at launch; Mochi on the door keeps the crew quiet while you present or share | 2 The crew |
| yap-1106-launch-walk | The engine is ready when the fire is (launch bound to engine load) | 2 The crew |
| yap-1106-news-duo-delivery | Cinder co-anchors Yappy News with Yappy | 2 The crew, 6 News |
| yap-1106-live-reactions-during-take | Yappy talks back while you talk (lore line kept: over-eager scribe) | 2 The crew |
| yap-wf-19-mouse-button-push-to | Hold fn, a key you pick, or a mouse button | 3 Dictation |
| yap-1106-paste-target-defect | Text lands in the app you dictated into | 3 Dictation |
| yap-1106-fb-04-paste-waits-for-pause | Paste waits for a pause | 3 Dictation |
| yap-1107-clipboard-write-never-fails-silently, fix-b6-1-done-line-no-false-paste | Copy card when paste is blocked | 3 Dictation |
| yap-wf-06-backtrack-caught-toast | "scratch that" / "wait, no" caught | 4 Cleanup |
| yap-wf-05-polished-your-dictation-reveal, yap-1107-polish-diff-readable, yap-1107-reveal-stays-until-read | Polished toast and Reveal edit | 4 Cleanup |
| yap-wf-20-per-app-style-chip | Per-app style | 4 Cleanup |
| yap-1107-use-original-take-managed-state | Use original | 4 Cleanup |
| yap-call-1, yap-call-2, fix-b1-1-call-offer-withdraw-on-other-start | Call offer with a consent line, offers once, never starts or stops on its own; Zoom, FaceTime, browser calls | 5 Meetings |
| yap-1106-diarize-wire, yap-audit-utterance-segments | Both sides recorded and split into speakers | 5 Meetings |
| yap-1106-who-is-this, yap-1106-fb-22b-who-was-this-cards | "Who was this?" after the call | 5 Meetings |
| yap-1106-voiceprints, yap-1106-auto-label-and-correct | Named voices remembered on this Mac and labelled next time | 5 Meetings, 10 Privacy |
| yap-1106-meetings-redesign | Meetings window | 5 Meetings |
| yap-1106-news-prefs-onboarding | Interests picked at onboarding, editable in Settings | 6 News |
| yap-1106-news-rank-cache, yap-1106-news-feeds | Ranked, de-duplicated, cached on the Mac | 6 News |
| yap-1106-news-home-briefing | Briefing on Home | 6 News |
| yap-1106-vocab-bias | Names spelled right in dictation and meetings | 7 Dictionary |
| yap-wf-15-scratchpad-window-controls, fix-b5-1-pill-note-opens-its-own-note | Scratchpad over any app; a pill note opens to itself | 8 Scratchpad |
| yap-1106-insights-redesign | Daily words, year of activity, top apps, top people, usual wait | 9 Insights |
| yap-1106-byom-cloud-fast-path, yap-1107-cloud-first-when-key-present | Local by default; your own cloud account only if you add a key; on-device when offline | 10 Privacy |
| yap-wf-14-update-ready-in-the | Updates wait until you are not talking | Download |
| yap-1106-lic-02 / lic-06 | Pricing and licensing copy, kept byte-identical to main | Pricing, 10 Privacy |

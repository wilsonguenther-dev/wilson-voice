# dist-diff (2026-11-08T04:39:37Z)

$ git diff --stat origin/main -- site/dist
```
 site/dist/img/crew-cinder.webp | Bin 0 -> 650 bytes
 site/dist/img/crew-mochi.webp  | Bin 0 -> 394 bytes
 site/dist/img/crew-sal.webp    | Bin 0 -> 330 bytes
 site/dist/img/crew-yappy.webp  | Bin 0 -> 484 bytes
 site/dist/index.html           | 818 ++++++++++-------------------------------
 site/dist/privacy.html         |   2 +-
 site/dist/style.css            | 143 ++++---
 7 files changed, 248 insertions(+), 715 deletions(-)
```

## Sections

Removed: the code-drawn diorama of Yappy's old house and its 540-line canvas script (not a capture of the current build); "Three moves" (folded into 3 Dictation); "What you actually get" six-card grid (its true claims moved into the capability sections; "Native engine, no sidecar" removed: it named the engine); "Shipping in v0.9.3 / every line below is in the build on this page"; the "New in 0.9.3" download line.

Added: hero capture (Home); 1 The pill; 2 Yappy and the crew; 3 Dictate anywhere you can type; 4 Cleanup you can see; 5 Meetings, with notes and names; 6 Yappy News; 7 Your words, spelled right; 8 Scratchpad; 9 Insights; 10 Privacy (kept as "It works on a plane", false lines fixed, a Settings privacy capture added).

Kept byte-identical: the Pricing section (lines from <!-- PRICING --> to before <!-- DOWNLOAD -->, diffed against main: identical), the licence fact in Privacy, the licensing clause of the network-calls line, footer, three Download hrefs (/downloads/Yap-0.9.3-arm64.dmg) and the v0.9.3 label.

privacy.html: one word ("a new" -> "an updated" last-updated date); it named no engine or provider already. terms.html: untouched (licensing).

style.css: diorama/ticker rules removed; every text-transform: uppercase removed and tracking >= 0.06em cut to 0.01em (unslop: no uppercase tracked labels); capability row, crew grid and capture frame rules added.

## Images in site/dist/img

| File | Bytes |
|---|---|
| img/crew-cinder.webp | 650 |
| img/crew-mochi.webp | 394 |
| img/crew-sal.webp | 330 |
| img/crew-yappy.webp | 484 |

Still to be shot by the integrator (see CAPTURES.md): home, pill-meeting, pill-listening, pill-polished, meetings, news, dictionary, scratchpad, insights, settings-privacy.

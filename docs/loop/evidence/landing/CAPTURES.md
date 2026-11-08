# Landing page captures: what is in, what the integrator still shoots

The coder seat may not build the app (no-compile rule), and every capture below except the crew sprites needs the
built `desktop/dist` bundle. So the page references these files by name and size, and the integrator produces them
from the batch build ONCE, then commits them into `site/dist/img/` on this branch.

## Already in (real pixels)

| File | Source | Size |
|---|---|---|
| img/crew-yappy.webp, crew-sal.webp, crew-mochi.webp, crew-cinder.webp | `node scripts/site-crew-sprites.mjs ~/Obsidian/Wilson-Brain/Projects/Yap/design-2026-11-06/companions.html <dir>` (the #cast card canvases painted by assets/yappy.js + assets/friends-hd.js, copied pixel for pixel), then `magick <png> -filter point -resize 200%` and `cwebp -lossless` | 192x192 (96x96 canvas at 2x) |

## Integrator shoots (all at 2x, fixtures from scripts/smoke-fixtures-daily.json, no real user data)

Every file is WebP (`cwebp -q 90`), exactly twice the `width`/`height` the `<img>` carries in site/dist/index.html.
If a crop has to differ, change that `<img>`'s width/height to half the pixel size in the same commit.

| File | `<img>` size | Pixel size | Capture |
|---|---|---|---|
| img/home.webp | 980x700 | 1960x1400 | `YAP_SMOKE_SCALE=2 YAP_SMOKE_VIEWS=home bash scripts/smoke-windowed.sh` seeded Home at 980x700 (studio room + takes) |
| img/pill-meeting.webp | 300x140 | 600x280 | `node scripts/pill-matrix.mjs --out <dir>` listening (or idle) x dock bottom x meeting recording (clock beside Yappy) |
| img/pill-listening.webp | 300x140 | 600x280 | pill-matrix listening x bottom, no meeting |
| img/pill-polished.webp | 300x140 | 600x280 | pill-matrix the WF-05 "Polished your dictation" card with Reveal edit, bottom dock |
| img/meetings.webp | 980x700 | 1960x1400 | smoke-windowed `YAP_SMOKE_VIEWS=meetings`, seeded, a meeting open with named speakers |
| img/news.webp | 980x700 | 1960x1400 | smoke-windowed seeded Home with the Yappy News briefing showing (or the News view if the Nav has one) |
| img/dictionary.webp | 980x700 | 1960x1400 | smoke-windowed `YAP_SMOKE_VIEWS=dictionary`, seeded |
| img/scratchpad.webp | 980x700 | 1960x1400 | smoke-windowed `YAP_SMOKE_VIEWS=scratchpad`, seeded (a saved note open) |
| img/insights.webp | 980x700 | 1960x1400 | smoke-windowed `YAP_SMOKE_VIEWS=insights`, seeded |
| img/settings-privacy.webp | 980x700 | 1960x1400 | smoke-windowed `YAP_SMOKE_VIEWS=settings`, the privacy / model tab: must show no engine or provider name, or crop it out |

pill-matrix photographs at the float window's 1x unless it takes a scale; if it does not, set the CDP
deviceScaleFactor to 2 the way smoke-windowed.mjs does for YAP_SMOKE_SCALE (yap-1107-parity-kit) rather than upscaling.
Total weight of site/dist/img/ must stay under 2.5 MB: `du -ck site/dist/img/* | tail -1`.

Before committing, look at every image: no transcript text from a real person, no engine/provider/model names, no
Social, friends or visit UI.

## Then, still integrator

1. Lighthouse, headless only, never /Applications/Google Chrome.app:
   `CHROME_PATH=$HOME/Library/Caches/ms-playwright/chromium_headless_shell-1248/chrome-headless-shell-mac-arm64/chrome-headless-shell`;
   serve site/dist with `python3 -m http.server 8817 -d site/dist` (stop that PID after), then
   `npx lighthouse http://127.0.0.1:8817/index.html --only-categories=accessibility --preset=desktop --output=json --output-path=docs/loop/evidence/landing/lighthouse-desktop.json --chrome-flags="--headless=new"`
   and the same without `--preset=desktop` (mobile is the default) to lighthouse-mobile.json. Both need accessibility >= 0.95.
2. 390 px and 1440 px full-page captures of the local server (headless), inlined into the sheet below.
3. `docs/loop/evidence/landing/sections-<date -u +%F>.html`: one row per site section (hero, 1-10, pricing, download),
   the site section at 2x next to its design crop (pill.html for 1/3/4, companions.html for 2, home.html for hero/6,
   meetings.html for 5, news.html for 6; the in-app capture for 7-10), every image inlined as base64, no loose PNGs.
4. Re-run the exclusion grep in excluded-words.txt and refresh dist-diff.md with the image byte sizes.
5. PR body carries: DEPLOY NOTE: the lander must NOT deploy this. yapvoice.app is a manual staged deploy by Wilson or the orchestrator per docs/DEPLOY-SITE.md.

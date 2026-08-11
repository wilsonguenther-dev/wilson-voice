# YP3 screenshots — how they were made

Three of the four licensing states cannot be reached by hand: a trial with
exactly three days left needs eleven days of waiting, a lifetime license needs
a completed $29 purchase (and the live Payment Link is deliberately
`active: false` until launch), and the post-trial prompt needs both.

So these are captures of the **real components** — `LicensePanel`,
`PurchasePrompt`, the status chip — mounted against mocked `LicenseStatus`
payloads by the dev-tooling entry `desktop/dev/license-preview.html`. That entry
is opt-in and is **not** in any shipped build:

```bash
cd desktop
YAP_DEV_TOOLING=1 npx vite build     # adds dist/dev/license-preview.html
npx vite preview --port 4399 --strictPort

CHROME="/Applications/Google Chrome.app/Contents/MacOS/Google Chrome"
for s in trial:01-settings-license-trial \
         warning:02-trial-warning-one-toast \
         licensed:03-settings-license-licensed \
         prompt:04-license-required-prompt \
         chips:05-status-chip-four-states; do
  "$CHROME" --headless --disable-gpu --hide-scrollbars \
    --virtual-time-budget=2500 --window-size=1280,900 \
    --screenshot="docs/pr-screenshots/YP3/${s##*:}.png" \
    "http://localhost:4399/dev/license-preview.html#${s%%:*}"
done
```

A plain `npm run build` omits the entry entirely — verified by the absence of
`dist/dev/` in the default build.

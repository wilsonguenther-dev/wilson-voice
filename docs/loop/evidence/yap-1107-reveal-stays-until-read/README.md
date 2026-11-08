# yap-1107-reveal-stays-until-read — 2x capture evidence (batch 4 integrator, 2026-11-08T00:41:37Z)

Captured on the integrated batch (loop3/integration-4) with `node scripts/pill-matrix.mjs --only=<scene>` (headless Chrome, @2x, forced scenes, 3 docks x 3 meeting states, 9 captures each, 0 findings in every run):
polish-reveal-toast, polish-reveal-card, polish-reveal-toast-hover, polish-reveal-card-hover. Each scene has its contact sheet and its bottom-dock capture here.

Settings row "Keep the reveal open until I close it": NOT captured. The smoke Settings walk (scripts/smoke-windowed.sh, YAP_SMOKE_VIEWS=settings) crashes on the Audio tab (TypeError reading 'filter') before it reaches Notifications. Neither item in this batch touches the Audio tab. The row is proven by src/views/__tests__/notifications.test.tsx (4 switches, keep-reveal-open off by default) and the Rust test keep_polish_reveal_open_defaults_off_and_survives_a_restart.

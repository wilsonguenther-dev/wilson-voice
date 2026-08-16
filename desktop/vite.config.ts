import { defineConfig } from "vite";
import react from "@vitejs/plugin-react";
import { resolve } from "path";
import pkg from "./package.json";

// @ts-expect-error process is a nodejs global
const host = process.env.TAURI_DEV_HOST;

/**
 * YP3 — dev tooling, opt in.
 *
 * `dev/license-preview.html` renders the licensing surfaces against mocked
 * `LicenseStatus` payloads, which is the only way to see (and screenshot) a
 * trial with 3 days left, a lifetime license and the post-trial prompt without
 * a Stripe purchase and a fortnight of waiting. It is a BUILD ENTRY, so it is
 * only added when asked for — a shipped app must never carry a page that draws
 * an entitlement out of thin air.
 *
 * `dev/meeting-consent-preview.html` does the same for YV96's one-time capture
 * notice, which is unreachable a second time by construction.
 *
 * `dev/support-bundle-preview.html` does the same for YV98's crash-report
 * sheet, whose interesting state is a Mac whose mail client AppKit refuses to
 * drive — not something you can produce on demand.
 *
 * `dev/meeting-transcript-preview.html` does the same for YV108's mixed Me/Them
 * transcript, whose interesting state needs a recorded SECOND track — i.e. a
 * live call with the system-audio tap granted.
 *
 * `dev/speaker-chips-preview.html` does the same for YV129's "who is this?"
 * row, whose interesting state needs a six-person far-field recording that has
 * already been clustered and a roster of enrolled voices.
 *
 *   YAP_DEV_TOOLING=1 npm run build   # then dist/dev/*.html
 */
// @ts-expect-error process is a nodejs global
const devTooling = process.env.YAP_DEV_TOOLING === "1";

// https://vite.dev/config/
export default defineConfig(async () => ({
  plugins: [react()],

  // YP5 — the version the UI shows comes from package.json, which moves in
  // lockstep with tauri.conf.json and Cargo.toml at every release. It was a
  // hand-typed string, and it was still claiming v0.7.0 after the tree had
  // moved on — a version a user reads off the header has to be the real one.
  define: { __APP_VERSION__: JSON.stringify(pkg.version) },

  // Multi-page: main app + compact float pill (never share one React tree)
  build: {
    rollupOptions: {
      input: {
        main: resolve(__dirname, "index.html"),
        float: resolve(__dirname, "float.html"),
        ...(devTooling
          ? {
              licensePreview: resolve(__dirname, "dev/license-preview.html"),
              // YV96 — the one-time capture notice, which is otherwise
              // unreachable a second time: it shows once and the ack lives in
              // SQLite, so reviewing it would mean deleting a row between takes.
              meetingConsentPreview: resolve(
                __dirname,
                "dev/meeting-consent-preview.html",
              ),
              // YV98 — the crash-report sheet in both of its states: a Mac
              // that can compose, and one that can only reveal.
              supportBundlePreview: resolve(
                __dirname,
                "dev/support-bundle-preview.html",
              ),
              // YV108 — the meeting transcript, mic-only and two-track. The
              // two-track shape needs a second recorded track to exist, so
              // seeing it otherwise means holding a live call with the tap
              // granted.
              meetingTranscriptPreview: resolve(
                __dirname,
                "dev/meeting-transcript-preview.html",
              ),
              // YV102 — the system-audio setup step in all six of its states.
              // Four of them need hardware or a permission you cannot un-deny
              // (macOS asks once), so this is the only way to review them at
              // all, let alone screenshot them.
              systemAudioPreview: resolve(
                __dirname,
                "dev/system-audio-preview.html",
              ),
              // YV129 — the "who is this?" chip row. Every state worth looking
              // at needs a clustered six-person far-field recording and a
              // roster of enrolled voices, so this is the only way to review
              // the batching promise (four questions, not six) on screen.
              speakerChipsPreview: resolve(
                __dirname,
                "dev/speaker-chips-preview.html",
              ),
            }
          : {}),
      },
    },
  },

  clearScreen: false,
  server: {
    port: 1420,
    strictPort: true,
    host: host || false,
    hmr: host
      ? {
          protocol: "ws",
          host,
          port: 1421,
        }
      : undefined,
    watch: {
      ignored: ["**/src-tauri/**"],
    },
  },
}));

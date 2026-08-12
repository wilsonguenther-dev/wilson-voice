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

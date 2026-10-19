/**
 * TC-1 — the build channel, as the frontend sees it.
 *
 * `scripts/test-build.sh` exports `YAP_CHANNEL=test`, `YAP_BUILD_SHA` and
 * `YAP_BUILD_BRANCH` before `tauri build`; `vite.config.ts` folds them into the
 * `__YAP_BUILD__` constant at build time. The Rust side reads the same three
 * variables (`src-tauri/src/channel.rs`) and logs the same chip text, so the
 * header, the float window and `yap.log` always name the same head.
 */
export interface BuildInfo {
  channel: string;
  sha: string;
  branch: string;
}

/** The chip text for a test build — `null` on production (no chip at all). */
export function testChipText(info: BuildInfo): string | null {
  if (info.channel.trim() !== "test") return null;
  const sha = info.sha.trim().slice(0, 7) || "local";
  const branch = info.branch.trim();
  return branch ? `TEST BUILD ${sha} · ${branch}` : `TEST BUILD ${sha}`;
}

/** This build's chip text (compile-time constant). */
export const TEST_CHIP: string | null = testChipText(__YAP_BUILD__);

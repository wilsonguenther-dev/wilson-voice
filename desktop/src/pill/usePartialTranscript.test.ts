/**
 * yap-1106-partial-transcript-stream — the hook is an adapter (it listens), so
 * it is pinned as SOURCE here; every rule it applies is the pure module's and
 * is driven in partialTranscript.test.ts.
 */
import { describe, expect, it } from "vitest";
// @ts-expect-error -- node builtin, untyped here (no @types/node); the test runs under Node.
import { readFileSync } from "node:fs";

const src = readFileSync(new URL("./usePartialTranscript.ts", import.meta.url), "utf8") as string;

describe("usePartialTranscript — the adapter", () => {
  it("listens for the partial and for every way a take starts or ends", () => {
    expect(src).toContain("listen<unknown>(TRANSCRIPT_PARTIAL_EVENT");
    for (const ev of ['"recording"', '"transcript"', '"transcript_error"', '"take_cancelled"']) {
      expect(src).toContain(`listen<${ev === '"recording"' ? "boolean" : "unknown"}>(${ev}`);
    }
  });

  it("folds events only through the pure reducer and redraws through the throttle", () => {
    expect(src).toContain("reducePartial(before, e)");
    expect(src).toContain("nextRenderDelay(lastRender.current, performance.now(), PARTIAL_RENDER_MIN_MS)");
    expect(src).not.toMatch(/setInterval/);
  });

  it("is display state only: no command, no storage — a partial can never reach history", () => {
    expect(src).not.toMatch(/\binvoke\s*[<(]/);
    expect(src).not.toMatch(/@tauri-apps\/api\/core/);
    expect(src).not.toMatch(/localStorage|sessionStorage|indexedDB/);
  });

  it("draws nothing while the governor holds heavy work back, but still counts it", () => {
    expect(src).toContain("if (!governorAllowsPartials(governorRef.current))");
    expect(src).toContain("s.hiddenByGovernor += 1;");
  });

  it("logs one take_end line per streamed take, never the words", () => {
    expect(src).toContain("console.info(partialTakeLogLine(s))");
    expect(src).not.toMatch(/console\.\w+\([^)]*\.text/);
  });

  it("tears its listeners and its pending redraw down on unmount", () => {
    expect(src).toContain("unsubs.forEach((u) => u())");
    expect(src).toContain("clearTimeout(timer.current)");
  });
});

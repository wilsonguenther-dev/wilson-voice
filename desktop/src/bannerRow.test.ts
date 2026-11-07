/**
 * yap-1107-settings-banner-no-overlay — the structural half of "banners push
 * content down, never cover it", checked from source so it runs in vitest
 * without a browser. The geometry half (rects, elementFromPoint, the exact
 * push delta at 1190x773 2x) is scripts/settings-banner-shots.mjs.
 */
import { readFileSync } from "node:fs";
import { describe, expect, it } from "vitest";

const SRC = new URL("./", import.meta.url);
const read = (rel: string): string => readFileSync(new URL(rel, SRC), "utf8") as string;

/** Every declaration block whose selector list names `selector` exactly. */
function blocksFor(css: string, selector: string): string[] {
  const out: string[] = [];
  const noComments = css.replace(/\/\*[\s\S]*?\*\//g, "");
  const re = /([^{}]+)\{([^{}]*)\}/g;
  let m: RegExpExecArray | null;
  while ((m = re.exec(noComments))) {
    const sels = m[1].split(",").map((s) => s.trim());
    if (sels.includes(selector)) out.push(m[2]);
  }
  return out;
}

describe("app-level banner row", () => {
  const css = read("App.css");
  const app = read("App.tsx");

  it("renders every app-level banner inside one [data-main-banners] row, before .content", () => {
    const open = app.indexOf('<div className="main-banners" data-main-banners>');
    const content = app.indexOf('<div className="content">');
    expect(open).toBeGreaterThan(-1);
    expect(content).toBeGreaterThan(open);
    const row = app.slice(open, content);
    for (const piece of [
      "<SecureInputBanner",
      "status.hotkeyDegraded",
      "<HideHint",
      "<PermissionHealthRow",
      "<PermissionRepair",
      "governorBanner",
      'className="banner update"',
    ]) {
      expect(row, piece).toContain(piece);
    }
    // Nothing banner-like is left between the header and the row.
    const head = app.slice(app.indexOf("</header>"), open);
    expect(head).not.toMatch(/className="banner/);
  });

  it("gives the row its own non-shrinking slot in the .main column", () => {
    const decls = blocksFor(css, ".main-banners").join(";");
    expect(decls).toMatch(/flex:\s*none/);
    expect(blocksFor(css, ".main-banners > *").join(";")).toMatch(/flex:\s*none/);
    expect(blocksFor(css, ".content").join(";")).toMatch(/min-height:\s*0/);
  });

  it("never positions a banner out of flow", () => {
    for (const sel of [".main-banners", ".main-banners > *", ".banner", ".banner.blocked", ".banner.warn", ".banner.update", ".secure-input-quiet"]) {
      const decls = blocksFor(css, sel).join(";");
      expect(decls, sel).not.toMatch(/position:\s*(absolute|fixed|sticky)/);
    }
  });
});

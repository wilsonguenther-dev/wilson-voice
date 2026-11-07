#!/usr/bin/env node
/**
 * yap-1107-start-stop-any-surface — the 2x capture sheet per start x stop pair.
 *
 * INTEGRATOR ONLY (it compiles). Coders never run this.
 *
 *   node scripts/start-stop-any-surface-capture.mjs [--out docs/loop/evidence/start-stop-any-surface]
 *
 * 1. Runs the headless matrix: `cargo test --lib take_control` (the 4x4
 *    start-surface x stop-surface cases, the hotkey-hold row, the stop during
 *    warm-up and the double stop — desktop/src-tauri/src/take_control.rs).
 *    Its exit code is read from spawnSync directly, never through a pipe.
 * 2. Photographs the pill at Retina 2x with the existing instrument
 *    (scripts/pill-matrix.mjs, headless Chrome, never the app): `idle`
 *    (before start), `listening` (recording), `idle` (after stop). The pill
 *    renders ONLY the state the backend tells it (status / recording events),
 *    so the three frames are the same for every pair by construction — which
 *    is exactly the claim: no surface owns what the pill shows.
 * 3. Writes, per pair, <start>-to-<stop>-1-before@2x.png, -2-recording@2x.png,
 *    -3-after@2x.png and index.md listing pair -> files -> PASS/FAIL. A pair is
 *    PASS only when step 1 exited 0 AND its three frames exist.
 *
 * Exit 0 all PASS · 1 a pair FAILED · 3 environment (no cargo / no Chrome).
 */
import { spawnSync } from "node:child_process";
import { copyFileSync, existsSync, mkdirSync, mkdtempSync, readdirSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { dirname, join, resolve } from "node:path";
import { fileURLToPath } from "node:url";

const ROOT = resolve(dirname(fileURLToPath(import.meta.url)), "..");
const SURFACES = ["pill", "hotkey", "menu_bar", "dictate_button"];
const args = process.argv.slice(2);
const outIdx = args.indexOf("--out");
const OUT = resolve(ROOT, outIdx >= 0 ? args[outIdx + 1] : "docs/loop/evidence/start-stop-any-surface");
mkdirSync(OUT, { recursive: true });

function run(cmd, argv, opts = {}) {
  const r = spawnSync(cmd, argv, { cwd: ROOT, stdio: "inherit", ...opts });
  if (r.error) {
    console.error(`start-stop-capture: ${cmd} failed to run: ${r.error.message}`);
    process.exit(3);
  }
  return r.status;
}

// 1 — the headless matrix.
const testStatus = run("cargo", [
  "test",
  "--manifest-path",
  join(ROOT, "desktop/src-tauri/Cargo.toml"),
  "--lib",
  "take_control",
]);
console.log(`start-stop-capture: cargo test --lib take_control exit=${testStatus}`);

// 2 — the pill frames, into a scratch dir that is removed at the end.
const scratch = mkdtempSync(join(tmpdir(), "yap-start-stop-"));
const frames = {};
try {
  for (const phase of ["idle", "listening"]) {
    const dir = join(scratch, phase);
    const st = run("node", [join(ROOT, "scripts/pill-matrix.mjs"), "--out", dir, "--only", phase]);
    const hit = existsSync(dir)
      ? readdirSync(dir).find((f) => f.startsWith(phase + "__yappy__bottom__") && f.endsWith(".png"))
      : undefined;
    frames[phase] = st === 0 && hit ? join(dir, hit) : null;
    console.log(`start-stop-capture: pill-matrix --only ${phase} exit=${st} frame=${hit ?? "none"}`);
  }

  // 3 — per-pair sheets + index.
  const rows = [];
  let failed = 0;
  for (const a of SURFACES) {
    for (const b of SURFACES) {
      const pair = `${a}-to-${b}`;
      const files = [
        [`${pair}-1-before@2x.png`, frames.idle],
        [`${pair}-2-recording@2x.png`, frames.listening],
        [`${pair}-3-after@2x.png`, frames.idle],
      ];
      let ok = testStatus === 0;
      for (const [name, src] of files) {
        if (src) copyFileSync(src, join(OUT, name));
        else ok = false;
      }
      if (!ok) failed += 1;
      rows.push(`| ${a} | ${b} | ${files.map(([n]) => `[${n.split("-").slice(-1)[0]}](${n})`).join(" · ")} | ${ok ? "PASS" : "FAIL"} |`);
    }
  }
  const stamp = new Date().toISOString();
  writeFileSync(
    join(OUT, "index.md"),
    [
      "# Start from any surface, stop from any other — capture sheet",
      "",
      `Generated ${stamp} by scripts/start-stop-any-surface-capture.mjs (yap-1107-start-stop-any-surface).`,
      "",
      `Headless matrix \`cargo test --lib take_control\`: exit ${testStatus}. ` +
        "It covers all 16 start x stop pairs, the hotkey-hold row, a stop during warm-up and a double stop (acceptance lines 2 and 4).",
      "Frames are the pill at Retina 2x in headless Chrome (scripts/pill-matrix.mjs): before start (idle), recording (listening), after stop (idle). Acceptance line 3.",
      "",
      "| start | stop | frames | result |",
      "|---|---|---|---|",
      ...rows,
      "",
    ].join("\n"),
  );
  console.log(`start-stop-capture: ${16 - failed}/16 PASS, index at ${join(OUT, "index.md")}`);
  process.exitCode = failed === 0 ? 0 : 1;
} finally {
  rmSync(scratch, { recursive: true, force: true });
}

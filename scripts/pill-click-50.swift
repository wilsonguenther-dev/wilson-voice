#!/usr/bin/env swift
// yap-1107-pill-click-always-registers — acceptance 4: the 50-click harness.
// INTEGRATOR ONLY, on the built app (coders never run it).
//
//   swift scripts/pill-click-50.swift --rect X,Y,W,H --front com.apple.TextEdit \
//     --log "<path to Yap's log file>" [--seed 1107] [--clicks 50] \
//     [--out docs/loop/evidence/pill-click/clicks.csv]
//
// --rect is the DRAWN capsule in top-left-origin global screen points (the
// CGEvent space), read from the app's `pill hitbox screen_rect=` log line. Each
// click lands at a seeded-random point inside the capsule's stadium (inset
// 1 pt). Before EVERY click the frontmost app must be --front (a scratch text
// app: a stop pastes there) or Yap itself; anything else aborts the run, so a
// stray click never lands in Wilson's work. After each click the harness waits
// for the backend's `take intent surface=pill` line and records the timings.
import AppKit
import CoreGraphics
import Foundation

func arg(_ name: String) -> String? {
    let a = CommandLine.arguments
    guard let i = a.firstIndex(of: name), i + 1 < a.count else { return nil }
    return a[i + 1]
}
func die(_ msg: String) -> Never { FileHandle.standardError.write((msg + "\n").data(using: .utf8)!); exit(2) }

guard let rectArg = arg("--rect") else { die("--rect X,Y,W,H required") }
let r = rectArg.split(separator: ",").compactMap { Double($0) }
guard r.count == 4, r[2] > 2, r[3] > 2 else { die("bad --rect") }
guard let front = arg("--front") else { die("--front <bundle id> required") }
guard let logPath = arg("--log") else { die("--log <file> required") }
let clicks = Int(arg("--clicks") ?? "50") ?? 50
var seed = UInt64(arg("--seed") ?? "1107") ?? 1107
let out = arg("--out") ?? "docs/loop/evidence/pill-click/clicks.csv"

// SplitMix64: same seed, same 50 points, every run.
func next() -> Double {
    seed &+= 0x9E37_79B9_7F4A_7C15
    var z = seed
    z = (z ^ (z >> 30)) &* 0xBF58_476D_1CE4_E5B9
    z = (z ^ (z >> 27)) &* 0x94D0_49BB_1331_11EB
    return Double((z ^ (z >> 31)) >> 11) / Double(1 << 53)
}
func pointInPill() -> CGPoint {
    let (x, y, w, h) = (r[0] + 1, r[1] + 1, r[2] - 2, r[3] - 2)
    let rad = h / 2
    while true {
        let px = x + next() * w, py = y + next() * h
        let cx = min(max(px, x + rad), x + w - rad)
        if hypot(px - cx, py - (y + rad)) <= rad { return CGPoint(x: px, y: py) }
    }
}
func frontOK() -> Bool {
    let id = NSWorkspace.shared.frontmostApplication?.bundleIdentifier ?? ""
    return id == front || id.hasPrefix("com.wilsonguenther.")
}
func logSize() -> UInt64 { (try? FileManager.default.attributesOfItem(atPath: logPath)[.size] as? UInt64) ?? 0 }
/// Wait for the next `take intent surface=pill` Start/Stop line after `from`.
func waitIntent(from: UInt64, timeout: Double) -> (String, Double)? {
    let t0 = Date()
    while Date().timeIntervalSince(t0) < timeout {
        if let h = FileHandle(forReadingAtPath: logPath) {
            h.seek(toFileOffset: from)
            let text = String(decoding: h.readDataToEndOfFile(), as: UTF8.self)
            h.closeFile()
            for line in text.split(separator: "\n") where line.contains("take intent surface=pill") {
                if line.contains("action=StopWhenArmed") || line.contains("action=Stop") { return ("stop", Date().timeIntervalSince(t0) * 1000) }
                if line.contains("action=Start") { return ("start", Date().timeIntervalSince(t0) * 1000) }
            }
        }
        usleep(5_000)
    }
    return nil
}
func click(_ p: CGPoint) {
    let src = CGEventSource(stateID: .hidSystemState)
    CGEvent(mouseEventSource: src, mouseType: .mouseMoved, mouseCursorPosition: p, mouseButton: .left)?.post(tap: .cghidEventTap)
    usleep(120_000) // let the hover watch see the cursor arrive (real users move first)
    CGEvent(mouseEventSource: src, mouseType: .leftMouseDown, mouseCursorPosition: p, mouseButton: .left)?.post(tap: .cghidEventTap)
    usleep(60_000) // a normal click: well under the 300 ms click limit
    CGEvent(mouseEventSource: src, mouseType: .leftMouseUp, mouseCursorPosition: p, mouseButton: .left)?.post(tap: .cghidEventTap)
}

var rows = ["i,x,y,click_ms,state_before,state_after,latency_ms,ok"]
var state = "idle"
var passed = 0
let epoch = Date()
for i in 1...clicks {
    guard frontOK() else { die("abort before click \(i): frontmost app is not \(front) or Yap") }
    let p = pointInPill()
    let from = logSize()
    let at = Date().timeIntervalSince(epoch) * 1000
    click(p)
    let got = waitIntent(from: from, timeout: 12)
    let after = got.map { $0.0 == "start" ? "recording" : "idle" } ?? "no_intent"
    let ok = (state == "idle" && after == "recording") || (state == "recording" && after == "idle")
    if ok { passed += 1 }
    rows.append(String(format: "%d,%.1f,%.1f,%.0f,%@,%@,%@,%@", i, p.x, p.y, at, state, after,
                       got.map { String(format: "%.1f", $0.1) } ?? "", ok ? "true" : "false"))
    if after != "no_intent" { state = after }
    usleep(UInt32(700_000 + next() * 800_000))
}
try! rows.joined(separator: "\n").appending("\n").write(toFile: out, atomically: true, encoding: .utf8)
print("pill-click-50 seed=\(arg("--seed") ?? "1107") passed=\(passed)/\(clicks) csv=\(out)")
exit(passed == clicks ? 0 : 1)

// yap-1107-hotkey-exclusive-to-yap — acceptance (2), (3), (4): does a chord Yap
// handles reach the front app, and are any modifiers stuck after 50 takes?
// INTEGRATOR ONLY, against a TEST BUILD (scripts/test-build.sh → "Yap Test",
// bundle com.wilsonguenther.wilson-voice.test). Never Wilson's Yap, never his
// Terminal: every synthetic event goes to a front app this script chose — a
// TextEdit document it created through the AppleScript object model, or a Yap
// Test window — and the TextEdit document is closed without saving at the end.
//
//   swift desktop/scripts/hotkey-exclusive-matrix.swift --confirm-test-build \
//     --log "<Yap Test data dir>/logs/yap.log" \
//     --out docs/loop/evidence/hotkey-exclusive-matrix.md [--takes 50] [--hold-ms 700] [--gap-ms 900]
//
// Run the test build with insertion set to copy-only (no paste), so a take's
// transcript never lands in the TextEdit document: then ANY change to that
// document is a leaked key. Rows refuse to post unless the frontmost app is
// TextEdit or Yap Test.
//
// Leak detection: a LISTEN-ONLY test tap at kCGAnnotatedSessionEventTap sees
// every key event that got PAST Yap's session-level swallow tap, so a Globe
// (keycode 179) key-down / key-up counted there during a tap-chord row is a
// leak. For the Carbon row (⌃⌥⌘Space) RegisterEventHotKey consumes the key in
// the window server; the authoritative signal there is the front app (TextEdit
// text unchanged) plus one `hotkey swallowed ... by=carbon` line per phase.
// The test tap's count is printed beside it, labelled as such.
// Exit 0 = every leak count zero and no stuck modifier; 1 = a leak or a stuck
// modifier; 2 = refused (no test build, bad args, tap not creatable).
import AppKit
import CoreGraphics
import Foundation

let testBundle = "com.wilsonguenther.wilson-voice.test"

func arg(_ name: String) -> String? {
    let a = CommandLine.arguments
    guard let i = a.firstIndex(of: name), i + 1 < a.count else { return nil }
    return a[i + 1]
}
func die(_ msg: String) -> Never {
    FileHandle.standardError.write((msg + "\n").data(using: .utf8)!)
    exit(2)
}

guard CommandLine.arguments.contains("--confirm-test-build") else {
    die("refused: pass --confirm-test-build (synthetic chords go to a test build only)")
}
guard let outPath = arg("--out"), let logPath = arg("--log") else {
    die("usage: --confirm-test-build --log <yap.log> --out <md> [--takes N] [--hold-ms N] [--gap-ms N]")
}
let takes = Int(arg("--takes") ?? "50") ?? 50
let holdUs = UInt32((Int(arg("--hold-ms") ?? "700") ?? 700) * 1000)
let gapUs = UInt32((Int(arg("--gap-ms") ?? "900") ?? 900) * 1000)

guard let yap = NSRunningApplication.runningApplications(withBundleIdentifier: testBundle).first else {
    die("refused: \(testBundle) (Yap Test) is not running")
}

// ── the test tap ─────────────────────────────────────────────────────────────
enum S {
    static let lock = NSLock()
    static var row = ""
    static var counts: [String: Int] = [:]
    static let watched: Set<Int64> = [179, 49]  // Globe, Space (the Carbon chord key)
}
let cb: CGEventTapCallBack = { _, type, event, _ in
    if type == .keyDown || type == .keyUp {
        let code = event.getIntegerValueField(.keyboardEventKeycode)
        if S.watched.contains(code) {
            S.lock.lock()
            S.counts["\(S.row)|\(type == .keyDown ? "down" : "up")", default: 0] += 1
            S.lock.unlock()
        }
    }
    return Unmanaged.passUnretained(event)
}
let mask = CGEventMask((1 << CGEventType.keyDown.rawValue) | (1 << CGEventType.keyUp.rawValue))
guard let tap = CGEvent.tapCreate(
    tap: .cgAnnotatedSessionEventTap, place: .tailAppendEventTap, options: .listenOnly,
    eventsOfInterest: mask, callback: cb, userInfo: nil)
else { die("refused: the listen-only test tap was not created (Input Monitoring for this shell)") }
let tapThread = Thread {
    let src = CFMachPortCreateRunLoopSource(nil, tap, 0)
    CFRunLoopAddSource(CFRunLoopGetCurrent(), src, .commonModes)
    CGEvent.tapEnable(tap: tap, enable: true)
    CFRunLoopRun()
}
tapThread.start()
usleep(300_000)

// ── posting ──────────────────────────────────────────────────────────────────
let source = CGEventSource(stateID: .hidSystemState)
func key(_ code: CGKeyCode, down: Bool, flags: CGEventFlags, flagsChanged: Bool = false) {
    guard let e = CGEvent(keyboardEventSource: source, virtualKey: code, keyDown: down) else { return }
    if flagsChanged { e.type = .flagsChanged }
    e.flags = flags
    e.post(tap: .cghidEventTap)
    usleep(15_000)
}
let ctl = CGEventFlags.maskControl, fnF = CGEventFlags.maskSecondaryFn
let cmd = CGEventFlags.maskCommand, opt = CGEventFlags.maskAlternate

/// One fn⌃ take (the default binding): ⌃ down, fn down + Globe 179 down, hold,
/// Globe up + fn up, ⌃ up. Modifiers always released, in reverse order.
func fnControlTake() {
    key(59, down: true, flags: ctl, flagsChanged: true)
    key(63, down: true, flags: [ctl, fnF], flagsChanged: true)
    key(179, down: true, flags: [ctl, fnF])
    usleep(holdUs)
    key(179, down: false, flags: [ctl, fnF])
    key(63, down: false, flags: ctl, flagsChanged: true)
    key(59, down: false, flags: [], flagsChanged: true)
    usleep(gapUs)
}
/// One ⌃⌥⌘Space press (Carbon dictate toggle).
func carbonPress() {
    key(59, down: true, flags: ctl, flagsChanged: true)
    key(58, down: true, flags: [ctl, opt], flagsChanged: true)
    key(55, down: true, flags: [ctl, opt, cmd], flagsChanged: true)
    key(49, down: true, flags: [ctl, opt, cmd])
    key(49, down: false, flags: [ctl, opt, cmd])
    key(55, down: false, flags: [ctl, opt], flagsChanged: true)
    key(58, down: false, flags: ctl, flagsChanged: true)
    key(59, down: false, flags: [], flagsChanged: true)
    usleep(gapUs)
}

// ── front apps ───────────────────────────────────────────────────────────────
func osa(_ src: String) -> String? {
    var err: NSDictionary?
    let r = NSAppleScript(source: src)?.executeAndReturnError(&err)
    return err == nil ? (r?.stringValue ?? "") : nil
}
let sentinel = "yap-matrix-sentinel"
func frontId() -> String { NSWorkspace.shared.frontmostApplication?.bundleIdentifier ?? "unknown" }
func textEditText() -> String? { osa("tell application \"TextEdit\" to get text of document 1") }

_ = osa("tell application \"TextEdit\" to make new document with properties {text:\"\(sentinel)\"}")
let fronts: [(String, () -> Void)] = [
    ("TextEdit", { _ = osa("tell application \"TextEdit\" to activate") }),
    ("Settings", {
        yap.activate(options: [])
        usleep(400_000)
        guard frontId() == testBundle else { return }  // ⌘, never reaches another app
        key(43, down: true, flags: cmd); key(43, down: false, flags: cmd)  // ⌘, (test build only)
    }),
    ("Scratchpad", {
        guard frontId() == testBundle || frontId() == "com.apple.TextEdit" else { return }
        key(59, down: true, flags: ctl, flagsChanged: true)
        key(55, down: true, flags: [ctl, cmd], flagsChanged: true)
        key(38, down: true, flags: [ctl, cmd]); key(38, down: false, flags: [ctl, cmd])  // ⌃⌘J
        key(55, down: false, flags: ctl, flagsChanged: true)
        key(59, down: false, flags: [], flagsChanged: true)
    }),
    ("pill", {
        yap.activate(options: [])
        usleep(400_000)
        guard frontId() == testBundle else { return }  // ⌘W never reaches another app
        key(13, down: true, flags: cmd); key(13, down: false, flags: cmd)  // ⌘W: only the pill stays
    }),
]

let logStart = (try? FileManager.default.attributesOfItem(atPath: logPath)[.size] as? Int) ?? 0
var rows: [(String, String, String, String, Int, Int, String)] = []
let perFront = max(1, takes / fronts.count)
var takesRun = 0
for (name, bringFront) in fronts {
    bringFront()
    usleep(800_000)
    let front = frontId()
    // Synthetic keys go ONLY to TextEdit (our own document) or Yap Test.
    guard front == "com.apple.TextEdit" || front == testBundle else {
        _ = osa("tell application \"TextEdit\" to close document 1 saving no")
        CFMachPortInvalidate(tap)
        die("refused: front app for row \(name) is \(front), not TextEdit or Yap Test — nothing posted")
    }
    let before = name == "TextEdit" ? textEditText() : nil
    S.lock.lock(); S.row = "\(name)|fn⌃"; S.lock.unlock()
    for _ in 0..<perFront { fnControlTake(); takesRun += 1 }
    S.lock.lock(); S.row = "\(name)|⌃⌥⌘Space"; S.lock.unlock()
    carbonPress(); carbonPress()  // start, then stop
    S.lock.lock(); S.row = ""; S.lock.unlock()
    let textOk = name == "TextEdit" ? (textEditText() == before ? "unchanged" : "CHANGED") : "n/a (no key log in Yap windows; test tap is the signal)"
    for chord in ["fn⌃", "⌃⌥⌘Space"] {
        S.lock.lock()
        let d = S.counts["\(name)|\(chord)|down", default: 0]
        let u = S.counts["\(name)|\(chord)|up", default: 0]
        S.lock.unlock()
        rows.append((chord, name, front, "down+up", d, u, textOk))
    }
}
while takesRun < takes { fnControlTake(); takesRun += 1 }
usleep(1_000_000)

// ── stuck-modifier check ─────────────────────────────────────────────────────
let stuckMask: CGEventFlags = [ctl, fnF, cmd, opt]
let hid = CGEventSource.flagsState(.hidSystemState).intersection(stuckMask)
let combined = CGEventSource.flagsState(.combinedSessionState).intersection(stuckMask)
func names(_ f: CGEventFlags) -> String {
    var n: [String] = []
    if f.contains(ctl) { n.append("Control") }
    if f.contains(fnF) { n.append("Fn") }
    if f.contains(cmd) { n.append("Command") }
    if f.contains(opt) { n.append("Option") }
    return n.isEmpty ? "none" : n.joined(separator: ", ")
}

// ── diagnostics excerpt ──────────────────────────────────────────────────────
var excerpt: [String] = []
if let h = FileHandle(forReadingAtPath: logPath) {
    h.seek(toFileOffset: UInt64(logStart))
    let text = String(decoding: h.readDataToEndOfFile(), as: UTF8.self)
    excerpt = text.split(separator: "\n").map(String.init).filter { $0.contains("hotkey swallowed") }
    h.closeFile()
}
let tapLines = excerpt.filter { $0.contains("by=tap") }.count
let carbonLines = excerpt.filter { $0.contains("by=carbon") }.count

_ = osa("tell application \"TextEdit\" to close document 1 saving no")
CGEvent.tapEnable(tap: tap, enable: false)
CFMachPortInvalidate(tap)

let stamp = ISO8601DateFormatter().string(from: Date())
var md = "## Matrix run \(stamp) (\(takesRun) synthetic fn⌃ takes, Yap Test pid \(yap.processIdentifier))\n\n"
md += "| chord | front app | frontmost bundle id | phase | leaked down | leaked up | front-app check |\n|---|---|---|---|---|---|---|\n"
var leaks = 0
for r in rows {
    md += "| \(r.0) | \(r.1) | \(r.2) | \(r.3) | \(r.4) | \(r.5) | \(r.6) |\n"
    if r.0 == "fn⌃" { leaks += r.4 + r.5 }
    if r.6 == "CHANGED" { leaks += 1 }
}
md += "\nStuck-modifier check after \(takesRun) takes — CGEventSourceFlagsState(HID): \(names(hid)); (combined session): \(names(combined)).\n"
md += "\nDiagnostics: \(tapLines) `by=tap` and \(carbonLines) `by=carbon` `hotkey swallowed` lines (expected \(takesRun * 2) and \(fronts.count * 4)). Excerpt:\n\n```\n"
md += excerpt.prefix(12).joined(separator: "\n") + "\n```\n"
try? (((try? String(contentsOfFile: outPath, encoding: .utf8)) ?? "") + "\n" + md).write(toFile: outPath, atomically: true, encoding: .utf8)
print(md)
exit(leaks == 0 && hid.isEmpty && combined.isEmpty ? 0 : 1)

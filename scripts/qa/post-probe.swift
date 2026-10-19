// scripts/qa/post-probe.swift — yap-p0-insertion-bridge-fallback ROOT-CAUSE-PROBE.
//
// Question it answers: on THIS macOS, is a CGEventPost(⌘V) delivered when the
// posting process lacks Accessibility? fn-post.swift:32 already refuses to post
// without CGPreflightPostEventAccess(), so "keyDown needs no AX" is not assumed.
//
// Build (same as fn-post.swift in hotkey-walk.sh — ad hoc, never Yap's identity):
//   swiftc -O -o desktop/target/qa/post-probe scripts/qa/post-probe.swift
//   codesign -s - -f desktop/target/qa/post-probe
//
// Usage:
//   post-probe            print post_event_access=<CGPreflightPostEventAccess()>
//                         and ax_trusted=<AXIsProcessTrusted()>; never posts.
//   post-probe --post     ALSO create a fresh .txt, open it in TextEdit, and ONLY
//                         when TextEdit is verifiably frontmost put a marker on the
//                         pasteboard, post ⌘V then ⌘S, and read the file back:
//                         delivered=<marker reached the document>. Aborts (exit 4)
//                         without posting anything if TextEdit never became
//                         frontmost, so a ⌘V can never land in another app.
// macOS attributes the grant to the responsible process (the launching terminal),
// so run it from the terminal whose grant you mean to measure.
// Exit: 0 probe ran, 4 TextEdit not frontmost (nothing posted), 64 bad usage.
import AppKit
import ApplicationServices
import CoreGraphics
import Foundation

let args = Array(CommandLine.arguments.dropFirst())
if !(args.isEmpty || args == ["--post"]) {
    FileHandle.standardError.write("usage: post-probe [--post]\n".data(using: .utf8)!)
    exit(64)
}

let postAccess = CGPreflightPostEventAccess()
print("post_event_access=\(postAccess)")
print("ax_trusted=\(AXIsProcessTrusted())")
if args.isEmpty { exit(0) }

let marker = "yap-post-probe-\(UInt64(Date().timeIntervalSince1970 * 1000))"
let url = FileManager.default.temporaryDirectory.appendingPathComponent("\(marker).txt")
try? "".write(to: url, atomically: true, encoding: .utf8)
let textEdit = URL(fileURLWithPath: "/System/Applications/TextEdit.app")
let cfg = NSWorkspace.OpenConfiguration()
cfg.activates = true
let opened = DispatchSemaphore(value: 0)
NSWorkspace.shared.open([url], withApplicationAt: textEdit, configuration: cfg) { _, _ in opened.signal() }
_ = opened.wait(timeout: .now() + 5)

func textEditFrontmost() -> Bool {
    NSWorkspace.shared.frontmostApplication?.bundleIdentifier == "com.apple.TextEdit"
}
var waited = 0
while !textEditFrontmost() && waited < 30 { usleep(100_000); waited += 1 }
if !textEditFrontmost() {
    print("frontmost=\(NSWorkspace.shared.frontmostApplication?.bundleIdentifier ?? "none") — refusing to post")
    exit(4)
}
usleep(400_000)

NSPasteboard.general.clearContents()
NSPasteboard.general.setString(marker, forType: .string)

let source = CGEventSource(stateID: .privateState)
func chord(_ key: CGKeyCode) {
    for down in [true, false] {
        let ev = CGEvent(keyboardEventSource: source, virtualKey: key, keyDown: down)
        ev?.flags = .maskCommand
        ev?.post(tap: .cghidEventTap)
    }
}
chord(0x09) // V
usleep(300_000)
chord(0x01) // S
usleep(1_000_000)
let body = (try? String(contentsOf: url, encoding: .utf8)) ?? ""
print("delivered=\(body.contains(marker))")
if body.contains(marker) { chord(0x0D) } // ⌘W only when our posts provably land
exit(0)

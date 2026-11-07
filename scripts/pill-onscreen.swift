// FB-03 — is the Yap pill on screen right now? One CGWindowList read, from
// OUTSIDE the app, so it proves what a person would see rather than what the
// app believes. Public API only: owner PID, layer, bounds and alpha need no
// Screen Recording grant (window titles do, and are never read).
//
//   swift scripts/pill-onscreen.swift <yap-test-pid> [min-layer]
//
// Prints one JSON line and exits 0 when the pill panel (a window of that PID at
// min-layer or above, default 101 = the FB-03 base level) is in the on-screen
// list AND no other app's window in front of it covers its whole frame; 1 when
// it is not; 2 on bad arguments. Driven by scripts/pill-over-apps.sh.
import AppKit
import CoreGraphics
import Foundation

let args = CommandLine.arguments
guard args.count >= 2, let pid = Int(args[1]), pid > 0 else {
    FileHandle.standardError.write("usage: pill-onscreen.swift <yap-test-pid> [min-layer]\n".data(using: .utf8)!)
    exit(2)
}
let minLayer = args.count >= 3 ? (Int(args[2]) ?? 101) : 101

let options: CGWindowListOption = [.optionOnScreenOnly, .excludeDesktopElements]
let list = (CGWindowListCopyWindowInfo(options, kCGNullWindowID) as? [[String: Any]]) ?? []

func num(_ w: [String: Any], _ key: CFString) -> Int {
    (w[key as String] as? NSNumber)?.intValue ?? 0
}
func alpha(_ w: [String: Any]) -> Double {
    (w[kCGWindowAlpha as String] as? NSNumber)?.doubleValue ?? 1.0
}
func bounds(_ w: [String: Any]) -> CGRect {
    guard let d = w[kCGWindowBounds as String] as? NSDictionary,
          let r = CGRect(dictionaryRepresentation: d as CFDictionary) else { return .zero }
    return r
}

// The list is front to back: the first match is the pill panel.
var pillIndex: Int? = nil
for (i, w) in list.enumerated()
where num(w, kCGWindowOwnerPID) == pid && num(w, kCGWindowLayer) >= minLayer && alpha(w) > 0 {
    pillIndex = i
    break
}

let front = NSWorkspace.shared.frontmostApplication
let frontId = front?.bundleIdentifier ?? "unknown"
let frontPid = Int(front?.processIdentifier ?? 0)
let frontLayer = list.filter { num($0, kCGWindowOwnerPID) == frontPid && alpha($0) > 0 }
    .map { num($0, kCGWindowLayer) }
    .max() ?? -1

var covered = false
var pillLayer = -1
if let pi = pillIndex {
    let pill = list[pi]
    pillLayer = num(pill, kCGWindowLayer)
    let frame = bounds(pill)
    for w in list[..<pi] where num(w, kCGWindowOwnerPID) != pid && alpha(w) > 0 {
        if bounds(w).contains(frame) {
            covered = true
            break
        }
    }
}
let listed = pillIndex != nil
let onScreen = listed && !covered
print("{\"front\":\"\(frontId)\",\"front_layer\":\(frontLayer),\"listed\":\(listed),\"covered\":\(covered),\"pill_layer\":\(pillLayer),\"on_screen\":\(onScreen)}")
exit(onScreen ? 0 : 1)

// scripts/qa/fn-post.swift — posts synthetic fn (Globe) edges for hotkey-walk.sh.
//
// hotkey-walk.sh compiles this with swiftc into desktop/target/qa/fn-post and
// signs it AD HOC (codesign -s -). It never carries Yap's identity.
//
// Usage: fn-post check | fn-post OP [OP ...]
//   OP = down | up | ctrl-down | ctrl-up | sleep:<ms> | chord:<keycode>:<flags>
//   down      -> a flagsChanged event for kVK_Function (63) WITH maskSecondaryFn
//   up        -> the same event with maskSecondaryFn cleared
//   ctrl-down -> a flagsChanged event for kVK_Control (59) WITH maskControl
//   ctrl-up   -> the same event with maskControl cleared
//   Modifier ops ACCUMULATE: every flagsChanged event carries all the modifiers
//   held so far, so "ctrl-down down" is fn⌃ (maskControl | maskSecondaryFn) —
//   Yap's default pttBinding "fn_control" — and "up ctrl-up" releases it in
//   reverse. hotkey-walk.sh picks the ops from the configured binding.
//   chord -> keyDown then keyUp of <keycode> carrying CGEventFlags <flags>
//            (decimal), e.g. chord:49:1835008 = ⌃⌥⌘Space, Yap's DICTATE_TOGGLE
// Exit: 0 posted (or check passed), 3 the caller may not post events
// (Accessibility / PostEvent access missing — macOS attributes it to the
// terminal that launched this binary), 64 bad usage.
import CoreGraphics
import Foundation

let kVKFunction: CGKeyCode = 63
let kVKControl: CGKeyCode = 59

func usage() -> Never {
    FileHandle.standardError.write("usage: fn-post check | fn-post (down|up|ctrl-down|ctrl-up|sleep:<ms>|chord:<keycode>:<flags>)...\n".data(using: .utf8)!)
    exit(64)
}

let ops = Array(CommandLine.arguments.dropFirst())
if ops.isEmpty { usage() }
// Validate every op BEFORE posting anything, so a typo can never leave fn "down".
func chordParts(_ op: String) -> (CGKeyCode, UInt64)? {
    let p = op.split(separator: ":", omittingEmptySubsequences: false)
    guard p.count == 3, p[0] == "chord", let k = UInt16(p[1]), let f = UInt64(p[2]) else { return nil }
    return (k, f)
}
func isOp(_ op: String) -> Bool {
    op == "down" || op == "up" || op == "ctrl-down" || op == "ctrl-up" || (op.hasPrefix("sleep:") && UInt32(op.dropFirst(6)) != nil)
        || chordParts(op) != nil
}
if ops != ["check"] && !ops.allSatisfy(isOp) { usage() }

// Preflight only: never prompts, never posts when access is missing.
if !CGPreflightPostEventAccess() {
    print("post_event_access=false")
    exit(3)
}
if ops == ["check"] {
    print("post_event_access=true")
    exit(0)
}

// A private source: the posted flags ride on the event itself and never
// leak into the shared HID modifier state.
guard let source = CGEventSource(stateID: .privateState) else { exit(70) }

// The modifiers this run is holding; every flagsChanged event carries them all.
var held: CGEventFlags = []

func postModifier(_ key: CGKeyCode, _ mask: CGEventFlags, down: Bool) {
    if down { held.insert(mask) } else { held.remove(mask) }
    guard let ev = CGEvent(keyboardEventSource: source, virtualKey: key, keyDown: down) else { exit(70) }
    ev.type = .flagsChanged
    ev.flags = held
    ev.post(tap: .cghidEventTap)
}

func postChord(_ key: CGKeyCode, _ flags: UInt64) {
    for down in [true, false] {
        guard let ev = CGEvent(keyboardEventSource: source, virtualKey: key, keyDown: down) else { exit(70) }
        ev.flags = CGEventFlags(rawValue: flags)
        ev.post(tap: .cghidEventTap)
        if down { usleep(60_000) }
    }
}

for op in ops {
    if let (key, flags) = chordParts(op) {
        postChord(key, flags)
        continue
    }
    switch op {
    case "down": postModifier(kVKFunction, .maskSecondaryFn, down: true)
    case "up": postModifier(kVKFunction, .maskSecondaryFn, down: false)
    case "ctrl-down": postModifier(kVKControl, .maskControl, down: true)
    case "ctrl-up": postModifier(kVKControl, .maskControl, down: false)
    default:
        usleep(UInt32(op.dropFirst(6))! * 1000)
    }
}
exit(0)

// scripts/qa/fn-post.swift — posts synthetic fn (Globe) edges for hotkey-walk.sh.
//
// hotkey-walk.sh compiles this with swiftc into desktop/target/qa/fn-post and
// signs it AD HOC (codesign -s -). It never carries Yap's identity.
//
// Usage: fn-post check | fn-post OP [OP ...]
//   OP = down | up | sleep:<ms>
//   down  -> a flagsChanged event for kVK_Function (63) WITH maskSecondaryFn
//   up    -> the same event with no modifier flags
// Exit: 0 posted (or check passed), 3 the caller may not post events
// (Accessibility / PostEvent access missing — macOS attributes it to the
// terminal that launched this binary), 64 bad usage.
import CoreGraphics
import Foundation

let kVKFunction: CGKeyCode = 63

func usage() -> Never {
    FileHandle.standardError.write("usage: fn-post check | fn-post (down|up|sleep:<ms>)...\n".data(using: .utf8)!)
    exit(64)
}

let ops = Array(CommandLine.arguments.dropFirst())
if ops.isEmpty { usage() }
// Validate every op BEFORE posting anything, so a typo can never leave fn "down".
func isOp(_ op: String) -> Bool {
    op == "down" || op == "up" || (op.hasPrefix("sleep:") && UInt32(op.dropFirst(6)) != nil)
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

func post(down: Bool) {
    guard let ev = CGEvent(keyboardEventSource: source, virtualKey: kVKFunction, keyDown: down) else { exit(70) }
    ev.type = .flagsChanged
    ev.flags = down ? .maskSecondaryFn : []
    ev.post(tap: .cghidEventTap)
}

for op in ops {
    switch op {
    case "down": post(down: true)
    case "up": post(down: false)
    default:
        usleep(UInt32(op.dropFirst(6))! * 1000)
    }
}
exit(0)

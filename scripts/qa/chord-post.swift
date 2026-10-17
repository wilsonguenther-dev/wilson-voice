// QA ONLY — posts the Meetings shortcut (Control-Command-M) as a keyboard
// event, the documented path a user takes. Same shape as fn-post.swift: built
// ad hoc by meeting-walk.sh into a temp dir, never shipped in the app.
import CoreGraphics
import Foundation

let kVKM: CGKeyCode = 46
let ops = Array(CommandLine.arguments.dropFirst())
if ops != ["check"] && ops != ["post"] {
    FileHandle.standardError.write("usage: chord-post check | chord-post post\n".data(using: .utf8)!)
    exit(64)
}
if !CGPreflightPostEventAccess() {
    print("post_event_access=false")
    exit(3)
}
if ops == ["check"] {
    print("post_event_access=true")
    exit(0)
}
let src = CGEventSource(stateID: .hidSystemState)
let flags: CGEventFlags = [.maskControl, .maskCommand]
guard let down = CGEvent(keyboardEventSource: src, virtualKey: kVKM, keyDown: true),
      let up = CGEvent(keyboardEventSource: src, virtualKey: kVKM, keyDown: false) else {
    exit(1)
}
down.flags = flags
up.flags = flags
down.post(tap: .cghidEventTap)
usleep(40_000)
up.post(tap: .cghidEventTap)
print("posted ctrl+cmd+m")
exit(0)

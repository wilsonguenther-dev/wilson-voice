// scripts/qa/perm-helper.swift — the QA permission helper (yap-qa-harness).
//
// perm-probe.sh compiles this with swiftc into desktop/target/qa/perm-helper and
// signs it AD HOC (codesign -s -). It never carries Yap's identity.
//
// THESE ANSWERS DESCRIBE THIS HELPER, NOT YAP. macOS attributes a command-line
// binary's TCC checks to its "responsible process" — the terminal (or agent
// host) that launched it — so ax_trusted / listen_event say whether THAT
// terminal holds Accessibility / Input Monitoring. They are never Yap's grants.
import ApplicationServices
import CoreGraphics

// AXIsProcessTrusted(): Accessibility (needed to post synthetic CGEvents).
let axTrusted = AXIsProcessTrusted()
// CGPreflightListenEventAccess(): Input Monitoring. Preflight only — never prompts.
let listenEvent = CGPreflightListenEventAccess()

print("ax_trusted=\(axTrusted)")
print("listen_event=\(listenEvent)")

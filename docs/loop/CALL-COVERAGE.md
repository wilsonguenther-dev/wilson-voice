# Call coverage ledger (yap-call-0, absorbs yap-call-4)

One row per entry in `desktop/src-tauri/src/call_catalog.json`. `tests/call_coverage_ledger.rs`
fails if a catalog bundle id has no row here, if a `verified: true` catalog row has no recorded
(non-synthetic) fixture, or if a browser row was verified from a Terminal-AX run.

How a column gets filled:

- **Bundle id**: `defaults read "<app>/Contents/Info" CFBundleIdentifier` on the build Mac
  (macOS 26.6.2, 2026-10-27), or `UNVERIFIED: not installed`.
- **Input process**: the CoreAudio process object whose `kAudioProcessPropertyIsRunningInput` is 1,
  taken from a `--probe-calls` snapshot DURING a real call (`scripts/call-walk.sh`). Until then it is
  the plan's expectation and says UNVERIFIED.
- **Detected?**: Y/N from that snapshot scored by `call_signals::score`. UNVERIFIED until the walk.
- **Track 1 non-zero?**: from the meeting diagnostics row of a recording made during the call.
  FaceTime and Phone stay "your side only" until a walk shows a non-zero track 1; the far side of
  those calls is not measured, so nothing claims a two-sided recording without it.

| App | Bundle id | Verified on disk | Input process (in call) | Detected? | Offer text | Track 1 non-zero? | Notes |
|-----|-----------|------------------|-------------------------|-----------|------------|-------------------|-------|
| Zoom | `us.zoom.xos` | yes (`defaults read`) | UNVERIFIED (expected: `us.zoom.xos` itself) | UNVERIFIED | Zoom call — record it? | UNVERIFIED | fixture `zoom-in-call` is synthetic |
| Microsoft Teams | `com.microsoft.teams2` | UNVERIFIED: not installed | UNVERIFIED (expected: a `com.microsoft.teams2*` helper) | UNVERIFIED | Microsoft Teams call — record it? | UNVERIFIED | |
| FaceTime | `com.apple.FaceTime` | yes (`defaults read`) | UNVERIFIED (expected: `com.apple.avconferenced`) | UNVERIFIED | FaceTime call — record your side? | UNVERIFIED — your side only | system_daemon; audio via avconferenced |
| Phone / iPhone calls on Mac | `com.apple.mobilephone` | yes (`defaults read`) | UNVERIFIED (expected: callservicesd, `com.apple.TelephonyUtilities`) | UNVERIFIED | Phone call — record your side? | UNVERIFIED — your side only | system_daemon |
| iPhone Mirroring | `com.apple.ScreenContinuity` | yes (`defaults read`) | UNVERIFIED | UNVERIFIED | iPhone Mirroring call — record it? | UNVERIFIED | whether a mirrored call ever opens the Mac mic is unknown |
| Slack | `com.tinyspeck.slackmacgap` | UNVERIFIED: not installed | UNVERIFIED (expected: a Slack helper, ppid = Slack) | UNVERIFIED | Slack call — record it? | UNVERIFIED | huddles |
| Discord | `com.hnc.Discord` | UNVERIFIED: not installed | UNVERIFIED (expected: a Discord helper) | UNVERIFIED | Discord call — record it? | UNVERIFIED | |
| WhatsApp | `net.whatsapp.WhatsApp` | UNVERIFIED: not installed | UNVERIFIED | UNVERIFIED | WhatsApp call — record it? | UNVERIFIED | |
| Webex | `Cisco-Systems.Spark` | UNVERIFIED: not installed | UNVERIFIED | UNVERIFIED | Webex call — record it? | UNVERIFIED | |
| Chrome-family Meet (Chrome) | `com.google.Chrome` | yes (`defaults read`) | UNVERIFIED (expected: `com.google.Chrome.helper`, ppid = Chrome) | UNVERIFIED | Google Meet call — record it? (title match) / Google Chrome call — record it? (AX unknown) | UNVERIFIED | browser: verified only from a walk with ax_source "yap" |
| Chrome-family Meet (Edge) | `com.microsoft.edgemac` | UNVERIFIED: not installed | UNVERIFIED | UNVERIFIED | Google Meet call — record it? | UNVERIFIED | browser |
| Chrome-family Meet (Brave) | `com.brave.Browser` | UNVERIFIED: not installed | UNVERIFIED | UNVERIFIED | Google Meet call — record it? | UNVERIFIED | browser |
| Chrome-family Meet (Arc) | `company.thebrowser.Browser` | UNVERIFIED: not installed | UNVERIFIED | UNVERIFIED | Google Meet call — record it? | UNVERIFIED | browser |
| Safari | `com.apple.Safari` | yes (`defaults read`) | UNVERIFIED (expected: `com.apple.WebKit.GPU`, ppid 1) | UNVERIFIED | Google Meet call — record it? (title match only) | UNVERIFIED | WebKit.GPU is attributed ONLY with a Safari window-title match |

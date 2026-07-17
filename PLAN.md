# Wilson Voice — Stabilization Plan (stop thrashing)

**Date:** 2026-07-17  
**Status:** PLAN ONLY until Wilson approves Phase 1  
**Problem:** Multiple fake apps, freezes, broken paste/hotkeys, no real OS permission path.

---

## Deep audit results (facts, not guesses)

### Processes right now
| Process | Status |
|---------|--------|
| `/Applications/Wilson Voice.app/.../wilson-voice` (pid ~95761) | **Only** Voice process; arm64 Tauri; currently healthy (not hung) |
| `python -m wilson_voice` | **Not running** (old menubar is dead) |
| LaunchAgents for voice | **None** |
| Login items | Wispr Flow, Claude, LM Studio — **not** Wilson Voice |

### “Hella Wilson Voice apps” — what you actually saw
| Thing | What it is | Action |
|-------|------------|--------|
| Desktop shell `.app` | Fake bash → Python | **Already deleted** |
| `~/Applications/Wilson Voice.app` | Same fake | **Already deleted** |
| `/Applications/Wilson Voice.app` | Real Tauri v0.3 | **KEEP — only user install** |
| Build tree `.../target/.../Wilson Voice.app` | Compile output (not for double-click daily) | Ignore / gitignored |
| **`Wilson Voice_0.3.0_aarch64.dmg` + mounted disk** | Tauri `bundle targets: dmg` auto-opens a **disk named Wilson Voice** | **Stop building DMG**; eject if mounted; delete DMG |

That silver “Wilson Voice” drive in Finder **is not a second app** — it is a **mounted DMG** from `npm run tauri build`. Looks like another app. That was self-inflicted mess.

### TCC (macOS privacy) — critical gap
Queried user TCC DB for `com.wilsonguenther.wilson-voice`:

**No rows.** The real app has **never been granted**:
- Microphone  
- Accessibility  
- Input Monitoring (`ListenEvent`)  
- PostEvent (synthetic keystrokes)

What *does* have grants (old broken path):
- **Python** / Homebrew Python → Accessibility / ListenEvent / Microphone  
- **wilson-os**, KokoriKeys → Accessibility / PostEvent  

So even a perfect hotkey stack fails for paste/mic until **Wilson Voice** (bundle id) is toggled ON in System Settings — not “Python”.

### Why it froze (“not responding”)
Proven by `sample`:
1. **Mutex deadlock** in `emit_status` (nested `parking_lot` locks) — fixed in `d7f72b0`
2. Secondary: registering hotkeys / second webview during `didFinishLaunching` blocked main thread — deferred

### How macOS hotkeys actually work (research)

| Approach | API | TCC needed | Used by |
|----------|-----|------------|---------|
| **RegisterEventHotKey** (Carbon) | Combo keys with modifiers | Usually **none** for basic combos | Tauri `global-shortcut` / `global-hotkey` |
| **CGEvent tap** / `NSEvent` global monitor | All keys, hold modifiers alone | **Accessibility** and/or **Input Monitoring** | pynput, many dictation apps for Fn/Right-Option alone |
| **enigo / CGEventPost** paste Cmd+V | Synthetic keyboard | **Accessibility** | VoiceInk, Handy paste path |
| **Mic** | AVFoundation / ffmpeg avfoundation | **Microphone** for the **app binary** | All dictation apps |

**Official Tauri rule** ([global-hotkey README](https://github.com/tauri-apps/global-hotkey)):  
on macOS the hotkey manager must live on the **main thread with a running event loop**. Do **not** invent kernel hooks.

**Wispr-style “hold Right ⌥ alone”** is **not** a Carbon hotkey — it needs an event tap + Input Monitoring. That is why Python pynput asked for trust and said “not trusted”.

**Product decision:**  
- **v1 reliable:** ⌘⇧V hold (Carbon via Tauri plugin) + big UI button  
- **v1.1 polish:** Right-⌥ hold via CGEvent tap **after** AX trust prompt  

Do not promise Fn-alone without Karabiner or event-tap work.

---

## Target architecture (correct, not script soup)

```
┌─────────────────────────────────────────────────────────┐
│  ONE app: /Applications/Wilson Voice.app                │
│  bundle id: com.wilsonguenther.wilson-voice             │
│  ad-hoc signed for now (Developer ID later)             │
├─────────────────────────────────────────────────────────┤
│  UI (React/WebView)  │  Tray  │  optional float later   │
├─────────────────────────────────────────────────────────┤
│  Rust main thread                                       │
│   - tauri-plugin-global-shortcut (⌘⇧V) on main thread   │
│   - enigo paste AFTER AXIsProcessTrusted                │
│   - spawn ffmpeg for mic (TCC: Microphone on this app)  │
│   - rusqlite WAL + FTS5 history                         │
├─────────────────────────────────────────────────────────┤
│  Sidecar: ~/Desktop/wilson-voice/.venv + asr_worker.py  │
│  (MLX Whisper only — not a second menubar app)          │
└─────────────────────────────────────────────────────────┘
```

**One identity in Privacy panes.** Never launch Python as the UI process.

---

## Phase plan (execute in order, stop between phases)

### Phase 0 — Hygiene (do immediately, low risk)
1. Eject any volume named Wilson Voice  
2. Delete `target/release/bundle/dmg/*.dmg` (or stop building it)  
3. Set `bundle.targets` to **`["app"]` only** — no DMG litter  
4. Confirm **only** `/Applications/Wilson Voice.app` is user-facing  
5. Do **not** copy apps to Desktop or `~/Applications` ever again  

### Phase 1 — Permissions UX (blocks all real use)
1. On first launch, check:
   - Mic (AVCapture / permission status)  
   - `AXIsProcessTrusted()` for Accessibility  
2. Settings screen: three big buttons open correct Privacy panes  
3. UI shows **red/green** for each grant for **this** bundle id  
4. Block paste with clear message if AX denied  
5. Document: user must enable **Wilson Voice**, not Python  

### Phase 2 — Hotkeys done correctly
1. Register **only** `Super+Shift+V` via Tauri global-shortcut **on main thread after setup returns** (already deferred)  
2. Hold-to-talk = Pressed start / Released stop (already designed)  
3. UI always works without hotkeys (record button)  
4. Optional later: CGEvent tap for Right Option **behind** trust prompt  
5. Never fight Wispr Flow for the same combo if both running  

### Phase 3 — Paste / record reliability
1. Paste: enigo Cmd+V only if AX trusted; else clipboard-only + toast  
2. Record: ffmpeg avfoundation with explicit error if Mic denied  
3. No osascript System Events for keystrokes (that’s why Python paste failed with error 1002)  

### Phase 4 — Product UI polish (only after 0–3 work)
1. Home / Insights / Dictionary / Scratchpad already scaffolded on SQLite  
2. Floating pill **off by default** until Phase 1–2 proven  
3. No GraphQL — SQLite FTS5 is the local data plane  

### Phase 5 — Distribution (later)
1. Developer ID + notarization so TCC is stable and Gatekeeper is clean  
2. Ad-hoc re-sign every rebuild **resets** trust UX — user re-grants; expect that until signed  

---

## Explicit non-goals (stop doing these)
- Building more shell-wrapper `.app`s  
- Copying apps to Desktop  
- Building + auto-opening DMGs during dev  
- Using Python as the menubar host  
- Nested mutexes / heavy work inside `didFinishLaunching`  
- Claiming Fn key works without event-tap or Karabiner  

---

## Success criteria
- [ ] Force Quit never shows Wilson Voice “not responding”  
- [ ] Exactly one install path: `/Applications/Wilson Voice.app`  
- [ ] No Wilson Voice volume in `/Volumes` after build  
- [ ] TCC rows exist for `com.wilsonguenther.wilson-voice` Mic + Accessibility  
- [ ] Hold ⌘⇧V → record → transcript in Home → text in clipboard; paste if AX granted  
- [ ] UI Record button works without any hotkey  

---

## Recommended next action (one step)
**Phase 0 + Phase 1 only:** stop DMG, keep single app, ship permission checklist UI, verify Mic + Accessibility grants for the real bundle.

No new features until paste + record work end-to-end with permissions green.

# Wilson Voice — Product Architecture (v0.3)

Local Wispr Flow alternative. Researched against open-source dictation apps and implemented as a real desktop product, not a script soup.

## Research base

| Project | Stack | Lessons applied |
|---|---|---|
| [Handy](https://github.com/cjpais/Handy) | Tauri + React + Rust, offline Whisper/Parakeet | Native shell, tray, hotkeys, local ASR |
| [VoiceInk](https://github.com/Beingpax/VoiceInk) | Native macOS | Custom dictionary, per-app feel, history |
| [OpenWhispr](https://github.com/OpenWhispr/openwhispr) | Electron/Tauri-class desktop | SQLite history, sidebar (History/Dictionary/Settings) |
| [Muesli](https://github.com/Muesli-HQ/muesli) | Native macOS | **SQLite WAL mode** for dictation storage |
| [sflow](https://github.com/daniel-carreon/sflow) | macOS menubar | SQLite history, browse/search/copy |
| [FluidVoice](https://github.com/altic-dev/FluidVoice) | Swift macOS | On-device enhancement, no cloud default |
| Wispr Flow (closed) | Product reference (screenshots) | Home feed, Insights, Dictionary, Snippets, floating Dictate |

## Why SQLite, not GraphQL

GraphQL is a **network API** for multi-client backends. Wilson Voice is a **single-user local app**. The correct local stack is:

- **SQLite (WAL + FTS5)** for durable storage + millisecond search
- **Typed Tauri commands** as the app “API” (no HTTP hop)
- Optional future: local HTTP/GraphQL only if a second process (mobile companion, web dashboard) needs the same data

FTS5 gives fast substring/prefix search over all past dictations — same retrieval UX as Wispr’s searchable Home feed.

## Data model

```
~/Library/Application Support/WilsonVoice/wilson_voice.db

transcripts          — every dictation (text, backend, asr_seconds, word_count, created_at)
transcripts_fts      — FTS5 virtual table (auto-synced via triggers)
dictionary           — personal terms + preferred spellings + hit counts
scratchpad           — freeform notes
daily_stats          — words/sessions/asr_ms per day (insights)
settings.json        — model, language, auto_paste, show_floating
```

## UI map (Wispr-aligned)

| Screen | Purpose |
|---|---|
| **Home** | Searchable transcript feed, copy/paste, stats strip |
| **Insights** | Total words, WPM, streak, 7-day bars, engine health |
| **Dictionary** | Custom terms applied post-ASR |
| **Scratchpad** | Park text / assemble prompts |
| **Settings** | Model, language, auto-paste, floating pill, TCC deep-links |
| **Floating Dictate** | Always-on-top pill (Wispr “Dictate fn” analogue) |
| **Menu bar tray** | Open / toggle / quit — app lives in tray |

## Pipeline

```
⌥Space hold → ffmpeg avfoundation mic → WAV
→ python asr_worker (mlx_whisper large-v3-turbo)
→ dictionary rewrite
→ SQLite insert + FTS index
→ clipboard write
→ enigo Cmd+V paste into frontmost app
→ UI event + notification
```

## Identity / TCC

Bundle id: `com.wilsonguenther.wilson-voice`  
Appears as **Wilson Voice** in Mic / Accessibility / Input Monitoring (not “Python”).

## Run

```bash
cd ~/Desktop/wilson-voice/desktop
npm run desktop:dev     # dev
npm run desktop:build   # release .app + dmg
open "src-tauri/target/release/bundle/macos/Wilson Voice.app"
```

Requires arm64 venv at `~/Desktop/wilson-voice/.venv` with `mlx-whisper`.

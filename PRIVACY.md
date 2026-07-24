# Yap — Privacy Policy

_Last updated: July 24, 2026_

Yap is a dictation app that runs **entirely on your Mac**. This policy explains, plainly, what happens to your data. The short version: **your voice and your words never leave your device, and we never see them.**

## What we collect

**Nothing.** Yap has no account, no sign-in, and no servers that receive your data. We do not collect, transmit, sell, or have any access to your speech, your transcripts, or anything about how you use the app.

## How your dictation is handled

- When you hold the shortcut and speak, audio is captured **locally** and transcribed **on your Mac** using an on-device speech model (Whisper, via Apple's MLX). The audio is processed in memory and the temporary recording file is deleted immediately after transcription — including if transcription fails.
- The resulting text is placed on your clipboard and (optionally) pasted into the app you were typing in.
- A history of your transcripts is saved **only on your Mac**, in a local database inside your user Library folder. It is never uploaded.

## The only times Yap uses the network

For transparency, here is **every** network connection Yap makes:

1. **First-run model download.** The first time you use Yap, it downloads the speech model to your Mac so it can run offline afterward. After that, transcription is fully offline.
2. **Update checks.** Yap may check for a newer version so it can offer to update itself.

That's the complete list. Yap sends **no** analytics, telemetry, crash reports, usage data, or personal information to us or anyone else.

## Data stored on your Mac, and your control over it

- **Transcripts** (the text of what you dictated) and a custom **dictionary** are stored in a local SQLite database in your Library folder. Yap does **not** keep your audio.
- You can **delete any transcript**, **clear your entire history**, and **export diagnostics** at any time from within the app. Diagnostic logs are designed to contain **no transcript text**.
- Because the history is stored in a standard local database, we recommend keeping **macOS FileVault** turned on so your data is encrypted at rest along with the rest of your disk.

## Permissions Yap asks for

- **Microphone** — so it can hear you when you dictate.
- **Accessibility** — so it can paste transcribed text into the app you're using.

Both are used **only** for dictation. macOS controls these grants, and you can revoke them at any time in System Settings › Privacy & Security.

## Children

Yap is a general-purpose productivity tool and is not directed to children under 13.

## Changes to this policy

If this policy changes, the updated version will ship with the app and be posted alongside it, with a new "last updated" date.

## Contact

Questions about privacy? Email **wilson@drivia.consulting**.

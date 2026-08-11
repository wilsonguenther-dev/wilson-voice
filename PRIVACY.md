# Yap — Privacy Policy

_Last updated: August 11, 2026_

Yap is a dictation app that runs **entirely on your Mac**. This policy explains, plainly, what happens to your data. The short version: **your voice and your words never leave your device, and we never see them.**

## What we collect

**Nothing about your dictation.** Yap has no account, no sign-in, and no server that receives your speech, your transcripts, or anything about how you use the app.

The one thing we do hold is what it takes to honour a license you bought, and it is described in full below.

## How your dictation is handled

- When you hold the shortcut and speak, audio is captured **locally** and transcribed **on your Mac** by a speech model running on your own hardware. The audio is processed in memory and the temporary recording file is deleted immediately after transcription — including if transcription fails.
- The resulting text is placed on your clipboard and (optionally) pasted into the app you were typing in.
- A history of your transcripts is saved **only on your Mac**, in a local database inside your user Library folder. It is never uploaded.

## Buying a license

Purchases are handled by **Stripe**, which acts as the merchant of record for the payment. Your card details go to Stripe and never to us — we never see or store a card number. Stripe's own privacy policy governs what it collects at checkout, and it is worth a read if that matters to you.

From a completed purchase we keep a single record so we can prove the license is real and re-send it if you lose it. That record holds a **one-way hash of your email address** (not the address itself), the license's own identifiers, the Stripe session and payment identifiers behind it, the plan and seat count, and the date. When you ask us to re-send a lost key, we hash the address you type and look for a match — which is why we can email your key back to you without holding a list of who bought Yap.

Your name, your address, your transcripts, and anything you dictated are not in that record, because none of them ever reach us.

## The only times Yap uses the network

For transparency, here is **every** network connection Yap makes:

1. **First-run model download.** The first time you use Yap, it downloads the speech model to your Mac so it can run offline afterward. After that, transcription is fully offline.
2. **Update checks.** Yap may check for a newer version so it can offer to update itself. You can switch this off in Settings.
3. **The revocation check.** This is the **only** network call the licensing path makes. Your license is verified **on your Mac**, against a public key compiled into the app — activating it, and every launch after, involves no server at all. Separately, in the background, Yap downloads one small public file listing the licenses that have been refunded or charged back, and checks its own against that list locally. It is a plain download of a file that is identical for everyone: it carries no license key, no email, no device identifier, and no query of any kind, so it cannot tell us who is running Yap or where. If it fails — you are offline, the host is down, the response is garbage — nothing changes and Yap keeps working exactly as it did.

That's the complete list. Yap sends **no** analytics, telemetry, crash reports, usage data, or personal information to us or anyone else.

## Data stored on your Mac, and your control over it

- **Transcripts** (the text of what you dictated) and a custom **dictionary** are stored in a local SQLite database in your Library folder. Yap does **not** keep your audio.
- **Your license key** is stored in a small file alongside them, and is re-verified from that file on every read. Removing the license from a Mac is a button in Settings.
- You can **delete any transcript**, **clear your entire history**, and **export diagnostics** at any time from within the app. Diagnostic logs are designed to contain **no transcript text**.
- Because the history is stored in a standard local database, we recommend keeping **macOS FileVault** turned on so your data is encrypted at rest along with the rest of your disk.

## Permissions Yap asks for

- **Microphone** — so it can hear you. That covers dictation and any longer recording you start yourself, such as a meeting or a class. Yap listens when you tell it to and not otherwise; the audio is transcribed on your Mac and the recording file is deleted afterwards.
- **Accessibility** — so it can paste transcribed text into the app you're using, and read the few words just before your cursor to match the formatting you were already using. That context is used in the moment and is never stored or sent.

macOS controls both grants, and you can revoke them at any time in System Settings › Privacy & Security. Neither permission is ever used to send anything anywhere — see the complete list of network calls above.

## Recording other people

Yap does not announce itself to anyone in the room or on the call. If you record or transcribe other people, that is your call and your responsibility — laws differ by state and country, and some places require everyone's permission.

## Children

Yap is a general-purpose productivity tool and is not directed to children under 13.

## Changes to this policy

If this policy changes, the updated version will ship with the app and be posted alongside it, with a new "last updated" date.

## Contact

Questions about privacy? Email **wilson@drivia.consulting**.

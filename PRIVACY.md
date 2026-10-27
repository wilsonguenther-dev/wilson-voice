# Yap — Privacy Policy

_Last updated: August 11, 2026_

Yap is a dictation app that runs **entirely on your Mac**. This policy explains, plainly, what happens to your data. The short version: **your voice and your words never leave your device, and we never see them.**

## What we collect

**Nothing about your dictation.** Yap has no account, no sign-in, and no server that receives your speech, your transcripts, or anything about how you use the app.

The one thing we do hold is what it takes to honour a license you bought, and it is described in full below.

## How your dictation is handled

- When you hold the shortcut and speak, audio is captured **locally** and transcribed **on your Mac** by a speech model running on your own hardware. The audio is processed in memory and the temporary recording file is deleted as soon as the transcript is saved.
- **If a take fails, its audio is kept so you can retry it.** When transcription errors out, or a quality gate throws a take away, deleting the recording would mean you had to say the whole thing again — so instead Yap moves it into a recovery folder on your Mac for **up to 7 days**. It stays on your machine and is never uploaded; the full rules, and the button that deletes it immediately, are under **Data stored on your Mac** below.
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

Licensing makes **no** network call. Your license is verified **on your Mac**, against a public key compiled into the app — activating it, and every launch after, involves no server at all, and Yap never contacts us to check, renew or confirm it.

Those two are the complete list. Yap sends **no** analytics, telemetry, crash reports, usage data, or personal information to us or anyone else. There is a **Send crash report to Wilson** button in Settings, and it is not an exception to that: it writes a diagnostics file to your Desktop, shows you every line of it first, and then hands it to *your* mail app — Yap has no way to upload it, and nothing is sent unless you press send in your own email.

## Data stored on your Mac, and your control over it

- **Transcripts** (the text of what you dictated) and a custom **dictionary** are stored in a local SQLite database in your Library folder. Yap does **not** keep the audio of a successful dictation — that recording is deleted once the transcript is saved.
- **The audio of a failed take is kept, on your Mac, for up to 7 days.** A take whose transcription errors out, or that a quality gate rejects, has its recording moved into a `recovery` folder beside that database and listed under **Failed dictations** in History, so you can press **Retry** instead of saying it all over again. **Discard** deletes the recording immediately; anything you leave alone is deleted automatically after 7 days, whether you ever open History or not. Retrying re-transcribes it on your Mac exactly like the first attempt — the audio is never uploaded, to us or to anyone.
- **Your writing samples** — the emails or messages you paste under **Settings › Dictation › Writing style**, and any custom rules you write there — are stored in the same local database, in a table named `user_context`. They are used for one thing: they are shown to the polish model **running on your Mac** as examples of how you write, so the cleaned-up text sounds like you. They are never uploaded, never sent to us or to anyone (some other dictation apps send theirs to a cloud model; Yap does not), never written to a log, and never included in a diagnostics file or crash report. Remove a sample from Settings and it is deleted from the database.
- **Your license key** is stored in a small file alongside them, and is re-verified from that file on every read. Removing the license from a Mac is a button in Settings.
- You can **delete any transcript**, **clear your entire history**, and **export diagnostics** at any time from within the app. Diagnostic logs contain **no transcript text** — and since the crash-report button can put those logs in a file you might email, that is enforced rather than intended. Every line is run through a redaction pass before it is packed: file paths, your account name, email addresses, long tokens and long numbers are replaced with a visible marker, and then the line itself has to be, in full, a message Yap ships inside itself — one of its own log messages, or a system error string from a fixed list — with only values like numbers and file names filling the gaps. Anything else is replaced with a marker and a count. A sentence you spoke is never one of Yap's own messages, however short it is and wherever in the app it was written down, and a number you spoke is not a value Yap put there. Yap shows you the redacted contents before it writes anything, and the bundle never contains a recording, a database, a transcript or one of your writing samples.
- Because the history is stored in a standard local database, we recommend keeping **macOS FileVault** turned on so your data is encrypted at rest along with the rest of your disk.

## Permissions Yap asks for

- **Microphone** — so it can hear you. That covers dictation and any longer recording you start yourself, such as a meeting or a class. Yap listens when you tell it to and not otherwise; the audio is transcribed on your Mac and the recording file is deleted once the transcript is saved — or, if the take failed, held in the local recovery folder for up to 7 days so you can retry it.
- **Accessibility** — so it can paste transcribed text into the app you're using, and read a little of the text around your cursor to match the formatting you were already using. Exactly four things are read, each from the field you are dictating into, at the moment you finish a dictation: the text just **before the cursor** (at most 500 characters), the text you have **selected** (at most 500 characters), the text just **after the caret** (at most 200 characters), and — in Chrome, Safari and Firefox only — the **address of the web page** you are on, which is used for one thing: telling an email page (Gmail) from a document (Google Docs) so your words are formatted for it. The selection, the text after the caret and the page address never leave your Mac: they are read inside Yap's own process, they never leave the Rust function that uses them, and they are never stored in your history, never written to a log, never put in a support bundle, and never sent anywhere. The page address is reduced to the kind of page it is and then discarded on the spot. Yap never reads a page's HTML, never takes a screenshot, and never reads a password field. Without the Accessibility permission none of this is read and dictation works exactly as before.

macOS controls both grants, and you can revoke them at any time in System Settings › Privacy & Security. Neither permission is ever used to send anything anywhere — see the complete list of network calls above.

## Recording other people

Yap does not announce itself to anyone in the room or on the call. If you record or transcribe other people, that is your call and your responsibility — laws differ by state and country, and some places require everyone's permission.

## Children

Yap is a general-purpose productivity tool and is not directed to children under 13.

## Changes to this policy

If this policy changes, the updated version will ship with the app and be posted alongside it, with a new "last updated" date.

## Contact

Questions about privacy? Email **wilson@drivia.consulting**.

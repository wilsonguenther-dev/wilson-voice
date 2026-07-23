# Wispr Flow "Intelligence" Teardown — parity spec for Yap

> **Target app:** `Yap` (formerly Wilson Voice). Stack: **Tauri 2** (Rust core) +
> **Python MLX-Whisper** ASR daemon + **React 19 / Vite** UI. We already detect the
> frontmost app (`focus::frontmost_app_name`), keep a SQLite **dictionary** table with
> `initial_prompt` biasing + `apply_dictionary` replacement, and have a pure
> `dictation.rs` (`mode_for_app` + `format_dictation`) that is currently **log-only**
> (not wired into the paste output).
>
> **Goal of this file:** document *concretely* how Wispr Flow is "smart" — filler
> removal, self-correction, context/app awareness, commands, dictionary, intent, and
> session context — plus where superwhisper / MacWhisper / Aqua / Willow set the bar,
> and end with a **prioritized, testable backlog** for Yap "smart dictation v2" that
> drops straight into the CI/CD loop.
>
> Every claim below is sourced to a real URL (see each section + the appendix).
> Verified **2026-07-23**. Where third-party writeups conflict with Wispr's own docs,
> the official docs win and the conflict is flagged.

---

## 0. TL;DR — what actually makes Wispr "smart"

Wispr Flow is **not** a better transcriber. It is **Whisper-class ASR + a fine-tuned
Llama "cleanup" LLM pass**, run in the cloud in **~700 ms end-to-end (p99)**, fed a rich
**context object** (active app, cursor surroundings, on-screen text, your dictionary,
your name). The LLM's job, per co-founder Sahaj Garg, is to *"recreate exactly what the
user would have typed."* That second pass is where filler removal, self-correction,
punctuation, list/email/code formatting, and per-app tone all happen.

The five intelligence layers, in order:

1. **ASR** — speech → raw tokens (Whisper-class; Wispr says transcription "always occurs
   on the cloud"; some third-party writeups describe a hybrid on-device first pass —
   treat that as unconfirmed).
2. **Backtrack** — deletes fillers + spoken self-corrections using the *whole* utterance
   as context ("coffee at 2 actually 3" → "coffee at 3").
3. **Smart Formatting** — punctuation-by-name, capitalization, lists, casing that adapts
   to cursor position and app.
4. **Context Awareness + Style** — reads active app + text near cursor, applies a per-app
   tone (Very Casual → Formal), spells names right.
5. **Command Mode** — a *separate* hotkey that treats speech as an instruction ("make this
   more concise", "translate to Polish") rather than literal text.

**Yap's realistic path to parity:** keep on-device (that's our differentiator vs Wispr's
cloud-only, no-offline design) and replicate layers 2–5 with a **small local instruct
LLM via MLX-LM** (Qwen2.5-1.5B/3B-Instruct or Llama-3.2-3B-Instruct, 4-bit) added to the
*same* MLX venv/daemon we already run for Whisper. Wispr proves the two-pass
(ASR → small fine-tuned LLM) architecture works; we do it locally.

Sources: [Baseten case study](https://www.baseten.co/resources/customers/wispr-flow/) ·
[Data Controls](https://wisprflow.ai/data-controls) ·
[Smart Formatting & Backtrack](https://docs.wisprflow.ai/articles/5373093536-how-do-i-use-smart-formatting-and-backtrack)

---

## 1. Polishing / cleanup — filler, false starts, self-correction, grammar

### What Wispr does

- **Two-pass design.** ASR captures audio; a **fine-tuned Llama** model does
  "transcript enhancement" — format + contextualize. Wispr chose Llama because "it's
  controllable and customizable." Runs on Baseten/AWS with TensorRT-LLM; **"100+ tokens
  in <250 ms"**, whole pipeline **<700 ms p99**.
  ([Baseten](https://www.baseten.co/resources/customers/wispr-flow/))
- **Backtrack** (the self-correction engine) removes fillers and spoken corrections **two
  ways**: (a) trigger phrases — "actually", "scratch that"; (b) plain **restatement** —
  "Flow uses your full dictation as context to decide what to change." Documented example:
  *"Let's do coffee at 2 actually 3"* → *"Let's do coffee at 3."*
  ([Smart Formatting & Backtrack](https://docs.wisprflow.ai/articles/5373093536-how-do-i-use-smart-formatting-and-backtrack))
- **Auto Cleanup has 4 levels: None / Light / Medium / High** (Settings → Style → Auto
  Cleanup). Filler removal, grammar, punctuation, and capitalization scale with the level.
  ([review roundup](https://spokenly.app/blog/wispr-flow-review))
- **Raw vs polished is preserved and reversible.** On desktop: hover a transcript in
  history → three dots → **"Undo AI edit"** reveals the raw version; redo anytime. You can
  also configure Flow to paste the raw transcription with no AI formatting.
  ([Smart Formatting](https://docs.wisprflow.ai/articles/5373093536-how-do-i-use-smart-formatting-and-backtrack))
- **Caveat:** on Mac/Windows **Smart Formatting is always on — there is no toggle**; only
  iOS exposes an on/off. The reversibility is per-transcript via Undo, not a global switch.

### Implication for Yap

We currently do **no** cleanup — `apply_dictionary` (string replace) and `format_dictation`
(list detection, log-only) are the whole "polish." The single highest-ROI feature is a
**local LLM cleanup pass** between ASR and paste, guarded so it can **never** lose text
(fallback to raw on timeout/error — preserves our "never lose text" guarantee). Ship an
**Undo AI edit / raw↔polished** control from day one; store both `raw_text` and
`polished_text` on the transcript row.

---

## 2. Context / app awareness — tone & formatting per active app

### What Wispr does

- **Context Awareness** (on by default) "reads your active app and adapts transcription
  accuracy, style, and formatting." It buckets apps into **4 categories: Email, Work
  messaging, Personal messaging, Other.**
  ([Context Awareness doc](https://docs.wisprflow.ai/articles/4678293671-feature-context-awareness))
- **What it reads:** "app info, textbox contents (before, selected, and after the cursor),
  on-screen text, variable and file names in coding apps." For **Slack / Apple Messages**
  it reads conversation context. For browser apps it resolves the **specific site** (Gmail
  vs Google Docs vs Chrome, individually). In **Notion** it *skips* context-aware
  formatting when surrounding text is ≤2 words or ends with "…".
- **What changes per app:**
  - *Gmail* → detects you're composing email, reads the recipient's name, applies your
    email writing style.
  - *Slack/Teams* → switches to casual messaging style, **removes trailing periods**.
  - *Cursor/VS Code/Windsurf* → code-aware output; **remembers file names across sessions**.
- **Style Personalization** is the tone dial, set **manually** (not learned): per app
  category you pick **Very Casual / Casual / Formal / Excited**. "Very Casual = no caps +
  less punctuation"; "Formal = caps + more periods." It only touches **formatting** — it
  explicitly "does not change your grammar, word choice, or phrasing."
  ([Personalized Style](https://wisprflow.ai/post/personalized-style))
- **Privacy:** password-field contents are never read; with Privacy Mode on, context data
  isn't retained/trained on.
  ([Data Controls](https://wisprflow.ai/data-controls))

### Implication for Yap

We already have the detector (`mode_for_app` → Email/Document/Notes/Code/Chat/Plain) — it
just isn't wired to output. Parity = **feed the detected mode into the LLM polish prompt**
(different system prompt per mode) and add a **per-mode style setting** (casual↔formal).
Reading text near the cursor on macOS is doable via the **Accessibility API**
(`AXSelectedText` / `AXValue` of the focused element) — that's our local analog of Wispr's
`textbox_contents`, with zero cloud.

---

## 3. Commands / self-editing — intent vs literal text

### What Wispr does

- **Command Mode** is a **separate hotkey** (Mac: `Fn+Ctrl` or `Cmd+Ctrl+Option`), so the
  command/literal ambiguity is solved by **which key you held** — not by parsing.
  ([Command Mode](https://docs.wisprflow.ai/articles/4816967992-how-to-use-command-mode))
  Three command classes:
  1. **Transform selection** — highlight text, say "make this more concise / more
     assertive / translate to Polish."
  2. **Generate** — with nothing highlighted, ask a question → answer inserted inline.
  3. **Adjust settings by voice** — "always capitalize acronyms", "never use exclamation
     marks" → updates Polish rules (but only after you tap **Apply**).
- **Inline literal commands during normal dictation:** punctuation-by-name ("period",
  "comma", "exclamation point", "em dash"), "new line"/"new paragraph", and **"press
  enter"** — the last is case-insensitive and **only** honored at the *end* of a
  dictation, which is how Wispr disambiguates it from literal text.
  ([Smart Formatting](https://docs.wisprflow.ai/articles/5373093536-how-do-i-use-smart-formatting-and-backtrack))
- **Aqua Voice** takes the opposite bet: *no* command syntax or mode — you just say "make
  this a list", "rephrase that", "redo the second sentence" mid-flow and it infers
  command-vs-content. This is the harder, "smarter"-feeling UX.
  ([Aqua review](https://vmake.ai/blog/aqua-voice-speech-to-text-tool))

### Implication for Yap

Cheapest win with zero ambiguity: **inline literal tokens** parsed in Rust *before* the
LLM pass — "new line/new paragraph", punctuation-by-name, and a trailing "press enter"
flag on the paste. A true **Command Mode** = a second global shortcut whose transcript is
sent to the local LLM as an *instruction over the current selection* (read selection via
AX, transform, paste back). Wispr's "separate hotkey" trick is the safe design — avoid
Aqua-style inference until the polish pass is solid.

---

## 4. Personal dictionary / vocabulary

### What Wispr does

- **Add words** manually (Dictionary → Add new), by **CSV import**, or via **auto-learn**.
- **Auto-learn from corrections:** if you type over a transcription, Flow notices and adds
  the corrected spelling; it "learns distinctive or specialized words… common everyday
  words are filtered out." Auto-learned entries get a **✨ sparkle** (contact-imported get
  **👤**).
  ([Dictionary doc](https://docs.wisprflow.ai/articles/4052411709-teach-flow-your-words-with-the-dictionary))
- **Misspelling replacement rules:** "Correct a misspelling" swaps a wrong spelling Flow
  keeps producing ("Eric"→"Erik") automatically. One replacement rule per word.
- **Starring = priority:** starred words win when Flow has conflicting/many entries.
- **Word boosting:** dictionary terms bias recognition of uncommon/specialized words at
  transcription time. **60-char limit per entry** (59 on iOS).
- **Snippets / voice shortcuts** (text replacement): say a short cue → Flow expands the
  full block (email address, scheduling link, FAQ, canned reply).

### Implication for Yap

We already do **word boosting** (`top_dictionary_terms` → Whisper `initial_prompt`) and
**replacement** (`apply_dictionary`). The gaps vs Wispr are: (1) **auto-learn from
corrections** — but Yap can't watch the target app's edits like Wispr's keyboard does; our
realistic version is **harvest from history** (already partially built) + a one-click
"add last correction" and CSV import; (2) **snippets/voice shortcuts** — a `snippets`
table `(cue → expansion)` applied in the same pass as `apply_dictionary`; (3) **starring**
= a `priority` column so starred terms sit last in the `initial_prompt` (most-weighted).

---

## 5. "What you were trying to say" — whisper-to-intent

### What Wispr does (and doesn't)

- Wispr's stated ceiling is **"recreate exactly what the user would have typed"** — i.e.,
  clean up disfluencies and formatting, **not** rewrite your meaning. Backtrack resolving
  "2 actually 3" → "3" is the closest thing to intent inference in default dictation.
- Rewriting to what you *meant* is **opt-in via Command Mode** ("make this more concise /
  assertive"), never automatic. This is a deliberate trust boundary — auto-rephrasing
  silently is where dictation tools lose users.
- **Aqua Voice** markets furthest into intent: "gets what you mean… clarifying a spelling
  on the fly or rephrasing a sentence," treating the doc as a **living document you edit by
  voice** (fusion transcription + client context engine), and claims a **0.05 WER**
  beating Whisper large-v3.
  ([Aqua](https://vmake.ai/blog/aqua-voice-speech-to-text-tool) ·
  [9to5Mac](https://9to5mac.com/2025/08/15/aqua-voice-shows-just-how-good-mac-dictation-could-be/))

### Implication for Yap

Match Wispr's default (clean disfluencies, don't rewrite meaning) — put true rephrase
behind an explicit **Command Mode** so we never silently change intent. An **"explain/
suggest"** affordance (show raw + polished side by side, let the user pick) is the honest
version of "what you were trying to say" and is trivial once we store both texts.

---

## 6. Session / history awareness + a context input API (Wilson's idea)

This is the highest-leverage finding for Yap.

### Wispr already has exactly this — and publishes the schema

Wispr's **Flow API** (invite-only) exposes **persistent sessions** that "maintain context
across multiple transcription calls," plus WebSocket streaming and batch endpoints.
([API search results](https://api-docs.wisprflow.ai/quickstart)) More importantly, the
**request schema is public** and is a ready-made blueprint for a local context API. Each
transcribe request accepts a `context` object:
([request schema](https://api-docs.wisprflow.ai/request_schema.md))

| Field | Meaning |
|---|---|
| `app.name`, `app.type` | active app name + category (`email`, `ai`, `other`) |
| `dictionary_context` | "list of uncommon names or words relevant to the context that might be mentioned" |
| `user_first_name` / `user_last_name` | so it spells the speaker's name right |
| `user_identifier` | user id/email in the target app |
| `textbox_contents.before_text` / `.selected_text` / `.after_text` | text around the cursor |
| `screenshot` | image of the app/screen |
| `content_text` / `content_html` | plaintext / HTML of the current page |
| `conversation` | chat history: participants + messages (role, content) |

**This is the design Wilson wants.** A coding tool (Claude Code / Codex) knows the session
topic, file names, and jargon; Wispr's `dictionary_context` + `content_text` fields are
precisely where that would be injected. Wispr's own Context Awareness proves the payoff:
"remembers file names seen in Cursor/Windsurf/VS Code across dictation sessions."

### Designing a lightweight local context API for Yap

Yap already runs a Rust core; add a **loopback HTTP endpoint** (bind `127.0.0.1` only, no
external surface) that mirrors a **subset** of Wispr's schema — enough for a coding agent
to push session context:

```
POST http://127.0.0.1:{port}/v1/context     # set/replace the current context
{
  "source": "claude-code",                   # who is pushing
  "session_title": "Refactor asr.rs warm daemon",   # → content_text / topic
  "app": { "name": "iTerm2", "type": "code" },
  "dictionary_context": ["asr_worker", "mlx_whisper", "NSPanel", "ptt_macos"],
  "before_text": "...", "selected_text": "...",     # optional, from AX or the tool
  "ttl_seconds": 900                          # auto-expire so stale topics don't bias
}
POST http://127.0.0.1:{port}/v1/context/clear
```

Yap merges the pushed `dictionary_context` into the Whisper `initial_prompt` (on top of
the learned dictionary) and passes `session_title` + `selected_text` into the **LLM polish
prompt** as "topic/context." A CLI shim (`yap-context "<title>" --terms a,b,c`) lets any
tool — or a Claude Code hook — push context in one line. Locally we can also read
`textbox_contents` ourselves via the macOS **Accessibility API** (focused element's
`AXSelectedText`/`AXValue`), so the coding tool only needs to supply the *topic + jargon*
it uniquely knows.

**Guardrails:** localhost-only, TTL expiry, size cap on terms, and it only *biases* — the
"never lose text" path is untouched if the endpoint is down.

---

## 7. Competitive landscape — where the bar is

| Tool | Where it runs | Polish / cleanup | Formatting & context | Commands | Vocabulary | Notes |
|---|---|---|---|---|---|---|
| **Wispr Flow** | **Cloud only**, no offline; ~700 ms p99 | Fine-tuned **Llama** 2nd pass; Backtrack; Auto Cleanup None/Light/Med/High; Undo AI edit | Context Awareness (4 app cats) + manual Style per category; reads cursor text + on-screen + file names | **Command Mode** (separate hotkey) + inline punctuation/"press enter" | Auto-learn from corrections (✨), snippets, misspelling rules, starring, CSV | 104 langs; $30M Series A (Menlo), ~$81M total; public `context` request schema |
| **superwhisper** | **Local models** (Whisper/Parakeet) *or* cloud LLM | Per-**mode** LLM post-process (GPT/Claude/**Llama** local) with your own prompt | **Super Mode** reads active app + selected text + recent clipboard; built-in modes (Email/Message/Note/Meeting) | Prompt-driven (write the mode's instructions) | Custom vocabulary | Fully offline possible; XML-tagged custom prompts. Closest architectural twin to Yap. |
| **MacWhisper** | **Local** (Whisper on Apple Silicon) | Cleanup is cloud or **none** (weak spot) | File-focused; weaker real-time app adaptation | Limited | Custom vocabulary | Best for transcribing recorded files, not app-wide dictation |
| **Aqua Voice** (YC W24) | Cloud; multi-model (GPT/Claude) | Strong; "living document" edit-by-voice, self-correction | Reads active app/site, tone-adapts (Slack/Gmail/Cursor); fusion transcription + client context engine | **No syntax** — infers command vs content ("make this a list", "redo sentence 2") | — | Claims 0.05 WER / 99.1% acc, <200–450 ms; furthest into "intent" |
| **Willow** | Cloud | High "zero-edit" rate; learns your vocab over time | Auto tone/format per app | — | Compounding vocab learning | ~200 ms latency, +40% vs built-in dictation |
| **WhisperKit** (open) | **On-device** real-time ASR | ASR only (no LLM polish) | — | — | — | Reference for on-device streaming ASR on Apple Silicon |

**The bar:** every serious competitor now ships (1) an **LLM cleanup pass**, (2) **per-app
tone/formatting**, and (3) **learned/custom vocabulary**. Yap has vocabulary; it is missing
the LLM cleanup pass and wired-in per-app formatting. **superwhisper is the closest twin**
(local Whisper/Parakeet + a local-LLM post-process per mode) — it's the proof our target
architecture ships and stays offline.

Sources: [superwhisper custom modes](https://superwhisper.com/docs/modes/custom) ·
[Willow vs superwhisper](https://willowvoice.com/blog/willow-vs-super-whisper-mac-dictation) ·
[MacWhisper alternatives](https://voicedash.ai/best-macwhisper-alternatives/) ·
[WhisperKit paper](https://arxiv.org/html/2507.10860v1)

---

## 8. Prioritized backlog — Yap "smart dictation v2"

Ordered by ROI. Each item: one-line spec + a **testable acceptance idea** for the CI/CD
loop. Rust-pure helpers get unit tests; the LLM pass gets **golden input→output fixtures**;
the context API gets an HTTP integration test. All LLM-touching items **must** fall back to
raw text on timeout/error (protects "never lose text").

### P0 — the intelligence unlock

1. **Local LLM polish pass (MLX-LM).** Add a small instruct model
   (Qwen2.5-1.5B/3B-Instruct-4bit or Llama-3.2-3B-Instruct-4bit) to the existing MLX venv;
   after ASR + `apply_dictionary`, run a tight "clean this dictation, don't change meaning"
   prompt; paste polished, store both `raw` and `polished`.
   *Accept:* golden fixture set (≥15 pairs: filler, run-ons, missing punctuation) where
   polished output matches expected within a normalized diff; **on a 1500 ms timeout the
   pasted text equals raw** (assert in an injected-failure test).

2. **Backtrack / self-correction + filler removal.** Handle trigger phrases ("actually",
   "scratch that", "no wait, I mean") and drop fillers ("um", "uh", "like", "you know")
   inside the polish prompt.
   *Accept:* fixture `"coffee at 2 actually 3"` → `"coffee at 3"`; `"um so like the thing"`
   → `"the thing"`; a control prose sample is returned unchanged.

3. **Raw ↔ polished toggle + "Undo AI edit."** Store `raw_text` + `polished_text` on the
   transcript row; history UI shows both; a global setting picks default paste (raw /
   polished) and Auto-Cleanup level (None/Light/Medium/High).
   *Accept:* DB migration test confirms both columns persist; unit test: `level=None`
   returns raw verbatim; toggling in history swaps clipboard content.

### P1 — context & formatting (wire up what we half-built)

4. **Wire `mode_for_app` into the polish prompt.** Select a per-mode system prompt
   (Email/Document/Notes/Code/Chat/Plain) instead of log-only.
   *Accept:* fixture — same raw utterance under `Email` gets a greeting/sign-off shape;
   under `Chat` stays lowercase, no trailing period; under `Code` leaves identifiers/
   camelCase untouched. Assert on the chosen prompt id + output shape.

5. **Per-mode Style setting (casual↔formal).** Mirror Wispr's Very Casual/Casual/Formal/
   Excited as a per-category dial that adjusts caps + punctuation only (never word choice).
   *Accept:* `VeryCasual` fixture → no capitalization, minimal punctuation; `Formal` →
   capitalized + terminal periods; grammar/words identical between the two.

6. **Local context API (loopback HTTP + `yap-context` CLI).** `POST /v1/context` accepting
   `{session_title, app, dictionary_context[], selected_text?, ttl_seconds}`; merge terms
   into Whisper `initial_prompt` and inject `session_title` into the polish prompt; auto-
   expire.
   *Accept:* integration test posts context with a rare term, runs a fixture WAV/stub ASR,
   asserts the term appears in the built `initial_prompt` and expires after TTL; endpoint
   bound to `127.0.0.1` only (assert refusal from non-loopback).

7. **Read cursor context via macOS Accessibility.** Pull focused element's
   `AXSelectedText`/`AXValue` as our local `textbox_contents`; feed `selected_text` into
   Command Mode and mid-sentence casing into the polish pass.
   *Accept:* harness reads a known selection from a test target app and the value reaches
   the pipeline; gracefully no-ops (falls back) when AX permission is absent.

### P2 — commands & vocabulary depth

8. **Inline literal commands (Rust, pre-LLM).** Parse "new line"/"new paragraph",
   punctuation-by-name ("period", "comma", "em dash"), and a trailing "press enter" flag.
   *Accept:* pure-fn fixtures: `"line one new line line two"` → `"line one\nline two"`;
   `"done period"` → `"done."`; trailing `"press enter"` sets the enter flag and is
   stripped from text.

9. **Command Mode (second global shortcut).** Speech under this hotkey = an instruction
   over the current AX selection ("make this concise", "make this a list", "translate to
   X"); transform via the local LLM, paste back.
   *Accept:* fixture selection + `"make this a bulleted list"` → bulleted list; disambiguation
   test: identical words under the *dictation* hotkey are transcribed literally.

10. **Snippets / voice shortcuts.** `snippets(cue, expansion)` table applied alongside
    `apply_dictionary`; say a cue → paste the full block.
    *Accept:* cue `"my scheduling link"` expands to the stored URL; a cue that is a
    substring of a normal word does **not** trigger (word-boundary test).

11. **Dictionary v2 — starring + auto-learn from corrections + CSV import.** `priority`
    column (starred terms weighted last/most in `initial_prompt`); "add last correction"
    action; CSV import/export.
    *Accept:* starred term outranks an unstarred conflict in the built prompt order; CSV
    round-trips; "add correction" persists a replacement rule that then fires in
    `apply_dictionary`.

### P3 — polish & trust

12. **Explain/suggest UX ("what you meant").** Side-by-side raw vs polished in history with
    one-tap accept of either; never auto-rewrites meaning in the default path.
    *Accept:* UI test shows both strings; accepting raw writes raw to clipboard, accepting
    polished writes polished.

13. **Privacy posture doc + "fully local" badge.** Since everything runs on-device (no
    cloud, unlike Wispr), surface that as the headline differentiator.
    *Accept:* n/a (docs) — but assert via a network test that the dictation path makes **no
    outbound connections** (Wispr can't claim this).

---

## Appendix — sources (all verified 2026-07-23)

**Wispr Flow — official docs & site**
- Context Awareness — https://docs.wisprflow.ai/articles/4678293671-feature-context-awareness
- Smart Formatting & Backtrack — https://docs.wisprflow.ai/articles/5373093536-how-do-i-use-smart-formatting-and-backtrack
- Command Mode — https://docs.wisprflow.ai/articles/4816967992-how-to-use-command-mode
- Dictionary / auto-learn — https://docs.wisprflow.ai/articles/4052411709-teach-flow-your-words-with-the-dictionary
- Personalized Style — https://wisprflow.ai/post/personalized-style
- Data Controls / Privacy Mode — https://wisprflow.ai/data-controls
- Features — https://wisprflow.ai/features
- Developer platform / API — https://wisprflow.ai/developers · https://api-docs.wisprflow.ai/quickstart
- **API request/`context` schema** — https://api-docs.wisprflow.ai/request_schema.md
- Slack use case — https://wisprflow.ai/use-cases/slack

**Wispr Flow — architecture & background**
- Baseten case study (Llama, 700 ms, TensorRT-LLM) — https://www.baseten.co/resources/customers/wispr-flow/
- Wikipedia (founding, funding, langs) — https://en.wikipedia.org/wiki/Wispr_Flow
- Zack Proser dev review — https://zackproser.com/blog/wisprflow-review

**Competitors**
- superwhisper custom modes — https://superwhisper.com/docs/modes/custom · https://github.com/superultrainc/superwhisper-docs/blob/main/modes/customizing-modes.mdx
- Aqua Voice — https://vmake.ai/blog/aqua-voice-speech-to-text-tool · https://9to5mac.com/2025/08/15/aqua-voice-shows-just-how-good-mac-dictation-could-be/ · https://aquavoice.com/
- Willow — https://willowvoice.com/blog/willow-vs-super-whisper-mac-dictation · https://willowvoice.com/blog/super-whisper-vs-wispr-flow-comparison-reviews-and-alternatives
- MacWhisper alternatives / comparison — https://voicedash.ai/best-macwhisper-alternatives/
- WhisperKit on-device ASR (paper) — https://arxiv.org/html/2507.10860v1

**Reviews used for feature confirmation**
- Spokenly Wispr review (Auto Cleanup levels) — https://spokenly.app/blog/wispr-flow-review
- Willow Wispr review — https://willowvoice.com/blog/wispr-flow-review-voice-dictation
- Chris Menard (filler removal) — https://chrismenardtraining.com/post/wispr-flow-ai-dictation-removes-filler-words/

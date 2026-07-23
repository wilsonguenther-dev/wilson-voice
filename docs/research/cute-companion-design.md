# Yap — Cute Companion Design Research

**Goal:** design the floating companion for Yap (open-source macOS dictation app) as an *adorable digital pet* — a cute animated creature, not an abstract shape. The previous origami/abstract version was rejected as "not cute." The pet lives in a tiny always-on-top WebKit webview, must be **hand-coded** (Canvas2D/SVG/CSS, no library drop-in), run at 60fps, and stay low-CPU.

This document is self-contained: genre study, the science + craft of cuteness, a build-decision on pixel-vs-vector, how the pet should signal dictation states, concrete character concepts for Yap, and a final ranked recommendation.

---

## 0. TL;DR — The Recommendation (read this first)

**Build a baby chick 🐥.** A round, egg-shaped yellow chick with a huge head, two enormous glossy eyes low on the face, tiny stubby wings, and — critically — an **expressive triangular beak that opens and closes**. "Yap" *is* talking, and a beak is the single most on-brand, most legible mouth you can animate by hand. It is also the easiest to draw cute at ~120px because it's essentially two overlapping circles plus a triangle.

**Render it as soft vector (SVG/Canvas paths), not pixel art.** At ~120px a vector chick reads rounder, softer, and cuter, scales crisply on Retina, and lets you do the springy squash-and-stretch that actually *sells* the cuteness. Pixel art wins nostalgia but loses at facial expressiveness at this size unless you commit to a large sprite budget.

**Runner-up concept:** a round blob/slime (think Pou's silhouette) — even simpler to animate, extremely on-trend, but a beak-less mouth is a weaker "yapping" signal. Keep it as the fallback / alternate skin.

**The five reference charms to match** (detailed at the end): Dinoki, Finch, Pou, oneko.js, and the Tamagotchi "few pixels, huge feeling" economy — plus Vlambeer/Disney motion craft as the animation spine.

---

## 1. The Desktop-Pet / Digital-Pet Genre — Study the Adorable Ones

What separates a *charming* companion from an *annoying* one is a recurring theme in every source below: **restraint + reactivity + personality, without demanding attention.** Clippy failed precisely because it was "too eager, too generic, too visible" and interrupted; the beloved ones stay out of the way and only come alive when you engage them ([Windows Forum — Clippy lessons](https://windowsforum.com/threads/clippy-lessons-for-microsoft-copilot-when-assistants-become-intrusive.411922/); [Nathalie Lawhead — About desktop pets](https://www.nathalielawhead.com/candybox/about-desktop-pets-virtual-companions-discussing-the-inhabitants-that-fill-the-void-of-our-digital-spaces)). Lawhead's key design law: *"every desktop pet demands a different level of attention — you have to communicate the type of attention it requires and honor that."* For a dictation tool this is gold: Yap's pet should be near-invisible when idle and only "perform" during capture.

### The reference set

| Pet | What it is | How it's built | Charming or annoying — why |
|---|---|---|---|
| **Dinoki** ([dinoki.ai](https://dinoki.ai/), [Show HN](https://news.ycombinator.com/item?id=44798849), [Indie Hackers build post](https://www.indiehackers.com/post/building-dinoki-native-ai-companion-for-mac-windows-4228efd94a)) | Native pixel-art AI desktop pet for Mac/Windows; "Tamagotchi vibes + GPT smarts" | **6 MB native SwiftUI, no Electron**, fully local, privacy-first. Pixel-art sprite pet that evolves. | **Charming.** Tiny footprint, native feel, a pet that *evolves* creates attachment. Closest spiritual sibling to Yap. Proof a pixel pet can feel premium and lightweight. |
| **Finch: Self-Care Pet** ([App Store](https://apps.apple.com/us/app/finch-self-care-pet/id1528595748), [MWM](https://mwm.ai/apps/finch-self-care-pet/1528595748)) | A soft-vector baby bird ("birb") tied to self-care habits | Soft flat-vector 2D character, gentle pastel palette, dress-up customization | **Charming.** *"The bird design itself is perfect."* Warm, non-judgmental framing; the pet benefits when *you* do well. The gold standard for "gentle, externalized motivation" and a bird that isn't cloying. |
| **oneko.js / neko** ([adryd325/oneko.js](https://github.com/adryd325/oneko.js/), [rinvii/neko](https://github.com/rinvii/neko), [Wikipedia](https://en.wikipedia.org/wiki/Neko_(software))) | The classic cursor-chasing cat, 1989 → JS | **32×32 sprite sheet**, a `div` with `background-position` offsets, ~100ms frame tick via `requestAnimationFrame`, tiny state machine (see §3/§4) | **Charming, and the cheapest possible reference.** Its whole charm is *reactivity* — it chases the cursor, sleeps when idle, scratches, yawns. Directly instructive for how Yap's pet should react to attention. |
| **Shimeji-ee** ([DalekCraft2/Shimeji-Desktop](https://github.com/DalekCraft2/Shimeji-Desktop), [gil/shimeji-ee](https://github.com/gil/shimeji-ee)) | Java desktop mascot with **swappable asset packs** | Per-character `img/<Name>/` folders + XML-defined actions/animations; looks for `shime1.png` | **Charming but heavy to author.** The lesson isn't the tech (dated Java) — it's the **asset-pack architecture**: one engine, many skins. Yap should separate the *animation rig* from the *character skin* so a chick, blob, or cat are drop-in swaps. |
| **Bongo Cat** ([shouwangzhe134/bongo-cat](https://github.com/shouwangzhe134/bongo-cat), [Gamma-Software/BongoCat-mac](https://github.com/Gamma-Software/BongoCat-mac)) | Cat that drums to your keyboard/mouse input | **Input-driven frame-swap overlay** — presses a key → swap to that key's sprite. Native Swift mac build exists. | **Charming *because* it's input-reactive in real time.** The exact pattern Yap wants: map an input signal (here: mic amplitude) to instant sprite/pose changes so the pet feels *connected* to what you're doing. |
| **clippy.js** ([clippyjs/clippy.js](https://github.com/clippyjs/clippy.js), [pi0/clippyjs](https://github.com/pi0/clippyjs)) | JS re-implementation of MS Agent (Clippy & friends) | Sprite-sheet agents with **probabilistic idle animations** — e.g. Links the cat flips a coin while idle: 10% chance to turn or scratch | **The cautionary tale + one great idea.** Clippy = intrusive/annoying. But the **probabilistic idle** (random small behaviors so the pet never loops identically) is worth stealing — it's what makes an idle feel *alive* rather than mechanical. |
| **Pou** ([softonic listing](https://pou.en.softonic.com/android)) | Round brown potato/blob alien pet | Minimalist cartoon vector, big eyes, "gibberish" voice; care/feed/mini-games | **Charming.** *"Familiar shape, big eyes, adorable expression."* Expressions change **gradually** → feels emotionally consistent. It "speaks gibberish" — a direct cue for a *yapping* pet. The blob silhouette is the easiest cute shape to animate. |
| **Nintendogs** | 3D puppy pet | AAA 3D (not our target), but | **Charming.** The benchmark for *reactive affection* — looks at you, responds to voice, expresses joy. Aspirational, not a build reference. |
| **Tamagotchi** ([JapanHouse LA — how it changed digital design](https://www.japanhousela.com/articles/how-tamagotchi-changed-digital-design-icon-japanese-tiny-toys-30th-anniversary-bandai/), [1996 pet wiki](https://tamagotchi.fandom.com/wiki/Tamagotchi_(1996_Pet))) | The original pocket pet | **32×16 px LCD, a handful of black dots** | **The masterclass in economy:** *"minimalism of design with maximalism of emotional impact… a few pixels could evoke genuine care."* If a chick can be adorable in 32×16 monochrome, Yap has no excuse. |
| **Divoom Pixoo / M5Stack "vpet"** ([Pixel-Pets on M5Stack](https://github.com/marceld23/Pixel-Pets), [pixoo64 topic](https://github.com/topics/pixoo64), [erodozer/vpet](https://github.com/erodozer/vpet)) | DIY LED-matrix hardware pets | Low-res pixel sprites on 16×16/64×64 LED grids | **Charming via constraint.** Confirms the Tamagotchi lesson on real hardware: cuteness survives *extreme* low resolution if the silhouette + eyes are right. |
| **VPet** ([LorisYounger/VPet](https://github.com/LorisYounger/VPet)) | Open-source WPF virtual pet, embeddable in any WPF app | WPF; `VPet-Simulator.Core` is a reusable engine you embed; mod tools for custom pets | **Charming + architecturally instructive.** Like Shimeji, it proves the value of a **reusable pet-core you drop into a host app** — mirror this by keeping Yap's pet a self-contained webview module. |
| **Desktop Mate** ([Steam](https://store.steampowered.com/app/3301060/Desktop_Mate/), [MateEngine](https://store.steampowered.com/app/3625270/MateEngine/)) | 3D anime companion that sits on your windows | **Unity + VRM** models; idle animations, cursor-chase, head-pat, low resource use | **Charming for its audience.** The interaction vocabulary — sits on top of windows, chases cursor, reacts to pats — is the modern desktop-pet UX playbook. |
| **WidgetPet / desktop-pet genre** ([GitHub desktop-pet topic](https://github.com/topics/desktop-pet)) | Widget-style mini pets | Various | The broad genre confirms the winning formula: **small, always-on-top, reactive, personality-forward, non-intrusive.** |

**The synthesized charm formula (what to bake into Yap):**
1. **Reactive, not autonomous-annoying** — it comes alive when you speak (Bongo Cat, oneko), sleeps/peeks when you don't.
2. **Personality through micro-behavior** — probabilistic idle fidgets (clippy.js), gradual expression changes (Pou).
3. **Tiny + native-feeling** — Dinoki's 6 MB ethos; low CPU is a *feature*, not a compromise.
4. **Attachment through evolution/care is optional but powerful** — Tamagotchi/Dinoki/Finch. Even a name and a blink cadence create a "someone's there" feeling.
5. **Skin/rig separation** — Shimeji/VPet: one animation engine, swappable characters.

---

## 2. The Visual Language of CUTENESS (Baby Schema / *Kindchenschema*)

Cuteness isn't subjective vibes — it's a measurable perceptual trigger. Ethologist **Konrad Lorenz** (1943) described the *Kindchenschema* ("baby schema"): infant proportions that hijack adult caregiving circuits. Modern fMRI/behavioral work confirms exaggerating these features **increases cuteness ratings, baby-talk, and motivation to care** ([Glocker et al. 2009, PubMed](https://pubmed.ncbi.nlm.nih.gov/22267884/); [Lorenz's baby schema review, PMC11285920](https://pmc.ncbi.nlm.nih.gov/articles/PMC11285920/); [phys.org summary](https://phys.org/news/2015-06-infant-animals-cute.html)).

### Lorenz's seven (+1) cuteness features — verbatim intent, translated to a coder's rules

| # | Lorenz feature | Rule you can code |
|---|---|---|
| 1 | Large head relative to body | **Head ≈ 40–55% of total body height.** Baby-est end: head as tall as the whole body (chick = basically all head). |
| 2 | Domed, protruding forehead; braincase dominates the face | **Cranium mass sits high and rounded**; the "face" (eyes/nose/mouth) occupies only the **lower ~40%** of the head. |
| 3 | Large, deep-set eyes **below the vertical midline of the skull** | **Place eye centers at ~55–65% down the head height** (i.e. below center), not at the middle. Eyes big: **eye diameter ≈ 20–30% of head width each.** |
| 4 | Short, thick limbs; large hands/feet | Stubby wings/paws/feet; **no long thin limbs.** Feet slightly oversized. |
| 5 | Rounded body shapes | **Everything is circles and superellipses.** No sharp corners anywhere in the silhouette. |
| 6 | Soft, elastic surface texture (the "baby fat" look) | Soft gradient shading, a subtle inner glow/rim light, no hard edges; springy squash-and-stretch *reads* as soft flesh (see §3). |
| 7 | Round, protruding chubby cheeks | Add **cheek roundness** — a slight outward bulge below the eyes; optional pink cheek blush dots (a cuteness cheat code). |
| 8 | (added later) Clumsy movement | Slight wobble/overshoot in motion, tiny stumbles — *cute incompetence.* |

### The concrete proportion cheat-sheet (design the chick to these)

- **Head : body height = ~1 : 1** for maximum baby (a chick is a big head with a tiny body nub). If you want a bit more "creature," go 1 : 0.7.
- **Eyes:** two big circles, **each ~25% of head width**, centers at **~58% down** the head and separated so there's roughly one eye-width of gap between them (wide-set = younger/cuter). Put a **large glossy catchlight** (white highlight) at the top-inner of each eye — catchlights are the strongest single "alive + cute" signal.
- **Pupils large, iris dark, sclera minimal.** Big dark eyes > detailed eyes.
- **Nose/beak/mouth: small.** Beak = a small triangle centered just below/between the eyes, **width ≈ 12–18% of head width.**
- **Body:** an egg/superellipse, **wider at the bottom** (low center of gravity = stable, baby-like).
- **Palette:** warm, high-value, low-saturation-shadow. Chick yellow `#FFD23F`-ish body, warm orange beak/feet, soft brown-not-black eyes. Avoid pure black outlines — use a **dark tint of the body color** for outlines (softer, cuter).

> **Important nuance from the research:** *"eye size alone is poorly correlated with cuteness"* — the features work **holistically**. Don't just inflate the eyes; get the **head ratio + low eye placement + roundness + small mouth** all together, or you land in uncanny-bobblehead territory ([PMC11285920](https://pmc.ncbi.nlm.nih.gov/articles/PMC11285920/); [Gaussian-process study, PMC9669709](https://www.ncbi.nlm.nih.gov/pmc/articles/PMC9669709/)).

---

## 3. The MOTION Language of Cuteness ("Juice")

A cute *drawing* is 40% of it; a cute *character* is the motion. The rule: **living things are never perfectly still, and cute living things move with springy softness and a little clumsiness.** The craft here is Disney's 12 principles ([NYFA](https://www.nyfa.edu/student-resources/12-principles-of-animation/); [GameJuice — 12 principles for games](https://gamejuice.co.uk/articles/disney-12-animation-principles-games)) applied with game-feel "juice" ([Vlambeer/Nijman "Art of Screenshake"](https://www.gamedesign.gg/knowledge-base/game-design/game-feel-feedback/the-art-of-screenshake-jan-willem-nijman-vlambeer/); [Jonasson & Purho "Juice it or Lose it"](https://gamejuice.co.uk/resources/juice-it-or-lose-it)).

### 3a. Squash & Stretch — the #1 principle
The first Disney principle: flatten on impact, stretch on the rebound, **preserving volume** (as it widens, it shortens). *"The amount of squash and stretch encodes material — more exaggeration = rubbery/soft, less = rigid."* For a soft baby creature, **exaggerate**. Concretely:
- **Idle breath:** scale the body between `scaleY 1.0 ↔ 1.03` and `scaleX 1.0 ↔ 0.99` (volume-preserving), on a slow sine.
- **Happy bounce / landing:** on landing, squash to `scaleY 0.8 / scaleX 1.2` for ~80ms, then spring back through an **overshoot** to `1.1 / 0.92` before settling at `1.0`.
- **Anchor the squash to the feet** (transform-origin at the bottom), never the center — it looks like it has weight.

### 3b. Anticipation & Overshoot (springs, not linear)
Every action gets a tiny wind-up the opposite direction first (anticipation), and every stop overshoots and settles (follow-through). **Never use linear or plain ease** — use **spring physics.** Springs are defined by **mass, stiffness, damping**; lower damping = more bounce/overshoot ([Figma — building spring animations](https://www.figma.com/blog/how-we-built-spring-animations/); [Apple WWDC23 — Animate with springs](https://developer.apple.com/videos/play/wwdc2023/10158/); [kvin.me — two-parameter springs](https://www.kvin.me/posts/effortless-ui-spring-animations)).
- Good starting cute-bounce spring: **stiffness ~180, damping ~12, mass 1** (visibly overshoots ~1–2 times then settles) — tune damping *down* for more bounce.
- In pure CSS you can bake a spring into `animation-timing-function: linear(...)` ([Carmen Ansio — spring physics in CSS](https://www.carmenansio.com/articles/spring-physics-css/)); in Canvas, integrate a tiny spring each frame (`v += (target - x) * k; v *= damping; x += v`).

### 3c. Idle micro-motions (the "it's alive" layer — runs 100% of the time)
A still character reads as *"a frozen statue, not a living character"* ([AnimSchool](https://blog.animschool.edu/2024/06/14/breathing-life-into-idle-animations/); [MoCap Online idle guide](https://mocaponline.com/blogs/mocap-news/idle-animation-game-dev-guide)). Layer these, each on its own clock so they never sync up (async = organic):
- **Breathing:** relaxed ≈ **15–20 breaths/min** (alert 20–25); motion is **tiny, ~1–2 cm equivalent** of vertical travel — subtle. A slow `scaleY` sine at ~0.25–0.33 Hz.
- **Blink:** humans blink irregularly. Use a **randomized 2–6 s interval**, blink duration ~100–150ms, occasional **double-blink** for charm. Blink = squash the eyes to a thin line and back (2–3 frames). Blinking is the cheapest, highest-impact life signal.
- **Weight shift / sway:** a slow **4–8 s** side-to-side lean.
- **Look-around:** every several seconds, pupils dart to a new point and hold (saccade), occasionally the whole head follows. Bonus: **make the eyes track the mouse cursor** (oneko-style) — instant "it's paying attention to *me*."
- **Probabilistic fidgets:** borrow clippy.js — each idle cycle, roll a die: small chance to yawn, wiggle, tilt head, ruffle wings. Prevents the loop from ever feeling mechanical.
- **Idle frame rate can be low** — 4–8 fps / 6–8 poses reads as "relaxed" and saves CPU; reserve 60fps for reactive springs.

### 3d. Reactive expressions
Map states to whole-body *poses + face*, not just faces:
- **Happy bounce:** hop with squash-land, eyes become upward arcs (^‿^), maybe a little "!" or sparkle.
- **Surprise:** fast anticipation dip → pop up +stretch, eyes go wide (pupils shrink), brief hold.
- **Sleepy:** eyelids droop to half, slow breath, occasional head-nod-and-catch, a floating "z".
- **Listening/attentive:** perk up — ears/antenna/wing-tips raise, spine straightens, eyes widen a touch.

### 3e. Secondary motion (the jiggle that sells softness)
Appendages **lag and overshoot** the main body (Disney "follow-through & overlapping action"). Ears, tail, antenna, wing-tips, a cheek jiggle, a little tuft of head-feathers — each is a **1-DOF spring driven by the parent body's velocity.** This is cheap (one spring integration each) and is the difference between "sticker" and "creature."

### 3f. Game-feel "juice" (Vlambeer / Jonasson-Purho) applied to a 120px pet
From "Juice it or Lose it" and "The Art of Screenshake," the transferable tricks:
- **More animation than seems necessary** — every state change gets a little tween, nothing snaps instantly.
- **Tween/ease everything** (position, scale, color) — no hard cuts.
- **Overshoot on pop-in.** When the pet appears, it scales up past 1.0 and springs back.
- **A tiny bit of shake/kick** on strong events (e.g. a confident wiggle when a phrase lands) — subtle at this size; a 2–4px shake, decaying fast.
- **Particles** — a few sparkles/hearts/musical-notes on success. Even 3–5 tiny dots with gravity + fade add huge delight.
- **Sound + variation** — soft, pitch-randomized "yap/chirp" blips (like Pou's gibberish / Animal Crossing "animalese") so it never sounds identical. Optional/muted by default for a dictation tool, but a strong charm lever.
- **Juice is cumulative** — none of these alone matters; stacked, they transform feel.

---

## 4. Pixel Art vs Soft-Vector for a Hand-Coded Cute Pet at ~120px

**Verdict: soft-vector (SVG/Canvas paths) is the better fit for Yap.** Reasoning and how to do each well:

### The tradeoff
| | **Pixel art** | **Soft-vector (paths/SVG/Canvas)** |
|---|---|---|
| Cuteness feel | Nostalgic, handmade, "toy" charm (Dinoki, Tamagotchi) | Round, soft, "plush/baby-fat" charm (Finch, Pou) |
| At ~120px on Retina | Must design at native px and integer-scale (e.g. 40×40 → ×3) or it blurs/shimmers; **facial expression in few pixels is hard** — *"32×32 or smaller, expressive faces are extremely difficult"* | **Crisp at any DPI**, buttery squash-and-stretch, easy expression morphs |
| Animation cost | Every pose = a hand-drawn frame (frame budget explodes for smooth springs) | **Procedural** — one rig, transforms/springs generate infinite in-betweens; 60fps springs are ~free |
| Hand-coded difficulty | Draw a sprite sheet + index frames (like oneko: 32×32 cells, `background-position`) | Draw ~1 layered vector rig, animate transforms; more math, far less art labor |
| CPU | Very low (blit a frame) | Low if you avoid re-rasterizing paths every frame (cache to offscreen canvas; animate via transforms) |

([Comfy Pixel — pixel vs vector for brand](https://www.comfypixel.com/custom-pixel-art-vs-vector-art-which-style-is-right-for-your-brand/); [Deepworld — raster/vector/pixel pros & cons](https://www.tumblr.com/deepworldgame/52235728843/game-art-tips-raster-vs-vector-vs-pixel-art))

### Why vector wins *for this specific pet*
The whole cuteness thesis in §2–§3 is **round soft forms + springy squash-and-stretch + expressive mouth/eyes.** Vector is purpose-built for exactly that: continuous scaling, volume-preserving squash, morphing a beak open/closed, eyes that blink and track — all procedural, all 60fps, all one small rig. Pixel art would force you to pre-draw dozens of frames to get the same springiness and would fight you on facial nuance at 120px.

### How to do vector well (recommended path)
- Build the pet as **stacked shapes**: body (superellipse), belly highlight, two eye-whites, two pupils+catchlights, beak (two triangles: upper/lower for open/close), cheeks (blush), wing-tips, feet, head-tuft.
- **Group into a rig** with named transform nodes (body, head, eyeL, eyeR, pupilL/R, beakUpper/Lower, wingL/R, tuft). Animate the *nodes*, not the pixels.
- **Cache the static base** to an offscreen canvas; each frame only re-composite with cheap transforms + draw the few moving parts. This keeps CPU tiny.
- Use `devicePixelRatio` scaling so it's razor-sharp on Retina.
- Outlines: a **dark tint of the fill**, ~2px, rounded joins — never harsh black.

### If you go pixel (keep as an alternate skin)
- Design at a small native grid (e.g. **48×48**) and **integer-scale** only (48→96→144) with `image-rendering: pixelated` — never fractional scale (shimmer).
- Follow oneko/Tamagotchi economy: **the silhouette + eyes carry the cuteness**; 2–3 frames per state is plenty. Keep a shared sprite sheet, index by `background-position` like oneko's 32×32 cells.
- Pixel art shines if you later want the **Dinoki "retro toy" brand.** The Shimeji/VPet skin architecture (§1) means you can support *both* a vector chick and a pixel chick on the same rig-driven state machine.

---

## 5. How a Cute Pet Signals Dictation States

Each app state should read **at a glance from across the room** via silhouette + pose + eyes — the way a Siri orb tightens/brightens to show listening ([SmoothUI — Siri Orb](https://smoothui.dev/docs/components/siri-orb); [ElevenLabs UI — Orb states](https://ui.elevenlabs.io/docs/components/orb)). But instead of an abstract orb, we do it with **character acting.** Core principle from §1: **near-invisible when idle, expressive only during capture.**

| State | Trigger | Pose / silhouette | Face & eyes | Motion | Extras |
|---|---|---|---|---|---|
| **Idle / peek** | Not dictating | Small, settled, low; maybe peeking from a screen edge or half-tucked | Calm, occasional blink; eyes may **track the cursor** | Slow breathing (~16 bpm), rare probabilistic fidget/yawn; low fps | Dimmed; minimal footprint. This is 95% of the time — it must never nag. |
| **Wake / summoned** | Hotkey pressed | **Pop-in with overshoot spring**, perks upright | Eyes snap open wide, look toward you | Bouncy scale-in, a happy little hop | Optional soft "chirp" |
| **Listening** | Mic hot, capturing | **Perk up:** spine straight, wing-tips/tuft raised, leaning in attentively | Eyes wide & bright, pupils dilated | **Beak opens/closes driven by live mic amplitude** (louder = wider) — the "yapping" core; subtle body pulse to voice | A soft ring/glow behind it that scales with volume (Bongo-Cat-style input→pose mapping) |
| **Thinking / processing** | Transcribing/AI step | Slight lean-back, cute *impatience* | Eyes glance up-and-around (thinking), maybe one raised brow-tuft | **Looping "hmm" fidget** — tap a foot, sway, tiny head-tilts; a rotating dot/"…" bubble | Keep it *charming impatience*, not a boring spinner. Never freeze (frozen = broken). |
| **Done / success** | Text inserted | **Happy bounce** — hop with squash-land | Eyes become upward arcs (^‿^), cheeks blush | Overshoot spring + **3–5 sparkle/note particles** | Optional **speech bubble** echoing a word, or a tiny "✓" |
| **Error / didn't catch** | No speech / fail | Small shrink/tilt, apologetic | Eyes go to worried/wobble, half-blink | Gentle head-shake (2–3px decaying shake), one confused "?" | Soft, never alarming — cute confusion, then back to idle |
| **Sleepy** | Long inactivity | Sinks lower, eyes droop | Half-lids, slow nod-and-catch | Very slow breath, floating "z" | Reinforces "resting, not gone." Wakes instantly on hotkey. |

**Design rule:** the **beak** is the transcription-state workhorse. Its openness = mic amplitude in real time; that single reactive channel makes the pet feel genuinely *connected* to your voice — the same trick that makes Bongo Cat delightful, mapped to audio instead of keystrokes.

---

## 6. Concrete Character Concepts for Yap

"Yap" = yapping = talking → the winning creature needs an **expressive, obvious mouth.** Ranked on three axes: **Adorable** (baby-schema potential), **On-brand** (says "talking/yapping"), **Easy to hand-animate well** at 120px vector.

| Rank | Concept | Adorable | On-brand (yapping) | Easy to animate | Notes |
|---|---|---|---|---|---|
| **1** | **Baby chick 🐥** | ★★★★★ | ★★★★★ | ★★★★★ | Two circles + a triangle beak. Head-is-the-body proportion = maximally baby. **The beak is the best possible "yap" mouth** — opens/closes cleanly, reads at any size, maps perfectly to mic amplitude. Yellow = friendly, cheerful, unmistakable. Chirp = literal yapping. **Winner on all three axes.** |
| **2** | **Round blob / slime** | ★★★★☆ | ★★★☆☆ | ★★★★★ | Pou-style silhouette — the single easiest shape to squash-and-stretch; on-trend, gender/species-neutral, infinitely skinnable. **Weakness:** no beak → the mouth is just a shape on a blob, a softer "talking" signal. Excellent **fallback / alternate skin**, and easiest to build first as a prototype rig. |
| **3** | **Puppy 🐶** | ★★★★★ | ★★★★☆ | ★★★☆☆ | Peak mass-appeal cuteness (Nintendogs), and "yap" literally describes a small dog's bark — strong pun. **But:** ears, snout, tongue, legs, tail = more moving parts, and dogs look uncanny fast if proportions slip. Higher craft cost for the same charm the chick gets cheaply. |
| **4** | **Cat 🐱** | ★★★★☆ | ★★★☆☆ | ★★★★☆ | Proven desktop-pet default (oneko, Bongo Cat) — cats *own* this genre. Easy-ish (oneko is 32×32). **But** cats read as *aloof/quiet*, the opposite of "yap," and the space is saturated. Charming, off-brand. |
| **5** | **Ghost 👻** | ★★★★☆ | ★★★☆☆ | ★★★★★ | Trivially easy (a wavy blob, float idle, no feet/limbs, no ground physics), very cute, trendy. **But** off-brand for voice (ghosts are silent/spooky) and less warm than a chick. Great low-effort alternate skin. |

**Why the chick wins, concretely:**
- **Fewest shapes for maximum cuteness:** head+body as one egg, two big low eyes, triangle beak, stub wings, two feet, a head-tuft for secondary jiggle. That's a rig you can hand-code in a day.
- **The beak is a built-in mouth** — no other concept gives you such a clean, legible, amplitude-drivable talking signal. It *is* the product name made visual.
- **Baby-schema native:** chicks are already almost-all-head with huge eyes; you barely have to exaggerate.
- **Warm, cheerful, unthreatening** — right emotional register for a helpful daily tool, and distinct from the cat-saturated desktop-pet field.
- **Precedent:** Finch already proved a soft-vector baby *bird* is beloved as a companion — Yap can own the "chick" niche the same way.

---

## 7. Final Ranked Recommendation

### The single cutest, most-buildable Yap companion
**A soft-vector baby chick 🐥** with:
- **Proportions:** head:body ≈ 1:1 (basically one egg), two glossy eyes each ~25% of head width placed ~58% down the face (below center), wide-set with big catchlights; a small orange triangle **beak** (~15% head width) that opens/closes; optional blush cheeks; stubby wings; two little feet; a 1–3 feather **head-tuft** for secondary jiggle. Warm yellow body, soft dark-tint outline, no harsh black.
- **Motion spine:** permanent idle layer (async breathing ~16 bpm + randomized 2–6 s blinks + slow sway + cursor-tracking eyes + probabilistic fidgets); everything else driven by **springs** (stiffness ~180 / damping ~12, tune for bounce) with volume-preserving **squash-and-stretch** anchored at the feet; secondary springs on tuft/wing-tips; a dash of Vlambeer juice (overshoot pop-in, sparkle particles + soft pitch-varied chirp on success).
- **State acting:** idle-peek → wake pop → **listening (beak opens with mic amplitude)** → thinking (cute impatient fidget, never a frozen spinner) → **done (happy bounce + sparkles + optional speech bubble)** → sleepy on long idle.
- **Architecture:** build it Shimeji/VPet-style — **one rig + state machine, swappable skins.** Ship the chick as default; keep **blob/slime** and **ghost** as easy alternate skins, and leave the door open for a **pixel-art chick** skin later if you want Dinoki's retro-toy brand.
- **Behavioral north star (from the whole genre):** *near-invisible and calm when idle, delightfully expressive only during capture.* That restraint is what separates a beloved companion from an intrusive Clippy.

### Top 5 reference links whose charm to match
1. **Dinoki** — the target vibe: a tiny (6 MB), native, private, *evolving* pixel pet that feels premium, not gimmicky. → https://dinoki.ai/ (build story: https://news.ycombinator.com/item?id=44798849)
2. **Finch: Self-Care Pet** — proof a **soft-vector baby bird** is deeply beloved as a gentle, non-nagging companion; match its warmth and "the bird design is perfect" polish. → https://apps.apple.com/us/app/finch-self-care-pet/id1528595748
3. **Pou** — the round-blob silhouette, **gradual expression changes**, and **gibberish "voice"** — the emotional-consistency + "yapping" cues to emulate. → https://pou.en.softonic.com/android
4. **oneko.js** — the cheapest, cleanest reference for a **hand-coded reactive pet**: 32×32 rig, ~100ms tick, cursor-tracking state machine. Steal the reactivity + implementation economy. → https://github.com/adryd325/oneko.js/
5. **Tamagotchi** — the **economy-of-cuteness** north star: if a few dots on a 32×16 LCD can make people *care*, our chick must be irresistible. → https://www.japanhousela.com/articles/how-tamagotchi-changed-digital-design-icon-japanese-tiny-toys-30th-anniversary-bandai/

**Motion craft to keep on the desk while building:** Disney's 12 principles ([NYFA](https://www.nyfa.edu/student-resources/12-principles-of-animation/)), Vlambeer's "Art of Screenshake" ([gamedesign.gg](https://www.gamedesign.gg/knowledge-base/game-design/game-feel-feedback/the-art-of-screenshake-jan-willem-nijman-vlambeer/)), "Juice it or Lose it" ([GameJuice](https://gamejuice.co.uk/resources/juice-it-or-lose-it)), and spring-animation fundamentals ([Figma](https://www.figma.com/blog/how-we-built-spring-animations/), [Apple WWDC23](https://developer.apple.com/videos/play/wwdc2023/10158/)). Cuteness science backing every proportion: [Glocker et al. 2009](https://pubmed.ncbi.nlm.nih.gov/22267884/) and the [baby-schema review](https://pmc.ncbi.nlm.nih.gov/articles/PMC11285920/).

---

### Appendix — full source list
- Dinoki: https://dinoki.ai/ · https://news.ycombinator.com/item?id=44798849 · https://www.indiehackers.com/post/building-dinoki-native-ai-companion-for-mac-windows-4228efd94a
- Finch: https://apps.apple.com/us/app/finch-self-care-pet/id1528595748 · https://mwm.ai/apps/finch-self-care-pet/1528595748
- oneko.js / neko: https://github.com/adryd325/oneko.js/ · https://github.com/rinvii/neko · https://en.wikipedia.org/wiki/Neko_(software)
- Shimeji-ee: https://github.com/DalekCraft2/Shimeji-Desktop · https://github.com/gil/shimeji-ee
- Bongo Cat: https://github.com/shouwangzhe134/bongo-cat · https://github.com/Gamma-Software/BongoCat-mac
- clippy.js: https://github.com/clippyjs/clippy.js · https://github.com/pi0/clippyjs
- Pou: https://pou.en.softonic.com/android
- Tamagotchi: https://www.japanhousela.com/articles/how-tamagotchi-changed-digital-design-icon-japanese-tiny-toys-30th-anniversary-bandai/ · https://tamagotchi.fandom.com/wiki/Tamagotchi_(1996_Pet)
- Divoom/M5Stack pets: https://github.com/marceld23/Pixel-Pets · https://github.com/topics/pixoo64 · https://github.com/erodozer/vpet
- VPet: https://github.com/LorisYounger/VPet
- Desktop Mate: https://store.steampowered.com/app/3301060/Desktop_Mate/ · https://store.steampowered.com/app/3625270/MateEngine/
- desktop-pet genre: https://github.com/topics/desktop-pet
- Charming-vs-annoying: https://windowsforum.com/threads/clippy-lessons-for-microsoft-copilot-when-assistants-become-intrusive.411922/ · https://www.nathalielawhead.com/candybox/about-desktop-pets-virtual-companions-discussing-the-inhabitants-that-fill-the-void-of-our-digital-spaces
- Baby schema / cuteness science: https://pubmed.ncbi.nlm.nih.gov/22267884/ · https://pmc.ncbi.nlm.nih.gov/articles/PMC11285920/ · https://www.ncbi.nlm.nih.gov/pmc/articles/PMC9669709/ · https://phys.org/news/2015-06-infant-animals-cute.html
- Motion/juice: https://www.nyfa.edu/student-resources/12-principles-of-animation/ · https://gamejuice.co.uk/articles/disney-12-animation-principles-games · https://www.gamedesign.gg/knowledge-base/game-design/game-feel-feedback/the-art-of-screenshake-jan-willem-nijman-vlambeer/ · https://gamejuice.co.uk/resources/juice-it-or-lose-it
- Springs: https://www.figma.com/blog/how-we-built-spring-animations/ · https://developer.apple.com/videos/play/wwdc2023/10158/ · https://www.kvin.me/posts/effortless-ui-spring-animations · https://www.carmenansio.com/articles/spring-physics-css/
- Idle/blink/breathing: https://blog.animschool.edu/2024/06/14/breathing-life-into-idle-animations/ · https://mocaponline.com/blogs/mocap-news/idle-animation-game-dev-guide
- Voice-state visual language: https://smoothui.dev/docs/components/siri-orb · https://ui.elevenlabs.io/docs/components/orb
- Pixel vs vector: https://www.comfypixel.com/custom-pixel-art-vs-vector-art-which-style-is-right-for-your-brand/ · https://www.tumblr.com/deepworldgame/52235728843/game-art-tips-raster-vs-vector-vs-pixel-art
</content>
</invoke>

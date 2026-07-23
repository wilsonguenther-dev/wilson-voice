# Companion & Pill Inspiration — design + UX research for "Yap"

> **What this is:** the *design / feel* dossier for Yap's new "yap companion" — an
> origami creature that folds into a small always-on-top HUD, yaps in sync with your
> voice, then talks back with word-count-reactive personality (rude / friendly / rose +
> curse filter). **The old plain obsidian capsule is SCRAPPED.** This file is the
> aesthetic & UX counterpart to the engineering dossier in
> [`origami-yapping-pill.md`](./origami-yapping-pill.md) (which covers *how* to build the
> fold cheaply: bake keyframes with Amanda Ghassaei's MIT Origami Simulator, play them
> back in Canvas2D at 60fps/low-CPU). Read that one for licenses & runtime; read this one
> for **what it should look like and why people will love it.**
>
> **Design constraints carried over:** Tauri 2 transparent always-on-top **NSPanel**
> (~340×120 pt window), **60fps at near-zero idle CPU**, **no CDN** (everything bundled),
> **permissive licenses only** (GPL is fatal at runtime). Every direction below is judged
> against those. All links are real and were verified during this research pass
> (2026-07-22).

---

## TL;DR — the recommendation up front

Build **"Kami," a folded paper companion that lives at the notch and *peeks* before it
fully unfolds.** At rest it's a small folded-paper object tucked under the MacBook notch
(or bottom-center). Trigger dictation → it **unfolds a beat** into a little paper crane/fox
whose **beak is the waveform mouth**; it yaps in sync with your amplitude, "chews" while
transcribing, does a satisfied settle on paste, then **folds back the exact same way**
(reverse of the same baked keyframes). Word-count reaction text lives in a **speech bubble
that springs out of the notch** Dynamic-Island-style. This wins because it's the only
direction that is simultaneously *novel* (nobody ships an origami dictation pet),
*on-brand* (folding = the literal "unfold your voice into text" metaphor), *technically
cheap* (baked Canvas2D frames, no live WebGL), *shareable* (a folding paper creature is a
GIF-native hero), and *developer-friendly* (mascots are just swappable JSON crease packs).

Ranked directions and the 5 reference links to copy the feel from are at the very bottom.

---

## 1. Best-in-class ambient / companion HUDs — what makes them feel premium

The bar Yap is measured against. For each: the **shape language** and the **motion
language** worth stealing.

### The two north stars

**Apple Dynamic Island** — a black pill that *springs* between compact and expanded,
morphing its content with real spring physics (Hooke's-law stiffness + damping), never a
hard cut. The four principles worth internalizing: it (1) *fluidly morphs* shape to match
content, (2) *merges* separate activities into one object and splits back out, (3) keeps
motion *continuous* (no frozen frames), (4) uses *consistent* system springs so it always
feels like the same living object.
- 4 principles, dissected: <https://uxdesign.cc/apples-dynamic-island-animations-dissected-into-4-principles-b3bfdf546d9a>
- Apple's own spring model (WWDC23 "Animate with springs"): <https://developer.apple.com/videos/play/wwdc2023/10158/>
- Buildable references to study the morph: SmoothUI <https://smoothui.dev/docs/components/dynamic-island> · Aceternity <https://ui.aceternity.com/blocks/illustrations/dynamic-island> · a written teardown <https://cho.sh/w/9F7F85> · Figma auto-layout file <https://www.figma.com/community/file/1149679181793588525/apple-dynamic-island-animated-auto-layout-components>
- **Steal for Yap:** the pill *is* the state machine — its silhouette changes with what it's
  doing. Compact (idle) → wide (listening) → split (done + word-count bubble). Springs, not
  tweens.

**Apple Siri orb / glow** — the modern "listening" language. iOS 18/macOS = a colorful
**glow that traces the screen edge** (or the search field on Mac); iOS 27 collapses that
into a **swirling orb that expands out of the Dynamic Island**. The trick that reads as
premium: *continuous organic motion* (sine-summed waves / churning gradient) so there's
never a static "am I on?" moment.
- iOS 27 orb + pill morph: <https://www.macrumors.com/2026/06/16/iphone-18-could-make-siri-a-circle/>
- iOS 18 Siri animation, as a Figma file to dissect: <https://www.figma.com/community/file/1382288908082112753/ios-18-siri-animation>
- **Steal for Yap:** the companion should *breathe* at idle and *churn* while thinking —
  motion is the "I'm alive / I'm listening" signal, not a static icon.

### Direct competitors (dictation HUDs) — the pills to beat

- **superwhisper** — minimalist floating window; lives in the menu bar, springs in on ⌥Space,
  shows a real-time transcript in a clean floating panel. Calm, privacy-forward, *no
  personality* — which is exactly the gap Yap exploits. Changelog (good for tracking their HUD
  evolution): <https://superwhisper.com/changelog> · App Store: <https://apps.apple.com/us/app/superwhisper/id6471464415>
- **Wispr Flow** — the "Flow Bar" (desktop) / "Flow Bubble" (mobile) pill. Live waveform +
  streaming words, philosophy "**invisible until needed**." Notable *pain point* users hate:
  the pill is **stuck bottom-center and gets in the way** — a whole third-party utility
  (**PillFloat**) exists just to move it. **Lesson for Yap: make position a first-class
  setting from day one.**
  - Navigating the app: <https://docs.wisprflow.ai/articles/5096240724-navigating-the-wispr-flow-app-desktop-ios-and-android>
  - Android floating bubble UI: <https://hothardware.com/news/wispr-flow-ai-dictation-app-android>
  - PillFloat (the "your pill is in the way" tell): <https://github.com/OrangeAKA/pillfloat> · <https://www.producthunt.com/products/pillfloat>
- **Pindrop** (open source, watzon) — *the most direct precedent and a gift*: a WhisperKit
  menu-bar dictation app that ships **four floating indicator styles — Orb, Pill, Notch,
  Bubble — with preset themes.** Proof that "pick your HUD shape" is a shippable,
  developer-loved pattern Yap should adopt (and out-charm). <https://github.com/watzon/pindrop>
- **Vox** (open source) — "transparent capsule UI with a floating status indicator + pulse
  animation" in the menu bar. The capsule-with-pulse baseline Yap is explicitly leaving
  behind. <https://github.com/ZhangHanDong/vox>
- **VoiceInk** (open source, GPL) — floating recording indicator, on-device. Good UX
  reference, but **GPL — reference only, never link its code into Yap.** <https://tryvoiceink.com/> · <https://openalternative.co/voiceink>
- **Bolo** (Gemini menu-bar dictation): <https://github.com/sskokku/Bolo> · **Handy** (cross-platform, the stack Yap already studied): <https://github.com/cjpais/Handy>

### Ambient HUDs & menu-bar craft to borrow motion/shape from

- **Raycast** — sleek dark-chrome + gradient accents, a **Compact Mode** that expands on
  type, and *playful easter eggs* (Confetti / Bounce) that reward actions without nagging.
  The gold standard for "keyboard-first tool that's still fun." New UI writeup: <https://www.raycast.com/blog/the-new-raycast> · design-system teardown: <https://getdesign.md/raycast/design-md>
- **Arc — Little Arc & Mini Player** — a chromeless floating window with **no close/minimize
  buttons**, and a Mini Player you can **flick to any corner** with a trackpad gesture. The
  "toy you throw around your screen" feel Yap's companion should have. Mini Player: <https://resources.arc.net/hc/en-us/articles/19234766331799-Mini-Player-Watch-or-Listen-as-you-Browse> · Little Arc: <https://resources.arc.net/hc/en-us/articles/19235387524503-Little-Arc-Quick-Lookups-Instant-Triaging>
- **Rewind / Limitless** — the ambient always-recording aesthetic; a small, calm, ever-present
  presence rather than a window. <https://medium.com/seeds-for-the-future/limitless-ai-the-latest-ai-wearable-d114adb0f525>
- **Bezel** — floating toolbar that **auto-repositions** (jumps to the opposite edge when it
  would overlap) — a nice model for a companion that gets out of your way. <https://nonstrict.eu/bezel/>
- **Bartender / Ice / Thaw** — menu-bar managers; **Ice** (MIT, SwiftUI) recently adopted a
  **liquid-glass** look and custom menu-bar shapes. The "tucks in, near-zero CPU, native
  chrome" bar. Ice: <https://github.com/jordanbaird/Ice> · Thaw (macOS 26): <https://github.com/stonerl/Thaw>

### Notch-HUD apps — the *exact* real estate Yap wants (study these hardest)

Yap lives at/near the notch, so these are the closest spatial analogues:
- **boring.notch** (open source, TheBoredTeam) — turns the notch into a Dynamic-Island hub
  with a **music visualizer**, file shelf, HUD replacement. The reference implementation for
  "notch → living island" done permissively. <https://github.com/TheBoredTeam/boring.notch> · PH: <https://www.producthunt.com/products/theboringnotch>
- **Atoll** — open-source Dynamic Island for macOS: <https://github.com/Ebullioscopic/Atoll>
- **Notchy** — SwiftUI, "**idles at near-zero CPU (~0% on M2)**," notch → fluid island with
  HUDs. Proof the always-on-top-at-notch idea can be truly cheap. <https://notchy.dev/>
- **NotchNook / DynamicLake** — commercial polish references (HUD replacement, expand-on-hover): <https://alternativeto.net/software/notchnook/about>

**Distilled premium cues to bake into Yap:** spring physics (never linear tweens) ·
continuous idle motion (breathe, don't freeze) · silhouette-morph as the state signal ·
chromeless (no window buttons) · repositionable / flick-to-corner · easter-egg reward on
success · near-zero idle CPU · native-feeling glass, not a webview box.

---

## 2. Desktop pets / companions — the "little friend" feel

What makes a companion **charming (opt-in, non-blocking, personality)** vs **annoying
(interruptive, purposeless, always-on)**. This is the make-or-break axis for Yap.

### The classics (charm DNA + code to study)

- **oneko.js** (adryd325) — the cat that chases your cursor; **zero-dependency**, a legend of
  "tiny delightful web mascot." The whole charm is *ambient + non-blocking* — it never asks
  for anything. <https://github.com/adryd325/oneko.js> · topic hub: <https://github.com/topics/oneko> · cross-platform port: <https://github.com/eliot-akira/neko>
- **Shimeji-ee** — desktop mascot that wanders/climbs your windows; **actions defined in XML,
  art in swappable image sets** — the original "mascot pack" architecture Yap should copy for
  pet-swapping. Mac-compatible fork: <https://github.com/gil/shimeji-ee> · topic: <https://github.com/topics/shimeji-ee> · a modern Shimeji-style dev-tool pet: <https://github.com/xtrimsystems/claude-pet>
- **Clippy.js** (pi0) — Microsoft Agent in JS; each agent has its own animation sets + voice.
  Great *nostalgia + personality* library — and the perfect **cautionary tale** (see below).
  <https://github.com/pi0/clippyjs> · live: <https://clippy.pi0.io/> · modern TS rewrite: <https://github.com/vchaindz/modern-clippy> · Felix Rieseberg's "Clippy, now with local LLMs": <https://github.com/felixrieseberg/clippy>

### The Clippy line — why companions fail, and the rules that save them

Clippy is *the* reference for the failure mode Yap must avoid. It was hated because it was
**interruptive** (popped up unasked), **purposeless** (obvious/unhelpful), and **always-on
with no user control** — "a hammer that jumps into your hand every time you walk by."
- Lessons for modern assistants: <https://windowsforum.com/threads/clippy-lessons-for-microsoft-copilot-when-assistants-become-intrusive.411922/>
- Why we hate Clippy but *love Duo* (Duolingo) & Finch — mascots done right: <https://medium.com/internet-meet-human/mascots-in-product-design-4fe483e0d5e9>
- Microsoft's own answer (Mico): **role-scoped, opt-in, configurable, not omnipresent**: <https://windowsforum.com/threads/mico-microsofts-friendly-copilot-avatar-for-multimodal-ai.386125/>

**The charm rules Yap must obey:**
1. **Opt-in & scoped.** The companion only "speaks" (word-count reactions) *after you finish
   a dictation* — never mid-flow, never unprompted. It reacts to *your* action, it doesn't
   initiate.
2. **Non-blocking.** It never steals focus, never covers the caret, never modal. It's an
   ornament on a job you already asked for.
3. **Dismissible & tunable.** Personality dial (rude/friendly/rose), curse filter, "quiet
   mode," and an "off" that leaves a plain pill. Control = the antidote to Clippy.
4. **Purpose first, charm second.** The dictation must be flawless; the pet is the reward
   layer, never a tax on the task.

### Modern indie companions (2026) — the field Yap is entering

- **Finch: Self-Care Pet** — the masterclass in *delight without annoyance*: low-friction
  personalization, reactive micro-animations, confetti on completion, a pet whose personality
  is *revealed gradually*. Study its restraint. App: <https://apps.apple.com/us/app/finch-self-care-pet/id1528595748> · design critique: <https://ixd.prattsi.org/2026/02/design-critique-finch-self-care-pet-ios-app/>
- **Dinoki** — native (SwiftUI, **6MB, no Electron**), pixel-art "Tamagotchi + GPT" desktop
  companion that evolves; **no data collection, privacy-forward**. Closest spiritual sibling
  to Yap's "little friend on Mac." Build story: <https://www.indiehackers.com/post/building-dinoki-native-ai-companion-for-mac-windows-4228efd94a>
- **Masko / Masko Code** — a mascot that **watches your Claude Code / agents**; ships Clippy +
  community-added mascots. Proof devs adopt pets *when tied to their workflow*. <https://masko.ai/> · <https://www.producthunt.com/products/masko-code>
- **Tamamon** — starts as an **egg and hatches/evolves the more you code**. The "growth loop"
  Yap could mirror with word-count milestones. <https://www.producthunt.com/products/tamamon-a-tiny-desktop-pet-that-grows>
- **Mac Pet** — pixel pet that lives in the **menu bar or the notch**, Pomodoro-synced, "always
  visible but never in the way." Spatial precedent for Yap. <https://mac-pet.com/>
- **OpenPets** (MIT, open source) — animated desktop pets with **plugins + a pet gallery + AI
  coding-agent hooks; pick which animation plays per reaction (idle/working/waiting).** This
  is essentially the open-companion architecture Yap wants — a great one to align with or
  learn from. <https://openpets.dev/>
- **MiniCPM Desk Pet** — local-LLM desktop companion with **character adapters (swap
  personalities)**, reacts to Cursor/Claude/Codex activity. <https://firethering.com/minicpm-desk-pet-open-source-local-ai-desktop-pet/>
- **Desktop Mate / MateEngine** (Steam) — Live2D/VRM mascots that perch on and interact with
  your windows; **MateEngine is lightweight + low resource**. Reference for "character with
  presence" (though VRM/Live2D is heavier than Yap wants). Desktop Mate: <https://store.steampowered.com/app/3301060/Desktop_Mate/> · MateEngine: <https://store.steampowered.com/app/3625270/MateEngine/>
- **Bondee / Finch-adjacent** cozy companions — the "warm, collectible, personality-forward"
  category the design should feel adjacent to.
- Field map (competitive scan): <https://alternativeto.net/feature/desktop-mascot> (Dockitty,
  Typibara, PetPalBar, Dinoki, etc.).

**Takeaway:** the market has *cursor-chasers* (oneko), *window-perchers* (Shimeji/Desktop
Mate), and *coding-agent watchers* (Masko/Tamamon/MiniCPM). **Nobody owns "the pet that
turns your voice into text and yaps back."** That's Yap's open lane — and the winning ones
are all **native, low-CPU, opt-in, swappable, privacy-forward**.

---

## 3. Inspiration galleries + concrete references (origami · liquid-glass · blob · audio-reactive)

Where to keep sourcing, plus specific pieces to copy the feel from.

### Gallery homes (browse continuously)

- **Dribbble** — best for motion studies. Origami search & tags: <https://dribbble.com/search/origami> · <https://dribbble.com/tags/origami_animation> · a concrete folded-paper motion piece (Blender): <https://dribbble.com/shots/25067971-Folded-paper-animation-origami-in-Blender>
- **Awwwards** — curated "**resources for paper-folding animation effect**" (direct hit for
  the origami direction): <https://www.awwwards.com/stunning-resources-for-paper-folding-animation-effect.html>
- **Cosmos (cosmos.so)** — AI-curated visual moodboards; build boards for "origami," "liquid
  glass," "blob creature," "audio reactive." The best *taste-tuning* feed for this project. <https://www.cosmos.so/>
- **Mobbin** — real-product UI/flows (search superwhisper/Wispr/Raycast/Arc patterns): <https://mobbin.com/>
- **godly** — "astronomically good" sites/logos/app-icons; filter by 3D/motion for hero
  inspiration: <https://godly.website/> · <https://godly.design/>
- **Product Hunt** — companion/mascot launches (Masko, Tamamon, PillFloat, boring.notch above)
  — track the category's reception in real time.
- **Reddit scouting grounds** — r/MacApps (menu-bar/notch/dictation reception), r/SideProject
  & r/IndieHackers (launch reactions), r/creativecoding (blob/shader/audio-reactive
  techniques), r/origami (crease patterns & which models read as "cute creature"). Use these
  to sanity-check charm-vs-annoying *before* shipping, not after.

### Origami / paper motion (the hero direction)

- Awwwards paper-folding resources (above) + Dribbble origami motion (above).
- Codrops "**folding cardboard-box** on scroll" (three.js hinge technique — the parent/child
  crease rig; swap GSAP for WAAPI to stay permissive): <https://tympanus.net/codrops/2022/12/13/how-to-code-an-on-scroll-folding-3d-cardboard-box-animation-with-three-js-and-gsap/>
- (Build path for the *real* fold is in the engineering dossier: bake with Origami Simulator,
  play in Canvas2D.)

### Liquid-glass / droplet (Direction C)

- 2026 trend context (glassmorphism + liquid comeback): <https://medium.com/design-bootcamp/ui-design-trend-2026-2-glassmorphism-and-liquid-design-make-a-comeback-50edb60ca81e>
- **Droplet → toolbar morph** (exactly the "blob that becomes a bar" motion): <https://medium.com/@sarvjeet-singh/liquid-glass-ui-crafting-a-droplet-inspired-animation-that-transforms-into-a-toolbar-43070e0e46e1>
- Libraries: **liquidGL** (ultra-light WebGL glass) <https://github.com/naughtyduk/liquidGL> · **liquid-glass-js** (Apple-style, spring physics) <https://github.com/dashersw/liquid-glass-js> · **webgl-liquid-glass** (shader + backdrop-filter + springs) <https://github.com/clayharmon/webgl-liquid-glass> · CSS-only snippets: <https://freefrontend.com/css-liquid-glass/>
- **Caution:** live WebGL glass in an always-on panel fights the battery budget — prefer
  CSS `backdrop-filter` + a single `feDisplacementMap` for the refraction, reserve WebGL for
  a hover/expand moment, not idle.

### Metaball / blob creature with a mouth (Direction B)

- The cheap gooey technique (blur → contrast/`feColorMatrix` threshold = fused blobs) — *this
  is CPU-light and perfect for a pill*: CSS-Tricks gooey <https://css-tricks.com/gooey-effect/> · CSS-Tricks blobs <https://css-tricks.com/blobs/> · pattern writeup <https://animationpatterns.art/animations/gooey-blob-metaball-filter/> · Varun Vachhar's metaballs explainer <https://varun.ca/metaballs/> · topic hub <https://github.com/topics/metaballs>
- **Steal:** a metaball "mouth" = two blobs that separate on loud syllables and merge on
  quiet — an amplitude-reactive gooey mouth with *zero* 3D.

### Audio-reactive orbs / mouths (the "yapping" motion)

- **VoiceOrb** — "simple orb that reacts to voice," Perlin-noise vertex displacement (the
  archetype for Yap's listening state): <https://github.com/aguscruiz/voiceorb>
- Codrops audio-reactive shaders (three.js + Shader Park): <https://tympanus.net/codrops/2023/02/07/audio-reactive-shaders-with-three-js-and-shader-park/> · 3D audio visualizer walk-through: <https://tympanus.net/codrops/2025/06/18/coding-a-3d-audio-visualizer-with-three-js-gsap-web-audio-api/>
- Vertex-displacement/noise blob refs: Maxime Heckel (r3f shaders) <https://blog.maximeheckel.com/posts/the-study-of-shaders-with-react-three-fiber/> · Clicktorelease (GLSL noise displacement) <https://www.clicktorelease.com/blog/vertex-displacement-noise-3d-webgl-glsl-three-js/>
- Cheap 2D options that fit the pill budget: **react-voice-visualizer** (Web Audio, no WebGL) <https://github.com/YZarytskyi/react-voice-visualizer> · **siriwave** (MIT, sine-summed canvas, `setAmplitude()`) <https://github.com/kopiro/siriwave> · Dribbble/p5 blob refs for look: <https://dribbble.com/shots/10873769-3D-Javascript-Audio-Reactive-Blob> · <https://editor.p5js.org/nicoleannevella/sketches/lTybmn36Q>

### "Talks back" mouth (lip-sync for the personality reply)

- **Rhubarb Lip Sync** (6–9 Hanna-Barbera visemes from audio) — and a **WASM port** you can
  run locally to drive the mascot's mouth on the reply. Great for making "yapping back" read
  as *speech*, not just a bar. <https://github.com/DanielSWolf/rhubarb-lip-sync> · WASM: <https://github.com/danieloquelis/rhubarb-lip-sync-wasm>

### Peeking / edge reveal (Direction E)

- Peek UI motion study (Dribbble, Anaïs Maxin): <https://dribbble.com/shots/4227437-peek-UI-animation>
- Notch pull/peek interaction (Swift): <https://github.com/quickbirdstudios/FlippingNotch>

---

## 4. Concrete "new pill/companion" design directions (the old capsule is dead)

Six directions. Each: **resting form → fold↔unfold → state signaling (listening / thinking /
done) → where the yapping text lives → why it's delightful, shareable & dev-friendly.**
Feasibility is rated against the NSPanel / 60fps / no-CDN / permissive constraints.

---

### A. ★ "Kami" — folded-paper creature at the notch *(RECOMMENDED)*

- **Resting form:** a small **folded-paper object** (a closed crane/fox silhouette, ~28px)
  tucked into the underside of the notch — like a paper charm hanging off the notch. Idle =
  a slow paper "breathe" (a 1–2° hinge sway), matte paper texture with a soft drop shadow so
  it reads as a *physical* folded thing on glass.
- **Fold ↔ unfold:** on ⌥Space it **unfolds one beat** — not fully flat, just enough to
  become a recognizable little crane/fox with an open beak. Uses the **baked keyframe player**
  from the engineering dossier: unfold = `t:0→1`, fold-back = `t:1→0`, provably exact reverse,
  ~700ms eased with a spring. rAF only runs during the fold; idle draws one static frame → 0
  idle CPU.
- **State signaling:**
  - *listening* → unfolded creature, **beak = the waveform mouth** (amplitude-driven), paper
    tint warms slightly.
  - *thinking/transcribing* → the creature **"chews"** (tiny beak wobble + a paper shimmer
    sweep) so it's never frozen — the Siri "continuous motion" rule.
  - *done/paste* → a **satisfied bob**, a single fold-flap "nod," optional 3-particle paper
    confetti (Raycast-style reward).
  - then **folds back** to the resting charm.
- **Where the yapping text lives:** a **speech bubble springs out sideways from the notch**
  (Dynamic-Island split/expand) carrying the word-count reaction — "247 words. Not bad,
  chatterbox." Tone (rude/friendly/rose) + curse filter select the copy variant. Bubble
  auto-dismisses; never covers the caret.
- **Why it wins:** *novel* (an origami dictation pet doesn't exist), *on-brand* (folding =
  "unfold your voice into text"), *cheap* (Canvas2D baked frames, no WebGL), *shareable* (a
  folding paper crane is a perfect looping GIF/TikTok hero — and Wilson makes viral video),
  *dev-friendly* (each pet is just a `crease.json` pack — crane, fox, frog, dragon — swappable
  with zero code). **Feasibility: high.**

---

### B. "Goo" — metaball blob creature with a gooey mouth

- **Resting form:** a single soft **gooey droplet** (~30px) with two dot eyes; idle = a slow
  wobble via the SVG blur→threshold gooey filter (very cheap, no 3D).
- **Fold ↔ unfold:** it doesn't fold — it **splits**. On listen, the blob **splits into a
  head + a mouth-blob** that separate on loud syllables and merge on quiet (metaball mouth).
  "Fold back" = the two blobs re-merge into the resting droplet.
- **State signaling:** *listening* = amplitude-reactive gooey mouth; *thinking* = the blob
  slowly orbits a satellite droplet (churn); *done* = a bouncy squash-and-settle.
- **Yapping text:** rendered *inside* a blob that inflates from the creature (the droplet→pill
  morph), text set on the glass.
- **Why it's good:** extremely cheap (pure SVG filter, `feColorMatrix`), organic and alive,
  reads as "slime buddy." **Downside:** less *ownable/branded* than origami — blobs are
  everywhere. **Feasibility: very high.** Refs: CSS-Tricks gooey, animationpatterns,
  varun.ca/metaballs (§3).

---

### C. "Dewdrop" — liquid-glass droplet that morphs into the bar

- **Resting form:** a **liquid-glass droplet** (Apple "Liquid Glass" language) refracting the
  desktop behind it; idle = a faint caustic shimmer.
- **Fold ↔ unfold:** on listen it **stretches into a glass pill** with a live waveform inside
  the glass (droplet→toolbar morph, spring physics); on done it **beads back** into a drop.
- **State signaling:** color-temperature shift inside the glass (cool idle → warm listening →
  green done); refraction intensifies while thinking.
- **Yapping text:** etched inside the glass pill.
- **Why it's good:** premium, matches the 2026 liquid-glass trend, feels native to macOS
  Tahoe. **Downside:** it's a *material*, not a *creature* — the least "little friend" of the
  set; heavier if done in live WebGL. Best as a **theme option** layered over A or B rather
  than the hero. **Feasibility: medium** (CSS `backdrop-filter` + `feDisplacementMap` cheap;
  full WebGL glass costly). Refs: droplet→toolbar Medium, liquidGL, liquid-glass-js (§3).

---

### D. "Seed" — glowing orb/egg that cracks open

- **Resting form:** a small **glowing seed/orb** (Siri-orb energy) with a soft pulse.
- **Fold ↔ unfold:** on listen the shell **cracks and a tiny creature peeks out** (Tamamon
  "hatch" energy); heavier dictation = it opens more. On done it closes back to a seed.
- **State signaling:** orb glow = level meter; churning gradient = thinking; a bright bloom =
  done.
- **Yapping text:** a bubble from the cracked shell.
- **Growth hook:** the seed can **evolve with cumulative word count** (milestones unlock new
  creatures) — the Tamamon/Finch retention loop, opt-in.
- **Why it's good:** built-in progression + collectibility; the orb is the most universally
  "premium AI" shape. **Downside:** orb-as-listening-indicator is now common (Siri, every AI
  app) — needs the *creature reveal* to differentiate. **Feasibility: high.** Refs: VoiceOrb,
  Codrops audio shaders, Tamamon (§2–3).

---

### E. ★ "Peek" — a creature that never fully unfolds *(strong hybrid — fold this into A)*

- **Resting form:** just the **top of a paper creature's head + eyes peeking over the notch
  edge** — mostly hidden, adorably lurking.
- **Fold ↔ unfold:** on listen it **rises a little further out** (peek → half-body) but
  **never fully commits** — the restraint *is* the charm and it keeps the footprint tiny and
  non-blocking (the #1 Clippy fix). On done it sinks back to just-the-eyes.
- **State signaling:** eyes track the caret / blink while thinking; a little more of the body
  rises with amplitude; ears/beak twitch on done.
- **Yapping text:** speech bubble beside the peeking head.
- **Why it's great:** maximal charm-per-pixel, minimal intrusion, *inherently* respectful of
  focus, and extremely shareable ("there's a little guy living in my notch"). **This is less a
  separate direction than the ideal *default behavior* for A** — Kami peeks at rest, unfolds
  only a beat when you talk. **Feasibility: high.** Refs: peek UI motion, FlippingNotch (§3).

---

### F. "Concertina" — accordion paper strip → caterpillar

- **Resting form:** a **folded accordion/fan strip** (a fat "=" of pleats) sitting flat.
- **Fold ↔ unfold:** it **expands like a concertina** into a segmented paper caterpillar; each
  pleat is a hinge (pure CSS `rotateX` rig — cheapest possible "fold," no bake needed for v0).
  Fold back = collapse the pleats.
- **State signaling:** pleats ripple head-to-tail with amplitude (a paper equalizer); a wave
  travels the body while thinking; it "inches" happily on done.
- **Yapping text:** unrolls along the expanded strip like a paper receipt/ticker.
- **Why it's good:** the **easiest to prototype** (accordion CSS hinge, no Origami Simulator
  bake), and the pleat-ripple *is* a natural waveform. A great **v0 stand-in** to ship motion
  fast while the baked crane (A) is being produced. **Downside:** less iconic than a crane.
  **Feasibility: very high (cheapest).** Ref: paperfold CSS hinge (in the engineering dossier).

---

### Direction scorecard

| # | Direction | Novel/ownable | "Little friend" | Runtime cost | Shareability | Dev-friendly | Verdict |
|---|-----------|:---:|:---:|:---:|:---:|:---:|---|
| **A** | **Kami origami creature** | ★★★★★ | ★★★★★ | ★★★★ (baked C2D) | ★★★★★ | ★★★★★ | **Hero** |
| **E** | **Peek (never fully unfolds)** | ★★★★★ | ★★★★★ | ★★★★★ | ★★★★★ | ★★★★★ | **Default behavior for A** |
| B | Metaball goo blob | ★★★ | ★★★★ | ★★★★★ | ★★★★ | ★★★★ | Great cheap alt / theme |
| D | Glowing seed/orb (evolves) | ★★★ | ★★★★ | ★★★★ | ★★★★ | ★★★★ | Growth loop, add later |
| F | Concertina accordion | ★★★ | ★★★ | ★★★★★ | ★★★ | ★★★★★ | **v0 fast-ship stand-in** |
| C | Liquid-glass droplet | ★★★ | ★★ | ★★ (WebGL risk) | ★★★★ | ★★★ | Best as a *theme* over A/B |

---

## 5. What would make devs LOVE it (Wilson's goal)

"More developer-friendly, everybody enjoys the little yap companion." The winning
open-source companions above (OpenPets, Shimeji, Pindrop, MiniCPM, Masko) share a pattern —
copy it:

1. **Open, swappable pet packs.** Model Yap's mascots as **Shimeji-style asset packs** — a
   folder with a `crease.json` (or `.riv`) + `personality.toml` (idle/listen/think/done
   animations + tone copy). `git clone` a pet, drop it in `~/.yap/pets/`, restart. This is the
   single biggest "devs love it" lever: **a pet is data, not code.** (OpenPets & Shimeji prove
   the appetite.) <https://openpets.dev/> · <https://github.com/gil/shimeji-ee>
2. **Pick-your-HUD, like Pindrop.** Ship the same shape-picker Pindrop already validated —
   Orb / Pill / Notch / Peek / Blob — plus position (notch / bottom-center / flick-to-corner,
   fixing Wispr's #1 complaint). <https://github.com/watzon/pindrop>
3. **Themeable tokens.** A `theme.toml` of design tokens (paper color, accent, glass on/off,
   glow) + a Raycast-style theme gallery. Let people theme their pet like a terminal prompt.
4. **Keyboard-first, no wake word.** ⌥Space to dictate; the companion is *seen*, never *in the
   way*; everything reachable without touching the mouse. (Aligns with Yap's existing
   no-wake-word stance.)
5. **Personality as config, not hardcode.** The rude/friendly/rose dial, curse-filter toggle,
   word-count message tiers, and "quiet mode" all live in editable TOML/JSON — PR your own
   sass. Ship the message tables so the community writes reactions.
6. **Native, tiny, private.** Follow Dinoki's flex: **no Electron, single-digit MB, zero
   telemetry, on-device.** Devs adopt companions that don't phone home and don't hog CPU
   (Notchy's "~0% idle on M2" is the bar). <https://www.indiehackers.com/post/building-dinoki-native-ai-companion-for-mac-windows-4228efd94a> · <https://notchy.dev/>
7. **A "make your own pet" path.** Document the bake pipeline (Inkscape crease pattern →
   Origami Simulator → `crane.json`) so anyone can author a mascot from a real crease pattern.
   A pet contest = free marketing + a growing pack library (the boring.notch / Masko community
   flywheel). <https://www.producthunt.com/products/masko-code>
8. **Easter eggs + reactions API.** Let a pet react to *events* (build passed, PR merged, long
   dictation) the way Masko/MiniCPM watch coding agents — optional hooks so the pet becomes a
   dev-desk companion, not just a dictation widget. <https://firethering.com/minicpm-desk-pet-open-source-local-ai-desktop-pet/>

---

## Ranked recommendation

1. **★ Ship "Kami" (Direction A) with "Peek" (Direction E) as its default resting behavior.**
   A folded paper creature that lives at the notch, peeks at rest, **unfolds a beat** when you
   talk (beak = waveform), chews while transcribing, settles on paste, **folds back exactly**,
   and yaps its word-count reaction from a Dynamic-Island speech bubble. It's the only
   direction that is novel + on-brand + cheap (baked Canvas2D, no WebGL) + shareable (GIF-native
   paper fold) + dev-friendly (pets = swappable `crease.json` packs). Build it on the exact
   engine already speced in `origami-yapping-pill.md`.
2. **Ship "Concertina" (F) as the v0 stand-in** if the crane bake slips — a pure-CSS accordion
   caterpillar gets charming folding motion on screen this week with no bake pipeline.
3. **Offer "Goo" (B) and "Dewdrop-glass" (C) as alternate themes/skins**, not separate apps —
   satisfies the metaball and liquid-glass crowds via the same pet/HUD framework.
4. **Add "Seed→hatch" evolution (D) later** as the retention/collectibility loop (word-count
   milestones unlock new pets), opt-in.
5. **Bake in the anti-Clippy rules everywhere:** opt-in, non-blocking, dismissible, personality
   as config, never speaks mid-dictation. Charm is the reward layer; flawless dictation is the
   job.

## Top 5 reference links to copy the feel from

1. **Apple Dynamic Island — 4 animation principles** (silhouette-morph state machine + springs, the exact fold/unfold/speech-bubble grammar): <https://uxdesign.cc/apples-dynamic-island-animations-dissected-into-4-principles-b3bfdf546d9a>
2. **Pindrop** (open-source dictation HUD with Orb/Pill/Notch/Bubble picker — the shape-choice pattern to adopt and out-charm): <https://github.com/watzon/pindrop>
3. **Awwwards — paper-folding animation resources** (the origami/paper motion language for Kami): <https://www.awwwards.com/stunning-resources-for-paper-folding-animation-effect.html>
4. **VoiceOrb** (voice-reactive orb — the listening-mouth motion reference for the beak/waveform): <https://github.com/aguscruiz/voiceorb>
5. **Dinoki build story** (native, 6MB, no-Electron, privacy-forward "little friend on Mac" — the product & tech bar): <https://www.indiehackers.com/post/building-dinoki-native-ai-companion-for-mac-windows-4228efd94a>

*Bonus (charm-vs-annoying guardrail):* **Finch design critique** — delight without
intrusion, gradual personality reveal, reactive micro-animations done right:
<https://ixd.prattsi.org/2026/02/design-critique-finch-self-care-pet-ios-app/>

---
*Research pass 2026-07-22. All URLs verified live during this session via web search. This is
the design/feel companion to `origami-yapping-pill.md` (engineering/licensing). Sources span
the named channels: X/indie-hacker (Dinoki, PillFloat), Reddit/PH (boring.notch, Masko,
Tamamon), GitHub (oneko, Shimeji, Clippy, Pindrop, Vox, boring.notch, VoiceOrb, Rhubarb,
liquid-glass libs), Apple/WWDC, and design galleries (Dribbble, Awwwards, Cosmos, Mobbin,
godly).*

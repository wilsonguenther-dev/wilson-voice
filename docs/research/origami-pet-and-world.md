# Origami-Cute Pet + The Little World It Lives In — research dossier

> **App:** Yap (formerly Wilson Voice) — local, open-source macOS dictation app.
> **Stack constraints (carried from `origami-yapping-pill.md` and `cute-companion-design.md`):**
> Tauri 2 (Rust + macOS WKWebView), React 19 / TS / Vite. The companion renders in a
> transparent, always-on-top NSPanel (`desktop/float.html`), today ~340×120 logical pt.
> **Hard rules for anything adopted:** no runtime CDN (bundle everything locally),
> permissive license only (MIT/BSD/Apache/MPL — GPL is fatal), **60fps at low CPU**
> inside a tiny always-on transparent webview, and the character is **hand-coded**
> (Canvas2D/SVG/CSS), not a library drop-in.
>
> This file is written to stand alone. It answers two questions and ends with two
> concrete verdicts: **(A)** whether an origami creature can be genuinely cute, and
> **(B)** a buildable design for the "little world + wander/run + working-chatter."
>
> **Prior context this builds on (already in `docs/research/`):**
> the earlier *abstract origami crane* was rejected as **not cute**; a separate track
> recommended a **soft-vector baby chick** rendered in Canvas/SVG. This dossier
> reconciles those: it explains *why* the crane failed and whether an origami *style*
> (not the crane) can be salvaged into something adorable.

---

## PART A — Can an origami creature be genuinely CUTE?

### A0. TL;DR verdict (details below)

**Yes — but only if you abandon "realistic folded origami" and steal the *papercraft-toy* language instead.** The abstract crane failed for a specific, diagnosable reason: real origami is made of **sharp straight creases and hard angular planes**, which is the exact *opposite* of the baby-schema geometry (round, soft, plump) that the brain reads as cute. Every genuinely-cute "paper" character in games — Olivia in *Paper Mario: The Origami King*, Atoi in *Tearaway*, Kirby in *Epic Yarn*, Yoshi in *Crafted World* — cheats the same way: they keep the **paper *texture and lighting*** (fold shadows, drop shadows, matte grain, a hand-made tactility) while giving the character a **round silhouette, a big face, and two huge eyes**. Cuteness comes from the face and proportions; "paper" is a *skin*, not a *shape*.

So the single best approach is **not** "a folded creature." It is: **take the already-recommended round baby-chick (or blob) silhouette and re-skin it in papercraft — matte paper fill, a subtle fold crease or two catching light, a soft drop shadow — so it reads as "a little paper toy that came to life."** That is origami-*cute*, and it's cheap to draw in Canvas2D.

---

### A1. Why the abstract crane failed — the science of "cute"

Cuteness is not subjective taste; it's a measurable perceptual trigger. Konrad Lorenz's **Kindchenschema ("baby schema")** is the set of infantile features the brain reads as cute and that motivate caretaking: **a large head relative to the body, a big rounded cranium, large eyes below the facial midline, small nose/mouth, chubby cheeks, short thick limbs, a plump body, and a soft-elastic surface texture** ([Glocker et al., *Ethology* 2009](https://onlinelibrary.wiley.com/doi/abs/10.1111/j.1439-0310.2008.01603.x); [ScienceDirect overview](https://www.sciencedirect.com/topics/neuroscience/baby-schema)). fMRI work shows exaggerating baby-schema features literally lights up the **nucleus accumbens**, the brain's reward center ([Glocker et al., *PNAS* 2009](https://www.pnas.org/doi/10.1073/pnas.0811620106)).

Read that list back against a traditional origami crane: **long thin neck and beak, sharp wings, straight creases, angular planes, no face, no eyes, small head, elongated body.** It is an almost perfect *anti-baby-schema*. It was never going to be cute — not because "origami can't be cute," but because *that particular fold* violated every cuteness lever at once. Note the one Kindchenschema feature paper *does* naturally have: **"soft-elastic surface texture"** is on the list — matte paper grain reads as soft and tactile. That is the hook we keep.

**Design takeaway:** cuteness lives in *silhouette + face + proportions*. Paper can supply *texture and charm*, but it must never dictate a sharp, angular *shape*.

---

### A2. The proof set — adorable paper/craft characters and what they actually do

Every one of these is beloved and unmistakably "paper/craft," yet none is a realistic sharp fold. Study what they keep and what they throw away.

| Character / world | Medium | What makes it cute (the transferable trick) | Source |
|---|---|---|---|
| **Olivia — *Paper Mario: The Origami King*** | "Origami" figure | She *looks* like folded paper but is deliberately **not** foldable in real life — a **round crown-fold head, big brown eyes, small features, expressive child-like personality**. The origami is a costume over baby-schema proportions. | [MarioWiki](https://www.mariowiki.com/Olivia), [Wikipedia](https://en.wikipedia.org/wiki/Paper_Mario:_The_Origami_King), [Incredible Characters Wiki — Olivia](https://greatcharacters.miraheze.org/wiki/Olivia_(Paper_Mario)) |
| **Atoi / Iota — *Tearaway*** (Media Molecule) | Papercraft world | **Envelope head, spindly paper legs, floppy paper hair**; the head is a simple rounded envelope shape with two dot eyes. The world *behaves* like paper — it **crumples, folds, and tears** with authentic pop-up-book physics (they consulted a pop-up-book expert). Charm = **tactility + a simple round face**, not fold complexity. | [Wikipedia](https://en.wikipedia.org/wiki/Tearaway_(video_game)), [PlayStation Blog — 7 unseen concepts](https://blog.playstation.com/archive/2017/03/31/7-unseen-concepts-that-shaped-tearaway-media-molecules-brilliant-papercraft-adventure), [Discovering Game Design](https://discoveringgamedesign.wordpress.com/2015/02/17/tearaway-papercraft-come-to-life/) |
| **Kirby — *Kirby's Epic Yarn*** | Yarn/felt | Already a round pink blob; the **yarn texture** and felt-overlap backgrounds add handmade warmth without changing the cute silhouette. Craft skin over a cute shape. | [Nintendo Life retrospective](https://www.nintendolife.com/news/2020/10/soapbox_10_years_on_kirbys_epic_yarn_is_still_the_pink_puffballs_finest_outing), [Wikipedia](https://en.wikipedia.org/wiki/Kirby's_Epic_Yarn) |
| **Yoshi — *Yoshi's Crafted World*** | Cardboard/household craft | World built from **boxes, paper cups, felt** — "far more tactile" precisely because it's arts-and-crafts. Yoshi stays round; the *environment* carries the craft identity (huge relevance to Part B). | [CBR](https://www.cbr.com/kirbys-epic-yarn-creative-nintendo-wii-game/), [Nintendo UK](https://www.nintendo.com/en-gb/News/2019/January/Yoshi-s-Crafted-World-for-Nintendo-Switch-and-Kirby-s-Epic-Yarn-for-Nintendo-3DS-family-systems-both-launch-in-March-2019-1495499.html) |
| **Papercraft toy mascots** (Paper Foldables, Makey) | Real folded paper | Community papercraft that *is* cute keeps **rounded balanced proportions, big printed eyes, and a soft-cornered body**; "soft and rounded looks with nicely balanced proportions look clean once assembled." | [Paper Foldables](https://www.paperfoldables.com/), [Make: Posable Papercraft Makey Mascot](https://makezine.com/projects/posable-papercraft-makey-mascot/) |

**The pattern is unanimous:** cute paper characters = **round body + big face + two big eyes**, wearing **paper as a texture** (fold shadows, matte grain, drop shadow). The medium is a coat of paint on top of baby-schema geometry. Sharp realistic folds appear only in the *world/props*, never in the *creature's face*.

---

### A3. Feasibility — can we hand-code a *cute* origami pet in Canvas2D?

Two viable builds. Both are cheap and hit the 60fps/low-CPU/no-CDN/permissive bar because they're just our own draw code.

**Option 1 — "Paper-skinned cute creature" (RECOMMENDED).**
Take the already-blessed round chick/blob and render it with a papercraft *material*, not paper *geometry*:
- **Fill:** flat matte color + a faint 1–2% noise/paper-grain overlay (a tiny tiling PNG data-URI, or procedural speckle) so it reads as cardstock, not plastic.
- **Fold shading:** paint **one or two crease lines** across the body as thin gradient bands — a lighter facet on one side of the crease, a hair darker on the other. Two creases is enough to say "folded paper." This is pure Canvas gradient work, no simulation.
- **Drop shadow:** a soft offset ellipse beneath it. Games and paper toys both rely on this — the drop shadow is what makes paper feel like it's *sitting in a real space* and lifts it off the background.
- **Face:** unchanged from the chick spec — two huge glossy eyes low on the face, tiny triangular beak that opens/closes on "yap." The face does all the cuteness; the paper does the charm.
- **Motion:** springy **squash-and-stretch** on idle bob, hop, and beak-open. Squash-and-stretch is the animation principle that gives "fleshiness, flexibility and life" and is the single biggest cuteness multiplier for a simple shape — Pixar's *Luxo Jr.* creates full emotion from a faceless lamp using squash/stretch + anticipation + arcs alone ([Animation Mentor](https://www.animationmentor.com/blog/squash-and-stretch-the-12-basic-principles-of-animation/), [Animaker — 12 principles](https://www.animaker.com/hub/12-principles-of-animation/)). One paper-specific bonus: on a big reaction you can add a quick **paper "crinkle" flex** (a fast squash with a crease-line flicker) — the *Tearaway* crumple, in miniature.

Result reads as: *"an adorable little paper chick/toy that came to life."* Origami-cute, achieved.

**Option 2 — "Genuine simple fold, but chosen for cuteness."**
If Wilson specifically wants a creature that visibly *is* folded, do **not** use the crane. Use a fold whose natural silhouette is already round and face-forward — a **jumping paper frog**, a **paper "chick"/hen base**, or a **paper cat head** (a square folded to a rounded-triangle face with two ear points). Then bolt on the baby-schema fixes: **shrink limbs, enlarge the head, add two big eyes below the midline, round the hard corners**. This can work (Olivia proves a "fold" reads cute once it has a face), but it's strictly harder to keep cute than Option 1 and buys little — at 120px nobody perceives fold *accuracy*, only the *paper look* and the *face*. There's a proven offline pipeline for real fold geometry if ever wanted (bake Amanda Ghassaei's MIT **Origami Simulator** to JSON keyframes, per `origami-yapping-pill.md`), but that's the animation of the *unfold*, not what makes it cute.

**Would a "paper chick" or "paper cat" read as cute? Yes** — provided it's Option 1 geometry (round + big face) wearing paper texture. A *literal* folded crane will not. A paper-*skinned* chick will.

---

### A4. Part A verdict

- **Is origami-cute viable? YES**, with one hard reframe: **cute comes from a round silhouette + big face + big eyes + squishy motion; "paper" is a texture/lighting skin, never the shape.** The crane failed because it was sharp, angular, faceless, small-headed — the anti-cute geometry — not because paper can't be cute.
- **Single best approach:** **Option 1 — the round baby-chick (or blob) rendered as a papercraft toy:** matte paper-grain fill, one or two soft **fold-crease shadow lines**, a soft **drop shadow**, big glossy eyes, an opening beak, and springy **squash-and-stretch** (plus an optional paper "crinkle" on big reactions). It satisfies every stack constraint (it's just our own Canvas2D draw code) and it unifies the two prior recommendations: it *is* the cute chick, wearing origami as a costume.

---

## PART B — The little world it lives in (wander / run / working-chatter)

### B0. The reframe: drop the box, make the pill a *world*

The instinct is right. The genre lesson is that the **habitat is a co-star, not a frame.** *Yoshi's Crafted World* is more charming than a plain platformer precisely because the *environment* is handmade craft ([CBR](https://www.cbr.com/kirbys-epic-yarn-creative-nintendo-wii-game/)); *Neko Atsume* is beloved not for complex pets but for a **cozy, customizable little yard** the cats wander into and out of ([Wikipedia](https://en.wikipedia.org/wiki/Neko_Atsume), [Yard wiki](https://nekoatsume.fandom.com/wiki/Yard)). So the design goal: the pill/screen stops being a "Tamagotchi box with a pet inside" and becomes a **tiny lived-in diorama** — a slice of world with a floor, a couple of props, depth, and light — that the pet **wanders across when idle** and **runs to the front of when you dictate.**

Critically, the desktop-pet literature warns that an always-visible companion becomes *Clippy* — "too eager, too generic, too visible," interrupting ([existing `cute-companion-design.md`; Nathalie Lawhead on desktop pets](https://www.nathalielawhead.com/candybox/about-desktop-pets-virtual-companions-discussing-the-inhabitants-that-fill-the-void-of-our-digital-spaces)). The fix baked into this design: **idle = calm and low-key in its little world; active = it performs, but only while you're actually dictating.**

---

### B1. Habitat / diorama design — what to steal

| Reference | What it is | What to steal for Yap's world | Source |
|---|---|---|---|
| **Neko Atsume** | Cozy yard cats wander into | **The wander-in/wander-out rhythm**; a **small set of props** (bed, bowl, toy) that give the scene life and later become a customization hook; seasonal/weather background swaps. Cuteness from *coziness*, not busyness. | [Wikipedia](https://en.wikipedia.org/wiki/Neko_Atsume), [Yard wiki](https://nekoatsume.fandom.com/wiki/Yard) |
| **Yoshi's Crafted World** | World built of paper/cardboard | The **whole diorama is craft** — floor, props, backdrop all matte paper/cardboard. Perfectly on-theme with Part A's papercraft skin: the pet *and* its world share one material language. | [CBR](https://www.cbr.com/kirbys-epic-yarn-creative-nintendo-wii-game/) |
| **Tearaway** | Papercraft that folds/crumples | **Depth via layered paper cutouts** + **parallax**; props pop up like a pop-up book. A back layer (sky/wall), a mid layer (props), a front floor edge → instant diorama depth in 2D. | [Wikipedia](https://en.wikipedia.org/wiki/Tearaway_(video_game)), [PS Blog](https://blog.playstation.com/archive/2017/03/31/7-unseen-concepts-that-shaped-tearaway-media-molecules-brilliant-papercraft-adventure) |
| **oneko.js / neko** | Cursor-chasing cat | **The wander/chase state machine** (see B2) — the canonical, cheapest model for "idle-wander vs run-to-target." | [adryd325/oneko.js](https://github.com/adryd325/oneko.js/), [Neko (software) Wikipedia](https://en.wikipedia.org/wiki/Neko_(software)), [cyberciti writeup](https://www.cyberciti.biz/open-source/oneko-app-creates-cute-cat-chasing-around-your-mouse/) |
| **Shimeji / Desktop Mate** | Mascots that roam the OS | **Roaming vocabulary**: walk along an edge, perch on a ledge, sit, distinct **idle animations** so it "feels alive rather than static." We confine this roaming to the pill's own little world instead of the whole OS. | [shimejis.xyz](https://shimejis.xyz/), [Best Desktop Pets for Mac 2026](https://mac-pet.com/en/blog/best-desktop-pets-mac/), [Desktop Mate on Steam](https://store.steampowered.com/app/3301060/Desktop_Mate/) |
| **Bitzee** | Handheld reactive digital pet | Modern proof kids/adults bond with a **tiny reactive creature in a tiny window** that "reacts to touch, swipes, tilts" and grows over time — supports adding a light **evolution/attachment** layer later. | [Bitzee (Spin Master)](https://shop.spinmaster.com/products/bitzee-interactive-digital-toy-pet-6068705) |

**Concrete scene recipe (fits the ~340×120 pill; scale up if the window grows):**
- **3 parallax layers**, all matte papercraft: (1) **back** — a soft solid/gradient "wall" or sky with maybe a paper cloud/plant; (2) **mid** — 1–2 **props** (a little bowl, a plant, a cushion) as paper cutouts with drop shadows; (3) **front floor** — a thin paper "ground" strip the pet stands on, with a subtle front-edge shadow for depth. Parallax = layers shift a few px against each other as the pet moves.
- **Light/day-night (optional, cheap):** tint the back layer warm in the day, cool/dim at night by clock, and give the pet a matching soft rim. Low-cost coziness; a known virtual-pet charm lever (Neko Atsume's seasonal yard).
- **Depth without 3D:** everything is flat paper cutouts + drop shadows + parallax. This is the *Tearaway* trick and it's trivially cheap in Canvas2D.

---

### B2. Behavior — the wander ↔ run state machine

Model it directly on oneko's proven, tiny state machine (idle → wander → run-to-target → arrive → perform → return), driven by two inputs Yap already has: an **idle timer** and the **dictation-active signal** (plus mic amplitude, which the pill already paints as `--level`).

**States and transitions:**

1. **IDLE / WANDER (dictation off).** The pet lives its life in the diorama. Borrow oneko's **probabilistic idle** so it never loops identically — like Clippy's cat flipping a coin to decide whether to scratch or turn ([clippy.js probabilistic idle, per existing doc](https://github.com/clippyjs/clippy.js)) and oneko's "stare, scratch, yawn, sleep" cycle ([Neko Wikipedia](https://en.wikipedia.org/wiki/Neko_(software))). Every few seconds roll a weighted die: *stand & blink*, *hop*, *waddle to a random floor spot*, *sniff a prop*, *preen*, or (after long idle) *curl up / doze* near a prop. Movement is slow, calm, low-key — this is the "near-invisible when idle" mandate that keeps it from being Clippy. Wandering uses easing/arcs (never linear) so it reads alive.
2. **SUMMON → RUN TO FRONT (dictation just activated).** On the hotkey/dictation-start event, the pet **drops whatever idle it's doing (anticipation crouch), then runs to the front-center of the scene** — the oneko "run toward the target" behavior, retargeted from cursor to the front of its world. Run cycle = faster waddle + bigger squash-and-stretch + a little dust/paper-scuff puff. This *entrance* is the emotional payoff: it visibly **comes when called**.
3. **WORKING / LISTENING (dictation live).** Front and center, facing the user, **beak opening in time with mic amplitude** (Bongo-Cat-style input→pose mapping; the pill already streams `--level`). It **chatters status** (B3). Big eyes track the "action." This is the one moment it's allowed to fully perform.
4. **DELIVER / REACT (transcript pasted).** On paste/commit: a quick **celebratory reaction** — happy hop, sparkle, ears/wings up, a paper "crinkle" flex — and, per the reactive-line idea, a short spoken/te​xt beat ("there!" / "done!"). Optionally a tiny "carries the text to the front and drops it" flourish.
5. **RETURN → IDLE.** After a beat it **waddles back** into the diorama and resumes wandering. World returns to calm.

This is a ~5-state finite state machine with a random idle-behavior sub-roller — cheap, and exactly the architecture oneko/Shimeji use. Keep the *rig* (skeleton of squash/stretch + eye/beak params) separate from the *skin* (chick, blob, paper-cat) so characters are drop-in swaps, mirroring Shimeji/VPet's asset-pack lesson (from existing doc).

---

### B3. "Working-chatter" — what an assistant-pet says while it works

This is where the pet earns its keep. Two design forces:

**(a) The Labor Illusion — showing work *increases* perceived value.** Harvard's Ryan Buell showed that when a service visibly signals effort ("Now getting results from American Airlines… from JetBlue…" with a running count), people rate it **higher quality, are more willing to pay, and want to use it again — even when the result is identical and the wait is *longer***. Operational transparency triggers reciprocity ([HBS — The Labor Illusion](https://www.hbs.edu/faculty/Pages/item.aspx?num=40158); [Management Science paper](https://pubsonline.informs.org/doi/10.1287/mnsc.1110.1376); [Forbes — show your work](https://www.forbes.com/sites/rogerdooley/2025/02/04/labor-illusion-why-brands-should-show-their-work-even-if-its-not-real/)). **The chattering pet *is* a labor-illusion device** — it makes transcription feel like a helpful creature doing a job for you, and makes waits feel shorter and more valued.

**(b) Loading-state / microcopy discipline — so the chatter helps instead of annoying.** From the AWS Cloudscape gen-AI loading-state pattern and loading-page craft ([Cloudscape — Generative AI loading states](https://cloudscape.design/gen-ai/patterns/generative-ai-loading-states/); [Appcues — loading page design](https://www.appcues.com/blog/loading-pages-design)):
- **Don't show a status for a sub-1-second op** — it flickers and feels jarring. If the transcript lands instantly, **skip the "formulating" beat and go straight to the happy delivery.** (Streaming/instant results beat any loading message.)
- Message format leans **present-tense, active voice**, sentence case ("Formulating…", "Cleaning that up…"). Keep it *specific* to the phase, not a generic spinner.
- **Phase the messages** to the real work, not fake time.

**The message ladder (map pet lines to actual pipeline phases):**

| Phase / trigger | What the pet does | Sample chatter (short, present-tense, rotate to avoid repeats) |
|---|---|---|
| Dictation start | Runs to front, perks up | *"Listening…"* / *"Go ahead…"* / a happy chirp |
| Actively hearing speech (amplitude high) | Beak flaps with your voice, nods | *(mostly non-verbal — flapping beak; occasional* "mhm!" *)* |
| Short utterance finished, transcript already ready (<1s) | **No working line** — jump to delivery | — |
| Longer utterance, still transcribing (>~1s) | Tilts head, little thinking loop | *"Formulating…"* / *"One sec…"* / *"Tidying that up…"* / *"Almost…"* (rotate; escalate only if it genuinely takes longer) |
| Transcript committed / pasted | Celebratory hop + crinkle, drops the "text" | *"There!"* / *"Done!"* / *"Got it."* |
| Error / no speech heard | Confused wiggle, ear droop | *"Hmm? Didn't catch that."* / *"Say again?"* |
| Long idle | Dozes / curls up | *(silent — z-z-z)* |

**Tone/format for the codebase:** keep chatter as a **rotating pool per phase** (never the same line twice in a row — the probabilistic-idle principle applied to speech), gate the "working" pool behind a **>~800ms–1s timer** so fast transcriptions skip it, and render lines as tiny speech puffs or a status ticker. If Yap does TTS the pet can literally *say* them in a cute clipped voice (Pou-style gibberish is the fallback if real words feel heavy — Pou's "speaks gibberish" is a beloved *yapping* cue, per existing doc).

---

### B4. The reactive-line idea, made concrete

The user's instinct — *short prompt → quick ack; long/essay prompt → "formulating, formulating…" then a reaction* — is exactly correct and is directly supported by the loading-state research (don't show a working state under ~1s; do phase transparency for longer work) plus the labor illusion (showing the work adds value). Concretely, branch on **transcript length / transcription latency**:

- **Short (utterance < ~1.5s of speech OR transcript ready < ~1s):** run-up → beak-flap → **instant happy delivery** (*"Got it!"*). **No working line.** Snappy = the pet feels fast and eager.
- **Long (multi-sentence / essay-length, transcription still running > ~1s):** run-up → beak-flap while you talk → on end, enter **WORKING loop**: cycle *"Formulating… formulating…"* / *"One sec…"* / *"Tidying that up…"* with a head-tilt thinking animation, escalating a notch if it's really long → then the **DELIVER reaction** (bigger celebratory hop + crinkle: *"Whew — there!"*). The longer the essay, the more the working-chatter *earns* the wait via the labor illusion.

The branch key is simply **"did the transcript land before the ~1s working-threshold timer fired?"** — no line if yes, working-loop if no. This is a handful of `if`s over signals Yap already emits (dictation start, speech end, transcript ready, paste).

---

### B5. Part B verdict — the concrete design to build

**Build a cozy papercraft micro-diorama with a summonable, chattering pet.** Specifically:

1. **World:** 3 parallax papercraft layers in the pill — back wall/sky, mid props (bowl/plant/cushion, drop-shadowed), front floor strip. Same matte-paper material as the pet (Part A), so pet + world are one craft language. Optional clock-based day/night tint for coziness. Depth is fake (flat cutouts + drop shadows + parallax) — cheap, 60fps, no 3D.
2. **Pet:** the Part-A **paper-skinned round chick** (rig separate from skin so it's swappable).
3. **Behavior (5-state FSM, modeled on oneko):** IDLE/WANDER (calm, probabilistic idle — waddle, sniff props, doze; near-invisible so it never becomes Clippy) → on dictation, **anticipation → RUN to front** (the "comes when called" payoff) → **WORKING** (faces user, beak flaps to mic amplitude, chatters) → **DELIVER** (celebratory hop + paper crinkle + "there!") → **RETURN** to wandering.
4. **Chatter:** phase-mapped, rotating-pool status lines that act as a **labor-illusion** value-add; **gated so sub-1s transcriptions skip the working line** (short → instant ack; long → "formulating…" loop → reaction), present-tense and specific per the loading-state guidance.

This is buildable now in Canvas2D within Yap's existing transparent NSPanel, uses only signals the app already has (dictation start/stop, mic `--level`, transcript-ready/paste), adds no runtime dependency, and turns the pill from a "box with a pet" into "a tiny world a creature lives in and runs across to help you."

---

## Consolidated source list

**Cuteness science & animation craft**
- Glocker et al., *Baby Schema… Cuteness Perception* — [Ethology 2009](https://onlinelibrary.wiley.com/doi/abs/10.1111/j.1439-0310.2008.01603.x) · [PNAS 2009 (brain reward)](https://www.pnas.org/doi/10.1073/pnas.0811620106) · [ScienceDirect: Baby Schema](https://www.sciencedirect.com/topics/neuroscience/baby-schema)
- 12 Principles / squash & stretch — [Animation Mentor](https://www.animationmentor.com/blog/squash-and-stretch-the-12-basic-principles-of-animation/) · [Animaker](https://www.animaker.com/hub/12-principles-of-animation/)

**Paper / craft characters (Part A proof set)**
- Paper Mario: The Origami King — [MarioWiki: Olivia](https://www.mariowiki.com/Olivia) · [Wikipedia](https://en.wikipedia.org/wiki/Paper_Mario:_The_Origami_King) · [Incredible Characters Wiki: Olivia](https://greatcharacters.miraheze.org/wiki/Olivia_(Paper_Mario)) · [Play Nintendo diorama printable](https://play.nintendo.com/printables/print-and-play/mario-the-origami-king-diorama/)
- Tearaway (Media Molecule) — [Wikipedia](https://en.wikipedia.org/wiki/Tearaway_(video_game)) · [PlayStation Blog: 7 unseen concepts](https://blog.playstation.com/archive/2017/03/31/7-unseen-concepts-that-shaped-tearaway-media-molecules-brilliant-papercraft-adventure) · [Discovering Game Design](https://discoveringgamedesign.wordpress.com/2015/02/17/tearaway-papercraft-come-to-life/)
- Kirby's Epic Yarn / Yoshi's Crafted World — [Nintendo Life](https://www.nintendolife.com/news/2020/10/soapbox_10_years_on_kirbys_epic_yarn_is_still_the_pink_puffballs_finest_outing) · [Wikipedia: Epic Yarn](https://en.wikipedia.org/wiki/Kirby's_Epic_Yarn) · [CBR](https://www.cbr.com/kirbys-epic-yarn-creative-nintendo-wii-game/) · [Nintendo UK](https://www.nintendo.com/en-gb/News/2019/January/Yoshi-s-Crafted-World-for-Nintendo-Switch-and-Kirby-s-Epic-Yarn-for-Nintendo-3DS-family-systems-both-launch-in-March-2019-1495499.html)
- Papercraft toy mascots — [Paper Foldables](https://www.paperfoldables.com/) · [Make: Posable Papercraft Makey Mascot](https://makezine.com/projects/posable-papercraft-makey-mascot/)

**Habitat / behavior (Part B)**
- Neko Atsume — [Wikipedia](https://en.wikipedia.org/wiki/Neko_Atsume) · [Yard wiki](https://nekoatsume.fandom.com/wiki/Yard)
- oneko / neko cursor-chase state machine — [adryd325/oneko.js](https://github.com/adryd325/oneko.js/) · [Neko (software) — Wikipedia](https://en.wikipedia.org/wiki/Neko_(software)) · [cyberciti](https://www.cyberciti.biz/open-source/oneko-app-creates-cute-cat-chasing-around-your-mouse/)
- Shimeji / Desktop Mate roaming — [shimejis.xyz](https://shimejis.xyz/) · [Best Desktop Pets for Mac 2026](https://mac-pet.com/en/blog/best-desktop-pets-mac/) · [Desktop Mate on Steam](https://store.steampowered.com/app/3301060/Desktop_Mate/)
- Bitzee reactive digital pet — [Spin Master](https://shop.spinmaster.com/products/bitzee-interactive-digital-toy-pet-6068705)
- clippy.js probabilistic idle + desktop-pet design ethics — [clippy.js](https://github.com/clippyjs/clippy.js) · [Nathalie Lawhead on desktop pets](https://www.nathalielawhead.com/candybox/about-desktop-pets-virtual-companions-discussing-the-inhabitants-that-fill-the-void-of-our-digital-spaces)

**Status-chatter / loading design**
- [AWS Cloudscape — Generative AI loading states](https://cloudscape.design/gen-ai/patterns/generative-ai-loading-states/)
- [Appcues — loading page design](https://www.appcues.com/blog/loading-pages-design)
- Labor Illusion — [HBS (Ryan Buell)](https://www.hbs.edu/faculty/Pages/item.aspx?num=40158) · [Management Science](https://pubsonline.informs.org/doi/10.1287/mnsc.1110.1376) · [Forbes](https://www.forbes.com/sites/rogerdooley/2025/02/04/labor-illusion-why-brands-should-show-their-work-even-if-its-not-real/)

**Prior in-repo dossiers this builds on:** `docs/research/cute-companion-design.md` (chick/blob recommendation, desktop-pet genre study), `docs/research/origami-yapping-pill.md` (Tauri/Canvas constraints; offline Origami Simulator bake pipeline), `docs/research/cute-character-animation-code.md`, `docs/research/motion-and-components.md`, `docs/research/companion-and-pill-inspiration.md`.

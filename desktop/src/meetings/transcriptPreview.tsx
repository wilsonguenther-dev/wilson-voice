/**
 * YV108 — dev tooling: the Meetings transcript in both of its shapes.
 *
 * A two-track transcript needs a recorded second track to exist, which means
 * seeing one — or screenshotting one for a review — would otherwise mean
 * holding a live call with the tap granted. This mounts the REAL
 * `TranscriptList` inside the app's own chrome against two fixed segment sets,
 * so a screenshot of this page is a screenshot of the shipping UI:
 *
 *   * `mic-only`  — a 22-A meeting: one speaker, the layout that already ships.
 *   * `two-track` — a virtual meeting: one interleaved Me/Them conversation.
 *   * `blank-tap` — a virtual meeting whose far side never made a sound: the
 *     tap recorded rows, the ASR found no words in them, and the screen must
 *     look exactly like the mic-only one. This scene exists because the review
 *     of YV108 found the opposite — labelled empty "Them" rows and a widened
 *     speaker gutter for a speaker the exported file did not contain.
 *
 * YV127 — the same three scenes are also the two sides of the overlap caveat,
 * which is why no fourth one was added for it: `mic-only` and `blank-tap` are
 * both meetings whose microphone is the clustered track, so they carry the
 * sentence, and `two-track` is the one configuration that never clusters Track
 * A, so it must not. Screenshotting the trio therefore shows the caveat
 * appearing AND being withheld, from the shipping component.
 *
 * The kinds those scenes render at are load-bearing for that claim, not
 * decoration: `blank-tap` and `two-track` BOTH declare `kind = "virtual"` and
 * differ only in whether the tap delivered words, which is what makes
 * `blank-tap` the case a `kind == "virtual"` gate gets wrong rather than a
 * second copy of `mic-only`. See the `kind` binding in `Preview` below.
 *
 * Same rules as `license/preview.tsx` and `meetings/preview.tsx`: a BUILD ENTRY
 * behind `YAP_DEV_TOOLING=1`, so no shipped build carries it.
 *
 *   YAP_DEV_TOOLING=1 npm run build   # then dist/dev/meeting-transcript-preview.html
 */
import React, { useState } from "react";
import ReactDOM from "react-dom/client";
import TranscriptList from "./TranscriptList";
import { MIC_TRACK, SYSTEM_TRACK, type TranscriptSegment } from "./transcript";
import "../App.css";

const MIC_ONLY: TranscriptSegment[] = [
  { id: "a", startSeconds: 0, track: MIC_TRACK, text: "let us start with the release checklist" },
  { id: "b", startSeconds: 6.5, track: MIC_TRACK, text: "the notarised dmg goes out on friday" },
  {
    id: "c",
    startSeconds: 14,
    track: MIC_TRACK,
    text: "and we hold the price where it is until the quarter closes",
  },
];

/** Deliberately supplied grouped by track — the shape two transcription passes
 *  produce — so the page is also a live demonstration that the interleave is
 *  the renderer's doing. */
const TWO_TRACK: TranscriptSegment[] = [
  { id: "m1", startSeconds: 0, track: MIC_TRACK, text: "can we ship the notarised build on friday" },
  { id: "m2", startSeconds: 9, track: MIC_TRACK, text: "the signing cert cleared this morning" },
  {
    id: "m3",
    startSeconds: 18,
    track: MIC_TRACK,
    text: "and we hold the price where it is until the quarter closes",
  },
  {
    id: "s1",
    startSeconds: 4.5,
    track: SYSTEM_TRACK,
    text: "friday works if the certificate is not still pending",
  },
  {
    id: "s2",
    startSeconds: 13.5,
    track: SYSTEM_TRACK,
    text: "then i will tell the reseller friday and put it in writing",
  },
  {
    id: "s3",
    startSeconds: 23.25,
    track: SYSTEM_TRACK,
    text: "agreed nobody is discounting this quarter",
  },
];

/** The far side was there but silent: rows exist, words do not. Blank spans are
 *  a real ASR output, which is why `render_transcript` has always dropped them
 *  and why the mirror now does too. */
const BLANK_TAP: TranscriptSegment[] = [
  { id: "b1", startSeconds: 0, track: MIC_TRACK, text: "can anyone hear me on this call" },
  { id: "b2", startSeconds: 5, track: SYSTEM_TRACK, text: "   " },
  { id: "b3", startSeconds: 9, track: MIC_TRACK, text: "i will send the notes over instead" },
  { id: "b4", startSeconds: 13, track: SYSTEM_TRACK, text: "\n\t " },
];

const SCENES = ["two-track", "mic-only", "blank-tap"] as const;
type Scene = (typeof SCENES)[number];

function sceneFromHash(): Scene {
  const h = window.location.hash.replace("#", "") as Scene;
  return SCENES.includes(h) ? h : "two-track";
}

function Preview() {
  const [scene, setScene] = useState<Scene>(sceneFromHash());
  const twoTrack = scene === "two-track";
  const segments =
    scene === "two-track" ? TWO_TRACK : scene === "blank-tap" ? BLANK_TAP : MIC_ONLY;
  /**
   * The meeting's DECLARED kind, which is not the same question as how many
   * tracks carried words. `two-track` and `blank-tap` are both meetings the
   * user picked "call" for — the difference between them is only that one
   * tap delivered speech and the other delivered silence — so both render at
   * `virtual`, and only the `mic-only` scene is the un-picked `unknown`.
   *
   * This distinction is the whole point of the `blank-tap` scene under YV127:
   * at `kind = "virtual"` it is the case a gate written against the kind alone
   * would get wrong, because the caveat is owed there anyway. Rendering it at
   * `unknown` would have made it a duplicate of `mic-only`.
   */
  const kind = scene === "mic-only" ? "unknown" : "virtual";

  return (
    <div className="shell">
      <aside className="sidebar">
        <div className="brand-block">
          <div className="mark">
            <span className="dot" />
          </div>
          <div>
            <div className="brand-name">Yap</div>
            <div className="brand-tag">v{__APP_VERSION__} · local · private</div>
          </div>
        </div>
        <nav className="nav">
          {["Home", "Meetings", "Permissions", "Insights", "Settings"].map((label) => (
            <button
              key={label}
              className={label === "Meetings" ? "nav-item active" : "nav-item"}
            >
              <span>{label}</span>
            </button>
          ))}
        </nav>
        <div className="sidebar-foot">
          <button className="dictate-side">
            Dictate<span className="dictate-key">fn⌃</span>
          </button>
        </div>
      </aside>

      <section className="main">
        <header className="main-head">
          <div>
            <h1>Meetings</h1>
            <p className="lede">
              Recorded meetings, searchable and exportable. Audio is kept for 7 days;
              the transcript stays.
            </p>
          </div>
          <div className="head-state">
            <div className="status-pill ready">Ready — hold fn⌃</div>
          </div>
        </header>

        <div className="content">
          <div className="meeting-detail">
            <button className="ghost">← All meetings</button>
            <input
              className="meeting-title-edit"
              defaultValue={
                scene === "two-track"
                  ? "Reseller sync"
                  : scene === "blank-tap"
                    ? "Standup (nobody unmuted)"
                    : "Thursday planning"
              }
            />
            <p className="card-meta">
              <span>
                {twoTrack
                  ? "Today 09:14 · 27s · complete"
                  : "Today 09:03 · 18s · complete"}
              </span>
              <span>audio kept</span>
            </p>
            <TranscriptList segments={segments} kind={kind} />
          </div>
        </div>
      </section>

      {/* Harness controls, deliberately unstyled by the app's own classes so
          they can never be mistaken for product UI in a screenshot.

          The `kind=` readout is here for the same reason the buttons are: a
          screenshot of this page is used as evidence, and the meeting kind is
          an INPUT no pixel of the product UI reveals. Printing it in the
          harness strip means a reviewer reads the kind off the picture instead
          of off a caption that can drift from the code — which is exactly how
          `blank-tap.png` came to be captioned `virtual` while the component
          was being handed `unknown`. */}
      <div
        style={{
          position: "fixed",
          right: 12,
          bottom: 12,
          zIndex: 999,
          display: "flex",
          alignItems: "center",
          gap: 6,
          font: "11px ui-monospace, monospace",
          opacity: 0.55,
        }}
      >
        <span data-testid="harness-kind">kind={kind}</span>
        {SCENES.map((k) => (
          <button
            key={k}
            onClick={() => {
              window.location.hash = k;
              setScene(k);
            }}
            style={{ padding: "2px 6px", borderRadius: 4 }}
          >
            {k}
          </button>
        ))}
      </div>
    </div>
  );
}

ReactDOM.createRoot(document.getElementById("root")!).render(
  <React.StrictMode>
    <Preview />
  </React.StrictMode>,
);

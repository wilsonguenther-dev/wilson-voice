/**
 * yap-notetaker-onboard — the 2-minute tour (step 1 of Set up Notetaker).
 *
 * A LOCAL, scripted walkthrough of the bundled sample meeting
 * (`sampleMeeting.ts`): start, live meters, stop, transcript, notes, export.
 * Nothing records, nothing is written, nothing leaves the Mac — the meters are
 * drawn, the transcript is the fixture, and Export shows what the Markdown
 * file would hold. Done = the user reaches the last step and presses Finish.
 */
import { useState } from "react";
import TranscriptList from "./TranscriptList";
import { SAMPLE_EXPORT, SAMPLE_NOTES, SAMPLE_SEGMENTS, SAMPLE_TITLE } from "./sampleMeeting";

export const TOUR_STEPS = [
  { id: "start", title: "Start", body: "Press Record meeting (or use the menu bar). Yap asks what kind of meeting it is so it knows who is in the room." },
  { id: "meters", title: "Live meters", body: "While it records, the two meters show your mic and the call's audio. If one stays flat, Yap tells you right away." },
  { id: "stop", title: "Stop", body: "Press Stop. Yap transcribes on your Mac — no upload, no bot in the call." },
  { id: "transcript", title: "Transcript", body: "Every turn, with who said it and when. It's searchable from the Meetings search box." },
  { id: "notes", title: "Notes", body: "Yap writes short notes and action items from the transcript, on your Mac." },
  { id: "export", title: "Export", body: "Export saves the notes and transcript as a Markdown file you own." },
] as const;

function Meters() {
  const bars = [0.35, 0.7, 0.5, 0.9, 0.45, 0.6, 0.3, 0.8];
  return (
    <div className="nt-tour-meters" aria-label="Sample meters: mic and call audio">
      {["Mic", "Call"].map((label, row) => (
        <div key={label} className="nt-tour-meter">
          <span className="tiny">{label}</span>
          <span className="nt-tour-bars" aria-hidden>
            {bars.map((h, i) => (
              <span key={i} style={{ height: `${Math.round(((h + row * 0.15) % 1) * 100)}%` }} />
            ))}
          </span>
        </div>
      ))}
    </div>
  );
}

export default function NotetakerTour({
  onFinish,
  onClose,
  initialStep = 0,
}: {
  onFinish: () => void;
  onClose: () => void;
  initialStep?: number;
}) {
  const [i, setI] = useState(initialStep);
  const step = TOUR_STEPS[i];
  const last = i === TOUR_STEPS.length - 1;
  return (
    <section className="panel nt-setup nt-tour" aria-label="Notetaker tour" data-tour-step={step.id}>
      <header className="nt-setup-head">
        <strong>{SAMPLE_TITLE}</strong>
        <span className="tiny">
          {i + 1} of {TOUR_STEPS.length}
        </span>
      </header>
      <div className="nt-setup-body">
        <h3 className="nt-tour-title">{step.title}</h3>
        <p className="tiny">{step.body}</p>
        {step.id === "start" && (
          <button className="primary" disabled>
            Record meeting
          </button>
        )}
        {step.id === "meters" && <Meters />}
        {step.id === "stop" && (
          <button className="primary danger" disabled>
            Stop
          </button>
        )}
        {step.id === "transcript" && <TranscriptList kind="virtual" segments={SAMPLE_SEGMENTS} />}
        {step.id === "notes" && (
          <ul className="nt-tour-notes">
            {SAMPLE_NOTES.map((n) => (
              <li key={n}>{n}</li>
            ))}
          </ul>
        )}
        {step.id === "export" && <pre className="nt-tour-export">{SAMPLE_EXPORT}</pre>}
      </div>
      <footer className="nt-setup-foot">
        <button className="ghost" onClick={onClose}>
          Exit tour
        </button>
        <span className="nt-setup-foot-gap" />
        {i > 0 && (
          <button className="ghost" onClick={() => setI(i - 1)}>
            Back
          </button>
        )}
        <button className="primary" onClick={() => (last ? onFinish() : setI(i + 1))}>
          {last ? "Finish" : "Next"}
        </button>
      </footer>
    </section>
  );
}

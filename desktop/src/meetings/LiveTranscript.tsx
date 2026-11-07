/**
 * yap-notetaker-notes — the last few lines of each track while a meeting
 * records, under "You" / "Call" on a call and with no label on a mic-only
 * meeting. Drawn inside the recording banner of the Meetings view; it clears
 * itself at Stop, and the finished transcript (TranscriptList) is the record.
 *
 * yap-1106-live-whos-talking — a line whose voice matched an enrolled person
 * reads their name ("Alex", with the reason as its tooltip), an unmatched one
 * "Someone new", and anything unsure keeps You / Call. The post-meeting pass
 * replaces every live name.
 */
import { listen } from "@tauri-apps/api/event";
import { useEffect, useState } from "react";
import { hasLiveLines, LIVE_EVENT, liveRuns, NO_LIVE, reduceLive, type LiveSnapshot } from "./liveSnapshot";

/** The presentational half: pure, so the test renders it without Tauri. */
export function LiveLines({ snapshot }: { snapshot: LiveSnapshot }) {
  if (!hasLiveLines(snapshot)) {
    return (
      <p className="tiny live-transcript-wait" data-live-transcript="waiting">
        Live transcript appears here every few seconds while you talk.
      </p>
    );
  }
  return (
    <div className="live-transcript" data-live-transcript="lines" aria-live="polite">
      {snapshot.tracks
        .filter((t) => t.lines.length > 0)
        .map((t) => (
          <div className="live-track" key={t.track}>
            {liveRuns(t).map((run, r) => (
              <div className="live-run" key={r}>
                {run.label && (
                  <span
                    className={t.track === 0 ? "seg-who" : "seg-who them"}
                    title={run.hint ?? undefined}
                    data-live-voice={run.hint ? "matched" : undefined}
                  >
                    {run.label}
                  </span>
                )}
                <ul>
                  {run.lines.map((line, i) => (
                    <li key={i}>{line}</li>
                  ))}
                </ul>
              </div>
            ))}
          </div>
        ))}
    </div>
  );
}

export default function LiveTranscript({ recording }: { recording: boolean }) {
  const [snapshot, setSnapshot] = useState<LiveSnapshot>(NO_LIVE);
  useEffect(() => {
    if (!recording) {
      setSnapshot(NO_LIVE);
      return;
    }
    let unlisten: (() => void) | undefined;
    let gone = false;
    listen<LiveSnapshot>(LIVE_EVENT, (e) => setSnapshot(reduceLive(e.payload, true)))
      .then((u) => (gone ? u() : (unlisten = u)))
      // No listener means no live lines; the recording and the final
      // transcript are unaffected, so this is shown as the waiting line.
      .catch(() => setSnapshot(NO_LIVE));
    return () => {
      gone = true;
      unlisten?.();
    };
  }, [recording]);
  if (!recording) return null;
  return <LiveLines snapshot={snapshot} />;
}

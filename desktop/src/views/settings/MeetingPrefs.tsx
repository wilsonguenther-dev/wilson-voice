// yap-1106-settings-redesign (§14, spec §1.4) — the two Settings › Meetings
// blocks that had no pane before: Meeting kinds (what a new meeting starts as)
// and Speakers / voiceprints (how Yap labels who is talking, and the truth
// about voiceprints: none are kept). Call audio, detection and destinations
// stay in ./Meetings; the retention rule renders from ./Privacy (its sentence
// is pinned there by the Rust retention test).
import { useAppCtx } from "../../appShell";
import {
  MIC_SPEAKER_LABEL,
  SYSTEM_SPEAKER_LABEL,
  UNCLUSTERED_SPEAKER_LABEL,
} from "../../meetings/transcript";
import { writeMeetingKindDefault } from "../../meetings/kindDefault";

export type MeetingPrefsPart = "kinds" | "speakers";

/** What each kind changes, in plain words. Unknown kinds get no line. */
export const KIND_EXPLAINS: Record<string, string> = {
  virtual: `Your microphone is "${MIC_SPEAKER_LABEL}" and everyone on the call is "${SYSTEM_SPEAKER_LABEL}".`,
  in_person: `Everyone is in the room, so Yap tells the voices apart as ${UNCLUSTERED_SPEAKER_LABEL} 1, ${UNCLUSTERED_SPEAKER_LABEL} 2.`,
  unknown: "Yap treats it like a room and tells the voices apart. Recording works the same.",
};

export default function MeetingPrefs({ part }: { part: MeetingPrefsPart }) {
  return part === "kinds" ? <MeetingKinds /> : <Speakers />;
}

function MeetingKinds() {
  const { meetingKind, meetingKinds, setMeetingKind, meetingStatus, toast } = useAppCtx();
  const recording = !!meetingStatus?.recording;
  function choose(kind: string) {
    if (kind === meetingKind) return;
    setMeetingKind(kind);
    const saved = writeMeetingKindDefault(kind);
    console.info(`ui settings_meeting_kind kind=${kind} saved=${saved}`);
    if (!saved) toast("Saved for now. Yap couldn't remember it for the next launch.");
  }
  return (
    <section className="settings-panel" data-settings-pane="meeting-kinds">
      <div className="panel" data-row data-settings-row="meeting-kinds">
        <h3>Meeting kinds</h3>
        {meetingKinds.length === 0 ? (
          <p className="settings-hint" data-meeting-kinds-empty>
            The kinds have not loaded yet. New meetings start as Not sure, and recording works the same.
          </p>
        ) : (
          <>
            <div className="field">
              <span>New meetings start as</span>
              <div className="meeting-kind-picker" role="radiogroup" aria-label="New meetings start as">
                {meetingKinds.map((c) => (
                  <button
                    key={c.kind}
                    type="button"
                    role="radio"
                    aria-checked={meetingKind === c.kind}
                    className={meetingKind === c.kind ? "chip selected" : "chip"}
                    onClick={() => choose(c.kind)}
                  >
                    {c.label}
                  </button>
                ))}
              </div>
            </div>
            {KIND_EXPLAINS[meetingKind] && <p className="settings-hint">{KIND_EXPLAINS[meetingKind]}</p>}
          </>
        )}
        <p className="settings-hint">
          You can still change it on the Meetings screen before you press Record.
          {recording && " A meeting is recording now; this applies to the next one."}
        </p>
      </div>
    </section>
  );
}

function Speakers() {
  return (
    <section className="settings-panel" data-settings-pane="speakers">
      <div className="panel" data-row data-settings-row="speakers">
        <h3>Speakers</h3>
        <p className="settings-hint">
          In a virtual meeting your microphone is "{MIC_SPEAKER_LABEL}" and the call is "{SYSTEM_SPEAKER_LABEL}". In
          person, Yap tells the voices apart as {UNCLUSTERED_SPEAKER_LABEL} 1, {UNCLUSTERED_SPEAKER_LABEL} 2, and so
          on, worked out fresh for each meeting. These labels are a best guess, not a fact.
        </p>
      </div>
      <div className="panel" data-row data-settings-row="voiceprints">
        <h3>Voiceprints</h3>
        <p className="settings-hint" data-voiceprints-empty>
          None saved. Yap does not keep a voiceprint of you or anyone else, so there is nothing here to delete.
        </p>
      </div>
    </section>
  );
}

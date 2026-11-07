/**
 * YV108 — the meeting transcript, as the Meetings detail draws it.
 *
 * ONE interleaved list, never two columns: both tracks are already on a single
 * time base by the time they are stored, and two parallel dumps would hand the
 * reader back exactly the work the merge did. A mic-only meeting takes the same
 * path and comes out as one speaker — no phantom second one, and no widened
 * gutter reserving room for one.
 *
 * YV125 — the speaker column depends on the MEETING as well as its segments,
 * so the meeting's `kind` is a prop. "You" is a claim about identity, and it is
 * only true where the mechanism makes it true (a call whose other participants
 * are on their own track); everywhere else the microphone is the track being
 * clustered and its lines say only that somebody spoke.
 *
 * It is a component rather than JSX inlined in `App.tsx` so the dev-tooling
 * preview (`dev/meeting-transcript-preview.html`) renders the REAL thing: a
 * two-track meeting needs a second recorded track to look at, which means a
 * screenshot of it would otherwise mean holding a live call.
 */
import { useEffect, useRef, useState } from "react";
import {
  correctionSentence,
  isTwoTrack,
  mergeChoices,
  nameTarget,
  orderedTranscript,
  splitFromHere,
  MIC_TRACK,
  UNDO_WINDOW_MS,
  type SpeakerCorrection,
  type TranscriptSegment,
} from "./transcript";

/**
 * yap-1106-auto-label-and-correct — what a correction hands back: a function
 * that undoes it (offered for 10 s), or null when the correction failed (the
 * caller already said why).
 */
export type CorrectSpeakers = (c: SpeakerCorrection) => Promise<(() => Promise<void>) | null>;

/**
 * Should the speaker column show? On a two-track call, and on any meeting the
 * diarization pass attributed (a "Speaker N" or a name to correct). A single
 * unattributed mic track still carries no label at all.
 */
export function showsSpeakers(rows: readonly TranscriptSegment[]): boolean {
  return (
    isTwoTrack(rows) ||
    rows.some(
      (s) =>
        (s.clusterIndex !== null && s.clusterIndex !== undefined) || Boolean(s.speakerName?.trim()),
    )
  );
}

type MenuStep = "menu" | "rename" | "someoneElse" | "merge";

/**
 * The chip's menu: Rename / This is someone else / Merge with… / Split this
 * speaker. Every action moves whole turns; the backend decides and locks.
 */
function SpeakerMenu({
  segments,
  kind,
  segmentId,
  track,
  clusterIndex,
  speaker,
  onPick,
  onClose,
}: {
  segments: readonly TranscriptSegment[];
  kind?: string | null;
  segmentId: string;
  track: number;
  clusterIndex: number;
  speaker: string;
  onPick: (c: SpeakerCorrection, mergedInto?: string) => void;
  onClose: () => void;
}) {
  const [step, setStep] = useState<MenuStep>("menu");
  const [name, setName] = useState("");
  const first = useRef<HTMLButtonElement | HTMLInputElement | null>(null);
  useEffect(() => {
    first.current?.focus();
  }, [step]);
  const merges = mergeChoices(segments, kind, track, clusterIndex);
  const splitIds = splitFromHere(segments, track, clusterIndex, segmentId);
  const submitName = () => {
    const target = nameTarget(name);
    if (!target) return;
    onPick(
      step === "rename"
        ? { kind: "rename", track, clusterIndex, target }
        : { kind: "someoneElse", track, segmentIds: [segmentId], target },
    );
  };
  return (
    <div
      className="speaker-menu"
      role="dialog"
      aria-label={`Correct ${speaker}`}
      onKeyDown={(e) => {
        if (e.key === "Escape") {
          e.stopPropagation();
          onClose();
        }
      }}
    >
      {step === "menu" && (
        <div role="menu" className="speaker-menu-list">
          <button
            type="button"
            role="menuitem"
            ref={(el) => {
              first.current = el;
            }}
            onClick={() => setStep("rename")}
          >
            Rename
          </button>
          <button type="button" role="menuitem" onClick={() => setStep("someoneElse")}>
            This is someone else
          </button>
          <button
            type="button"
            role="menuitem"
            disabled={merges.length === 0}
            title={merges.length === 0 ? "No other speaker to merge with" : undefined}
            onClick={() => setStep("merge")}
          >
            Merge with…
          </button>
          <button
            type="button"
            role="menuitem"
            disabled={splitIds.length === 0}
            title={
              splitIds.length === 0
                ? "Open this on a later line of the speaker to split from there"
                : `This line and the ${splitIds.length - 1} after it become a new speaker`
            }
            onClick={() => onPick({ kind: "split", track, clusterIndex, segmentIds: splitIds })}
          >
            Split this speaker
          </button>
        </div>
      )}
      {(step === "rename" || step === "someoneElse") && (
        <form
          className="speaker-menu-form"
          onSubmit={(e) => {
            e.preventDefault();
            submitName();
          }}
        >
          <label className="speaker-menu-label">
            {step === "rename" ? `Every line of ${speaker} becomes` : "This line was said by"}
            <input
              ref={(el) => {
                first.current = el;
              }}
              value={name}
              maxLength={60}
              placeholder="Name"
              onChange={(e) => setName(e.target.value)}
            />
          </label>
          <div className="speaker-menu-actions">
            <button type="button" className="link" onClick={() => setStep("menu")}>
              Back
            </button>
            <button type="submit" className="primary" disabled={!nameTarget(name)}>
              Save
            </button>
          </div>
        </form>
      )}
      {step === "merge" && (
        <div role="menu" className="speaker-menu-list">
          {merges.map((m, i) => (
            <button
              key={m.clusterIndex}
              type="button"
              role="menuitem"
              ref={
                i === 0
                  ? (el) => {
                      first.current = el;
                    }
                  : undefined
              }
              onClick={() =>
                onPick(
                  { kind: "merge", track, fromCluster: clusterIndex, intoCluster: m.clusterIndex },
                  m.label,
                )
              }
            >
              {m.label}
            </button>
          ))}
          <button type="button" className="link" onClick={() => setStep("menu")}>
            Back
          </button>
        </div>
      )}
    </div>
  );
}

/**
 * yap24-NT11 — the one line that admits rows were hidden as speaker echo. Pure,
 * so the copy is tested without a DOM; null when nothing was hidden.
 */
export function echoHiddenLine(count: number): string | null {
  if (count <= 0) return null;
  return `${count} ${count === 1 ? "line" : "lines"} hidden as speaker echo`;
}

/**
 * yap-1106-transcript-grid — the rows the list draws: `segments`, or, once the
 * reader pressed Show, the segments and the hidden echo rows interleaved in
 * time order. Pure, so the two-track decision below is testable without a DOM.
 */
export function shownRows<S extends TranscriptSegment>(
  segments: readonly S[],
  hidden: readonly S[] | undefined,
  showEcho: boolean,
): readonly S[] {
  return showEcho && hidden && hidden.length > 0
    ? [...segments, ...hidden].sort((a, b) => a.startSeconds - b.startSeconds)
    : segments;
}

export default function TranscriptList<S extends TranscriptSegment>({
  segments,
  hidden,
  kind,
}: {
  segments: readonly S[];
  /**
   * yap24-NT11 — mic rows the echo-residue rule suppressed. Never rendered
   * until the reader asks; then they join the list inline, in time order.
   */
  hidden?: readonly S[];
  /**
   * The meeting's `meetings.kind` (YV125). Optional so a caller rendering a row
   * from before migration 4 does not have to invent one — a missing kind is
   * `unknown`, which is the branch that does NOT claim the microphone is you.
   */
  kind?: string | null;
  /**
   * yap-1106-auto-label-and-correct — when given, every attributed speaker
   * chip is a button that opens the correction menu.
   */
  onCorrect?: CorrectSpeakers;
}) {
  const [showEcho, setShowEcho] = useState(false);
  const [menuFor, setMenuFor] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);
  const [undoBar, setUndoBar] = useState<{ sentence: string; undo: () => Promise<void> } | null>(
    null,
  );
  // The undo bar lives for exactly UNDO_WINDOW_MS, then the correction stands.
  useEffect(() => {
    if (!undoBar) return;
    const t = window.setTimeout(() => setUndoBar(null), UNDO_WINDOW_MS);
    return () => window.clearTimeout(t);
  }, [undoBar]);
  const pick = async (c: SpeakerCorrection, mergedInto?: string) => {
    if (!onCorrect || busy) return;
    setBusy(true);
    setMenuFor(null);
    try {
      const undo = await onCorrect(c);
      if (undo) setUndoBar({ sentence: correctionSentence(c, mergedInto), undo });
    } finally {
      setBusy(false);
    }
  };
  const echoLine = echoHiddenLine(hidden?.length ?? 0);
  const shown = shownRows(segments, hidden, showEcho);
  // yap-1106-transcript-grid — the class and the speaker cells are decided by
  // the rows that are DRAWN, not by `segments` alone: once Show adds the echo
  // rows, the list may gain (or already hold) a second track, and a row whose
  // label is missing lands in the wrong grid column.
  // yap-1106-auto-label-and-correct — and an attributed single-track meeting
  // shows its speakers too, so a "Speaker 2" can be named or corrected.
  const speakers = showsSpeakers(shown);
  return (
    <>
    {undoBar && (
      <p className="transcript-undo" role="status">
        {undoBar.sentence}
        {" - "}
        <button
          type="button"
          className="link"
          disabled={busy}
          onClick={() => {
            const { undo } = undoBar;
            setUndoBar(null);
            setBusy(true);
            void undo().finally(() => setBusy(false));
          }}
        >
          Undo
        </button>
      </p>
    )}
    {echoLine && (
      <p className="transcript-echo-note">
        {echoLine}
        {" - "}
        <button type="button" className="link" onClick={() => setShowEcho((v) => !v)}>
          {showEcho ? "Hide" : "Show"}
        </button>
      </p>
    )}
    {/* Both the class and the rows come from the same rendering rules the
        export uses: a track that contributes no lines is not a second speaker,
        so it does not widen the gutter either. */}
    <ol className={speakers ? "transcript two-track" : "transcript"}>
      {orderedTranscript(shown, kind).map(({ segment, track, speaker, offset, text }) => (
        <li key={segment.id}>
          {/* Timestamp and speaker are DATA, so they wear the pixel voice the
              rest of the app gives numbers. */}
          <span className="seg-time">{offset}</span>
          {/* yap-notetaker-notes — turns are You / Call on a two-track call; a
              single mic track (in person) carries no speaker label at all. */}
          {speakers &&
            (onCorrect &&
            segment.clusterIndex !== null &&
            segment.clusterIndex !== undefined ? (
              <span className="seg-who-wrap">
                <button
                  type="button"
                  className={track === MIC_TRACK ? "seg-who seg-who-chip" : "seg-who them seg-who-chip"}
                  aria-haspopup="dialog"
                  aria-expanded={menuFor === segment.id}
                  disabled={busy}
                  title="Correct this speaker"
                  onClick={() => setMenuFor((v) => (v === segment.id ? null : segment.id))}
                >
                  {speaker}
                </button>
                {menuFor === segment.id && (
                  <SpeakerMenu
                    segments={shown}
                    kind={kind}
                    segmentId={segment.id}
                    track={track}
                    clusterIndex={segment.clusterIndex}
                    speaker={speaker}
                    onPick={(c, into) => void pick(c, into)}
                    onClose={() => setMenuFor(null)}
                  />
                )}
              </span>
            ) : (
              <span className={track === MIC_TRACK ? "seg-who" : "seg-who them"}>{speaker}</span>
            ))}
          {/* The line's collapsed text, never `segment.text` — the export
              renders the collapsed form and the screen must not differ. It is
              ALWAYS the row's last child: App.css pins `.seg-text` to the last
              grid column, so a row with no speaker cell cannot push its prose
              into the 2.6 rem speaker gutter (yap-1106-transcript-grid). */}
          <span className="seg-text">{text}</span>
        </li>
      ))}
    </ol>
    </>
  );
}

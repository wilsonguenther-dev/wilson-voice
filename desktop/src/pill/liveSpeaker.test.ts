import { describe, expect, it } from "vitest";
import { NO_LIVE, SOMEONE_NEW, type LiveSnapshot, type LiveTrack } from "../meetings/liveSnapshot";
import {
  LIVE_SPEAKER_IDLE,
  NO_SPEAKER,
  liveSpeakerLine,
  liveSpeakerLog,
  liveSpeakerTitle,
  newestTrack,
  pickLiveSpeaker,
  speakerOfTrack,
  stepLiveSpeaker,
  type LiveSpeakerState,
} from "./liveSpeaker";

const HINT = "Probably Alex — voice match 0.82";

function snap(...tracks: LiveTrack[]): LiveSnapshot {
  return { recording: true, tracks };
}
const you = (lines: string[], voices?: LiveTrack["voices"]): LiveTrack => ({ track: 0, speaker: "You", lines, voices });
const call = (lines: string[], voices?: LiveTrack["voices"]): LiveTrack => ({ track: 1, speaker: "Call", lines, voices });
const none = { name: null, hint: null };
const alex = { name: "Alex", hint: HINT };

describe("fix-l3b4-1 — the pill's live speaker picker", () => {
  it("newest line wins: the track whose lines changed since the last snapshot", () => {
    let s: LiveSpeakerState = LIVE_SPEAKER_IDLE;
    s = stepLiveSpeaker(s, snap(you(["hi there"]), call([])), true);
    expect(s.speaker).toMatchObject({ shown: "track", word: "You", track: 0 });
    s = stepLiveSpeaker(s, snap(you(["hi there"]), call(["morning all"])), true);
    expect(s.speaker).toMatchObject({ shown: "track", word: "Call", track: 1 });
    // Track 0 speaks again: its lines change, track 1's do not.
    s = stepLiveSpeaker(s, snap(you(["hi there", "so the plan"]), call(["morning all"])), true);
    expect(s.speaker).toMatchObject({ shown: "track", word: "You", track: 0 });
  });

  it("when nothing changed, the previous speaker holds; several changed = the later track", () => {
    const a = snap(you(["one"]), call(["two"]));
    expect(newestTrack(NO_LIVE, a, null)?.track).toBe(1);
    expect(newestTrack(a, a, 0)?.track).toBe(0);
    expect(newestTrack(a, a, null)?.track).toBe(1);
    expect(newestTrack(a, a, 7)?.track).toBe(1);
    expect(newestTrack(NO_LIVE, snap(you([]), call([])), null)).toBeNull();
  });

  it("a matched name beats the track word, and the hint rides along", () => {
    const s = pickLiveSpeaker(snap(you([]), call(["let's start", "budget is fine"], [none, alex])), true);
    expect(s).toEqual({ shown: "name", word: "Alex", hint: HINT, track: 1 });
    expect(liveSpeakerTitle(s)).toBe(HINT);
    expect(liveSpeakerLine(s)).toBe(`Talking now: ${HINT}`);
  });

  it("a name with no hint still reads as a suggestion", () => {
    const s = pickLiveSpeaker(snap(call(["hello"], [{ name: "Sam", hint: null }])), true);
    expect(liveSpeakerTitle(s)).toBe("Probably Sam");
  });

  it("only the newest line of the track counts", () => {
    const s = pickLiveSpeaker(snap(call(["alex said", "then someone"], [alex, none])), true);
    expect(s).toMatchObject({ shown: "track", word: "Call" });
  });

  it("Someone new is its own kind", () => {
    const s = pickLiveSpeaker(snap(you([]), call(["who is this"], [{ name: SOMEONE_NEW, hint: null }])), true);
    expect(s).toEqual({ shown: "someone_new", word: SOMEONE_NEW, hint: null, track: 1 });
    expect(liveSpeakerTitle(s)).toBe("Someone new — a voice Yap does not know yet");
    expect(liveSpeakerLine(s)).toContain("Talking now: Someone new");
  });

  it("a mic-only meeting with no match shows nothing", () => {
    const mic: LiveTrack = { track: 0, speaker: null, lines: ["just me talking"], voices: [none] };
    expect(pickLiveSpeaker(snap(mic), true)).toBe(NO_SPEAKER);
    expect(liveSpeakerTitle(NO_SPEAKER)).toBeNull();
    expect(liveSpeakerLine(NO_SPEAKER)).toBeNull();
    // ...but a match on that one track still names the voice.
    const matched: LiveTrack = { ...mic, voices: [alex] };
    expect(pickLiveSpeaker(snap(matched), true)).toMatchObject({ shown: "name", word: "Alex" });
  });

  it("a stopped snapshot clears it, and so does the meeting no longer recording", () => {
    const s = stepLiveSpeaker(LIVE_SPEAKER_IDLE, snap(call(["hi"], [alex])), true);
    expect(s.speaker.shown).toBe("name");
    expect(stepLiveSpeaker(s, { recording: false, tracks: [] }, true)).toBe(LIVE_SPEAKER_IDLE);
    expect(stepLiveSpeaker(s, snap(call(["hi"], [alex])), false)).toBe(LIVE_SPEAKER_IDLE);
    expect(stepLiveSpeaker(s, null, true)).toBe(LIVE_SPEAKER_IDLE);
    expect(stepLiveSpeaker(LIVE_SPEAKER_IDLE, undefined, true)).toBe(LIVE_SPEAKER_IDLE);
  });

  it("an older payload without voices falls back to the track word", () => {
    const old = { recording: true, tracks: [{ track: 0, speaker: "You", lines: ["a"] }, { track: 1, speaker: "Call", lines: ["b", "  "] }] };
    expect(pickLiveSpeaker(old as LiveSnapshot, true)).toEqual({ shown: "track", word: "Call", hint: null, track: 1 });
    expect(liveSpeakerTitle(pickLiveSpeaker(old as LiveSnapshot, true))).toBe("Talking now: Call");
    expect(liveSpeakerLine(pickLiveSpeaker(old as LiveSnapshot, true))).toBe("Talking now: Call");
  });

  it("an unchanged speaker keeps the same object (no re-render)", () => {
    const a = stepLiveSpeaker(LIVE_SPEAKER_IDLE, snap(call(["hi"], [alex])), true);
    const b = stepLiveSpeaker(a, snap(call(["hi", "more"], [alex, alex])), true);
    expect(b.speaker).toBe(a.speaker);
    expect(b.snap).not.toBe(a.snap);
  });

  it("a track with no lines has no speaker", () => {
    expect(speakerOfTrack(call([]))).toBe(NO_SPEAKER);
  });

  it("the log line names the kind only, never a name or a word", () => {
    const s = pickLiveSpeaker(snap(call(["secret plan"], [alex])), true);
    const line = liveSpeakerLog(s);
    expect(line).toBe("pill live_speaker shown=name");
    expect(line).not.toContain("Alex");
    expect(line).not.toContain("secret");
    expect(liveSpeakerLog(NO_SPEAKER)).toBe("pill live_speaker shown=none");
  });
});

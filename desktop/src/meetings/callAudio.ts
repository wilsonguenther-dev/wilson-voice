/**
 * yap-1106-call-audio-truth — the "Call audio" chip and the recorded-tracks
 * sentence. Pure, unit-tested, and the ONLY place a tap state becomes words.
 *
 * ## What Yap already does
 *
 * Yap hears the other side of every call with a Core Audio process tap —
 * global, excluding only Yap itself (`syscapture.rs`, YV100). No bot joins the
 * call, nothing is installed, no Screen Recording permission. It needs the
 * one-time "System Audio Recording" grant that YV102's setup step provokes.
 * What it did NOT do until this item is SAY whether that track is landing:
 * one real meeting had 62 Call lines, another had 0, and both looked the same
 * while they ran.
 *
 * ## The four tap states, from YV104's discriminator
 *
 * The backend ships numbers (`MeetingStatus.callAudio`), never a sentence, so
 * the pill and the recording bar render the SAME verdict:
 *
 * | tap                                  | chip     |
 * |--------------------------------------|----------|
 * | none (mic only, or an in-person room)| off      |
 * | open, never a non-zero sample        | silent   |
 * | open, delivering                     | on       |
 * | open, delivered then quiet / lost    | silent / off |
 *
 * "Silent" is never "denied": a tap that has heard nothing has told you the
 * other side has not spoken, not that macOS refused (finding OS-4, YV102).
 * The ONE action — re-run the setup step — is offered only when the tap has
 * never delivered AND the setup row holds no proof of the grant.
 */

import type { CallAudioLive, MeetingStatus } from "../pill/meeting";
import type { Meeting } from "../appTypes";

export type CallAudioChipState = "on" | "silent" | "off";

export interface CallAudioChip {
  state: CallAudioChipState;
  /** `Call audio: on` — the chip's text in the recording bar. */
  label: string;
  /** One sentence: the tooltip in the pill, inline in the recording bar. */
  detail: string;
  /** The one action, when there is one: re-run YV102's setup step. */
  action: "turn-on" | null;
}

/**
 * A tap that delivered sound and has been quiet this long reads "silent" — a
 * call on hold, on mute, or between sentences. Shorter and the chip flickers
 * on every pause; it is a state, not a VU meter.
 */
export const CALL_AUDIO_QUIET_AFTER_S = 20;

/**
 * A tap that has NEVER delivered, past this, with no proof of the grant, earns
 * the action. Longer than Rust's 3 s `DENIAL_GRACE` on purpose: that one decides
 * what to WRITE to the setup row after the meeting, this one decides when to
 * interrupt a person who may simply be first to speak.
 */
export const CALL_AUDIO_UNHEARD_ACTION_AFTER_S = 20;

export const CALL_AUDIO_ACTION_LABEL = "Turn on call audio";

function chip(state: CallAudioChipState, detail: string, action: "turn-on" | null = null): CallAudioChip {
  return { state, label: `Call audio: ${state}`, detail, action };
}

/**
 * The chip for a running meeting, or `null` when there is no meeting — or when
 * the backend predates the field, in which case the YV110 sentence
 * (`systemAudioBadge`) still carries the mic-only case and nothing is guessed.
 */
export function callAudioChip(status: MeetingStatus | null | undefined): CallAudioChip | null {
  if (!status || !status.recording) return null;
  const live = status.callAudio;
  if (!live) return null;
  return chipFor(live);
}

/** The rule itself, over the four tap states. Exported for the tests. */
export function chipFor(live: CallAudioLive): CallAudioChip {
  if (!live.attached) {
    if (!live.planned) {
      return chip("off", "In-person meeting — the room is on your microphone.");
    }
    return live.grantUnknown
      ? chip(
          "off",
          "This meeting is recording your microphone only. Turn on call audio once and the next call gets both sides.",
          "turn-on",
        )
      : chip("off", "This meeting is recording your microphone only.");
  }
  if (live.lost) {
    return chip("off", "Call audio stopped reaching Yap. Your microphone is still recording.");
  }
  if (!live.everNonzero) {
    if (live.grantUnknown && live.silentSeconds >= CALL_AUDIO_UNHEARD_ACTION_AFTER_S) {
      return chip(
        "silent",
        "No sound from the call has reached Yap yet, and macOS has not confirmed call audio for Yap.",
        "turn-on",
      );
    }
    return chip("silent", "No sound from the call has reached Yap yet — the other side may not have spoken.");
  }
  if (live.silentSeconds >= CALL_AUDIO_QUIET_AFTER_S) {
    return chip("silent", `Call audio reached Yap and has been quiet for ${live.silentSeconds}s.`);
  }
  return chip("on", "Yap is hearing the call's sound from your Mac.");
}

// ─── The meeting detail header: which tracks were recorded ──────────────────

export interface RecordedTracks {
  /** `Your mic + call audio` or `Mic only: …` — the header's short line. */
  label: string;
  /** Why, when there is a why. `null` for the two-track case. */
  detail: string | null;
  twoTrack: boolean;
}

export const TRACKS_FALLBACK_WHY =
  "Yap did not get the call's sound from your Mac for this meeting. Run \"Set up meeting recording\" on the Meetings page before the next call.";

/**
 * Which tracks a stored meeting has, from the row itself: a system-audio wav
 * is a second track, its absence is mic only, and the stop path's note
 * (`meeting.error`) is the reason when the engine had one.
 */
export function recordedTracks(
  meeting: Pick<Meeting, "state" | "kind" | "error"> & { sysWavPath?: string | null },
): RecordedTracks {
  if (meeting.sysWavPath) {
    return { label: "Your mic + call audio", detail: null, twoTrack: true };
  }
  if (meeting.state === "recording") {
    return { label: "Recording now — tracks are listed when it stops", detail: null, twoTrack: false };
  }
  if (meeting.kind === "in_person") {
    return {
      label: "Mic only: in-person meeting",
      detail: "The room was on your microphone; there was no call to hear.",
      twoTrack: false,
    };
  }
  const why = meeting.error && meeting.error.trim().length > 0 ? meeting.error.trim() : TRACKS_FALLBACK_WHY;
  return { label: "Mic only: call audio was not captured", detail: why, twoTrack: false };
}

import { useEffect, useState } from "react";
import { useAppCtx } from "../../appShell";
import { VOICE_OPTIONS, VOICES, voiceFor } from "../../pill/voices";
import {
  CHATTER_OPTIONS, chatterSentence, chatterStatus, parseChatterMode, parseTakeCount, takeCountLabel, TAKES_SEEN_KEY,
} from "../../pill/chatter";

/**
 * Who sits in the pill (yap-1106-main-character-switch, companions.html
 * section 04): Yappy (default), Hoot, Tidy, Gulp, Shelly. Ember, Sal and Mochi
 * are room-only and never offered. Each id is a registered character
 * (pill/characters/registry.ts `PILL_CHARACTER_IDS`), so all 17 pill states
 * are covered by the character contract. yap-1106-retire-classic-pill:
 * Classic is retired, so it is not offered; a saved "classic" (or nothing
 * saved) is migrated to "yappy" at launch and reads as Yappy here.
 */
export const PILL_STYLE_CHOICES = [
  ["yappy", "Yappy", "Scribe. Types everything you say"],
  ["hoot", "Hoot", "Meeting engineer. Dry, nods more than he talks"],
  ["tidy", "Tidy", "Copy editor. Precise, sweeps the ums"],
  ["gulp", "Gulp", "Word keeper. Loves a new word"],
  ["shelly", "Shelly", "Power manager. Slow, steady, smug"],
] as const;

/** One structured log line when the main character changes (ids only). */
export function pillStyleLogLine(from: string | null | undefined, to: string): string {
  const clean = (v: string | null | undefined) => String(v ?? "none").replace(/[^a-z0-9-]/gi, "").slice(0, 24) || "none";
  return `settings main_character_set from=${clean(from)} to=${clean(to)}`;
}

/** The choice to highlight: the saved style when it is still offered, else Yappy. */
export function activePillStyle(saved?: string | null): string {
  return PILL_STYLE_CHOICES.some(([id]) => id === saved) ? (saved as string) : PILL_STYLE_CHOICES[0][0];
}

/**
 * yap-1106-chatter-setting — the pill's take count (the number that drives
 * "less over time"), read from the localStorage the pill and Settings share.
 * Kept current by the cross-window `storage` event and a re-read on focus.
 * Storage failing reads as 0, never an error on screen.
 */
export function readTakesSeen(store: Pick<Storage, "getItem"> | undefined = globalThis.localStorage): number {
  try { return parseTakeCount(store?.getItem(TAKES_SEEN_KEY)); }
  catch (e) { console.warn("settings chatter takes_read_failed", e); return 0; }
}

function useTakesSeen(): number {
  const [takes, setTakes] = useState(() => readTakesSeen());
  useEffect(() => {
    const refresh = () => setTakes(readTakesSeen());
    const onStorage = (e: StorageEvent) => { if (e.key === null || e.key === TAKES_SEEN_KEY) refresh(); };
    window.addEventListener("storage", onStorage);
    window.addEventListener("focus", refresh);
    return () => { window.removeEventListener("storage", onStorage); window.removeEventListener("focus", refresh); };
  }, []);
  return takes;
}

/** yap-1106-curse-setting — the curse toggle is offered only while the voice is Badass. */
export function curseToggleShown(companionTone?: string | null): boolean {
  return voiceFor(companionTone) === "badass";
}

export default function Companion() {
  const {
    saveSettings, settings,
  } = useAppCtx();
  const takes = useTakesSeen();
  if (!settings) return null;
  const chatter = parseChatterMode(settings.yappyTalks);
  const chatterNow = chatterStatus(chatter, takes);
  return (
                <section className="settings-panel" data-settings-pane="companion">
                  <div className="panel" data-row>
                    <h3 id="pill-main-heading">Who sits in the pill</h3>
                    {/* yap-1106-main-character-switch — jobs never move: meetings
                        stay Hoot's, and Yappy, when someone else sits here, stays
                        at the studio typewriter and visits only for milestones. */}
                    <div className="profile-row" role="group" aria-labelledby="pill-main-heading">
                      {PILL_STYLE_CHOICES.map(([id, label, blurb]) => {
                        const selected = activePillStyle(settings.pillStyle) === id;
                        return (
                          <button
                            key={id}
                            type="button"
                            aria-pressed={selected}
                            data-main-character={id}
                            className={selected ? "profile active" : "profile"}
                            onClick={() => {
                              if (selected) return;
                              console.info(pillStyleLogLine(settings.pillStyle, id));
                              saveSettings({ ...settings, pillStyle: id });
                            }}
                          >
                            <strong>{label}</strong>
                            <span>{blurb}</span>
                          </button>
                        );
                      })}
                    </div>
                    <p className="settings-hint">
                      {activePillStyle(settings.pillStyle) === "yappy"
                        ? "Yappy appears while you dictate. Hoot still comes over to record meetings."
                        : activePillStyle(settings.pillStyle) === "hoot"
                          ? "Hoot records your meetings himself, headphones on. Yappy stays in the studio and visits for your milestones."
                          : "Hoot still comes over to record meetings. Yappy stays in the studio and visits for your milestones."}
                    </p>
                  </div>
                  <div className="panel" data-row>
                    <h3>Screen position</h3>
                    <div className="profile-row">
                      {(
                        [
                          ["bottom", "Bottom", "Centred along the screen bottom"],
                          ["left", "Left edge", "Docked left, halfway down"],
                          ["right", "Right edge", "Docked right, halfway down"],
                        ] as const
                      ).map(([id, label, blurb]) => (
                        <button
                          key={id}
                          type="button"
                          className={
                            (settings.pillPosition ?? "bottom") === id
                              ? "profile active"
                              : "profile"
                          }
                          onClick={() =>
                            saveSettings({ ...settings, pillPosition: id })
                          }
                        >
                          <strong>{label}</strong>
                          <span>{blurb}</span>
                        </button>
                      ))}
                    </div>
                    <p className="settings-hint">The side docks pin it halfway down that edge, out of the way of your text.</p>
                  </div>
                  <div className="panel" data-row>
                    <h3 id="yappy-voice-heading">Yappy's voice</h3>
                    {/* yap-1106-voice-modes — Best friend, Badass, Professional, plus
                        Rose as a fourth. The stored ids stay friendly/rude/rose so an
                        existing choice carries over; "professional" is new. */}
                    <div className="profile-row" role="group" aria-labelledby="yappy-voice-heading">
                      {VOICE_OPTIONS.map(([id, voice]) => {
                        const selected = voiceFor(settings.companionTone) === voice;
                        return (
                          <button
                            key={id}
                            type="button"
                            aria-pressed={selected}
                            data-voice={voice}
                            className={selected ? "profile active" : "profile"}
                            onClick={() => {
                              console.info(`settings voice_set voice=${voice}`);
                              saveSettings({ ...settings, companionTone: id });
                            }}
                          >
                            <strong>{VOICES[voice].name}</strong>
                            <span>{VOICES[voice].blurb}</span>
                          </button>
                        );
                      })}
                    </div>
                    <p className="settings-hint">What Yappy says while you talk and when you finish. It applies to whichever companion sits in the pill, and each friend adds a few lines of their own.</p>
                    {/* yap-1106-curse-setting — the curse toggle exists only under
                        Badass and is off by default. Switching to another voice
                        hides it; the saved choice waits for Badass again. */}
                    {curseToggleShown(settings.companionTone) && (
                      <div className="curse-setting" data-curse-setting>
                        <label className="toggle">
                          <input
                            type="checkbox"
                            data-curse-toggle
                            checked={settings.companionCurse === true}
                            onChange={(e) => {
                              console.info(`settings voice_curse_set on=${e.target.checked}`);
                              saveSettings({ ...settings, companionCurse: e.target.checked });
                            }}
                          />
                          <span>Let Badass swear</span>
                        </label>
                        <p className="settings-hint">About one line in four, the strongest word at most once a day. Never during a meeting or a call.</p>
                      </div>
                    )}
                  </div>
                  <div className="panel" data-row data-chatter-row>
                    <h3 id="yappy-talks-heading">Yappy talks</h3>
                    {/* yap-1106-chatter-setting (FB-21) — the 50-take quieting is a
                        visible, switchable setting now, explained in one line with the
                        take count that drives it right beside it. */}
                    <div className="profile-row" role="group" aria-labelledby="yappy-talks-heading" aria-describedby="yappy-talks-explain">
                      {CHATTER_OPTIONS.map(([id, label, blurb]) => {
                        const selected = chatter === id;
                        return (
                          <button
                            key={id}
                            type="button"
                            aria-pressed={selected}
                            data-chatter={id}
                            className={selected ? "profile active" : "profile"}
                            onClick={() => {
                              if (selected) return;
                              console.info(`settings chatter_set mode=${id} from=${chatter} takes=${takes}`);
                              saveSettings({ ...settings, yappyTalks: id });
                            }}
                          >
                            <strong>{label}</strong>
                            <span>{blurb}</span>
                          </button>
                        );
                      })}
                    </div>
                    <p className="settings-hint chatter-explain" id="yappy-talks-explain" data-chatter-sentence>
                      {chatterSentence(chatter)}{" "}
                      <span className="chatter-count" data-chatter-takes={takes}>
                        {takeCountLabel(takes)}{chatterNow ? ` · ${chatterNow}` : ""}
                      </span>
                    </p>
                  </div>
                  <div className="panel" data-row>
                  <h3>Always on screen</h3>
                  <label className="toggle">
                    <input
                      type="checkbox"
                      checked={settings.showFloatingPill ?? true}
                      onChange={(e) =>
                        saveSettings({
                          ...settings,
                          showFloatingPill: e.target.checked,
                        })
                      }
                    />
                    <span>Keep the companion on screen at all times</span>
                  </label>
                  <p className="settings-hint">Off, it only appears while you talk.</p>
                  </div>
                </section>
  );
}

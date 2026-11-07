/**
 * Y10-A — "Languages I speak": the Settings half of the language picker (the
 * bar's chip is the other half, src/pill/LanguageChip.tsx). Offers only what
 * the installed model can transcribe, disables Auto-detect when the model
 * cannot detect, and shows the backend's notice in plain words.
 */
import { toggleLanguage, type LanguagePrefs } from "./languagePicker";
import { languageStore, useLanguageStore } from "./languageStore";

export default function LanguageSettings() {
  // WF-11 — the same store the bar's chip reads; the tray's Languages
  // submenu writes through the same backend writer, so all three agree.
  const { opts, error } = useLanguageStore();
  const write = (next: LanguagePrefs) => {
    void languageStore.write(next);
  };
  if (!opts) {
    return error ? <p className="settings-hint" role="alert">{error}</p> : null;
  }
  const { prefs } = opts;
  const canDetect = opts.supportsDetect !== false;
  return (
    <div className="field" data-languages>
      <span>Languages I speak</span>
      <div className="profile-row" role="group" aria-label="Languages I speak">
        {opts.offered.map((c) => (
          <label key={c.code} className="toggle">
            <input
              type="checkbox"
              checked={prefs.set.includes(c.code)}
              disabled={!prefs.set.includes(c.code) && prefs.set.length >= opts.maxSet}
              onChange={() => write(toggleLanguage(prefs, c.code, opts.maxSet))}
            />
            <span>{c.name}</span>
          </label>
        ))}
      </div>
      {prefs.set.length > 1 ? (
        <>
          <label className="toggle">
            <input
              type="checkbox"
              checked={prefs.auto && canDetect}
              disabled={!canDetect}
              onChange={(e) => write({ ...prefs, auto: e.target.checked })}
            />
            <span>Detect which of these I'm speaking</span>
          </label>
          <label className="field">
            <span>Otherwise use</span>
            <select
              value={prefs.primary}
              onChange={(e) => write({ ...prefs, primary: e.target.value })}
            >
              {prefs.set.map((code) => (
                <option key={code} value={code}>
                  {opts.offered.find((c) => c.code === code)?.name ?? code}
                </option>
              ))}
            </select>
          </label>
        </>
      ) : null}
      {opts.notice ? <p className="settings-hint" role="status">{opts.notice}</p> : null}
      <p className="settings-hint">
        Up to {opts.maxSet}. Spoken punctuation, filler clean-up and the polish model are English-only, so a take in another language is pasted as you said it.
      </p>
      {error ? <p className="settings-hint" role="alert">{error}</p> : null}
    </div>
  );
}

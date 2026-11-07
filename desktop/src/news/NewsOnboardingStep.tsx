// yap-1106-news-prefs-onboarding — the onboarding step "What do you want to
// hear about?". OnboardingFlow only routes here when the cloud option is
// connected and the offer was never answered (`newsStepDue`); otherwise the
// step does not exist and the one-time Home card offers it later.
// "Not now" skips it (nothing saved, the offer is marked declined, Settings →
// Yappy News stays the way back in). "Keep me posted" saves and moves on.
import { useState } from "react";
import { errorText } from "../errors";
import NewsInterests from "./NewsInterests";
import { EMPTY_PREFS, NEWS_PRIVACY_LINE, dismissNews, hasAnyTopic, saveNews, type NewsPrefs } from "./newsPrefs";

export function NewsStepView({
  value,
  onChange,
  busy,
  error,
  onSkip,
  onSave,
  onBack,
}: {
  value: NewsPrefs;
  onChange: (p: NewsPrefs) => void;
  busy: boolean;
  error: string | null;
  onSkip: () => void;
  onSave: () => void;
  onBack: () => void;
}) {
  return (
    <div className="onboard-step" data-onboard-step="news">
      <h1>What do you want to hear about?</h1>
      <p className="onboard-lede">
        Twice a day Yappy can tell you what happened in the worlds you care about, one line per story, with a link to
        the original. Pick a few now or skip it; you can change this any time in Settings → Yappy News.
      </p>
      <NewsInterests value={value} onChange={onChange} disabled={busy} />
      <p className="muted tiny">{NEWS_PRIVACY_LINE}</p>
      {error && (
        <p className="onboard-error" role="alert">
          {error}
        </p>
      )}
      <div className="onboard-actions">
        <button className="ghost" onClick={onBack} disabled={busy}>
          Back
        </button>
        <button className="ghost" onClick={onSkip} disabled={busy} data-news-skip>
          Not now
        </button>
        <button className="primary" onClick={onSave} disabled={busy || !hasAnyTopic(value)} data-news-save>
          {busy ? "Saving…" : "Keep me posted"}
        </button>
      </div>
    </div>
  );
}

export default function NewsOnboardingStep({ onNext, onBack }: { onNext: () => void; onBack: () => void }) {
  const [value, setValue] = useState<NewsPrefs>(EMPTY_PREFS);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);

  async function skip() {
    setBusy(true);
    try {
      await dismissNews("onboarding");
    } catch (e) {
      // Skipping must never trap the user in onboarding: the worst case of a
      // failed write is that the Home card offers it once more.
      console.warn("news onboarding dismiss failed", errorText(e));
    } finally {
      setBusy(false);
    }
    onNext();
  }

  async function save() {
    setBusy(true);
    setError(null);
    try {
      await saveNews({ ...value, enabled: true }, "onboarding");
      setBusy(false);
      onNext();
    } catch (e) {
      setBusy(false);
      setError(`${errorText(e)} Try again, or choose Not now and set it up later in Settings.`);
    }
  }

  return (
    <NewsStepView
      value={value}
      onChange={setValue}
      busy={busy}
      error={error}
      onSkip={() => void skip()}
      onSave={() => void save()}
      onBack={onBack}
    />
  );
}

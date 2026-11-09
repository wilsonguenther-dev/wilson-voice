// yap-1106-news-prefs-onboarding — the onboarding step "What do you want to
// hear about?". OnboardingFlow only routes here when the cloud option is
// connected and the offer was never answered (`newsStepDue`); otherwise the
// step does not exist and the one-time Home card offers it later.
// yap-1107-parity-news: the layout follows the approved page, measured.
// "Not now" skips it (nothing saved, the offer is marked declined, Settings →
// Yappy News stays the way back in). "Keep me posted" saves and moves on.
import { useEffect, useRef, useState } from "react";
import { errorText } from "../errors";
import { FRIENDS } from "../sprites/art";
import { domBuffer, paintCrisp } from "../sprites/crisp";
import NewsInterests from "./NewsInterests";
import { EMPTY_PREFS, NEWS_PRIVACY_LINE, dismissNews, hasAnyTopic, loadNews, prefsOf, saveNews, type NewsPrefs } from "./newsPrefs";

/* yap-1107-parity-news — the approved page (design-2026-11-06/news.html §01)
   draws this step as: a mono "Step N of M" line, Yappy at 72 px beside the
   heading "Want me to keep you posted?" and a one-line lede, the ten chips, the
   two fields, the privacy line, and a footer with Not now (left) and Keep me
   posted (right). No Back button is drawn, so Back is the Escape key. The page
   says "Step 5 of 6"; the real spine is welcome, permissions, calibration,
   news, try-it, done, so the label is computed from it (4 of 6) instead of
   printed from the picture. */
export const NEWS_STEP_HEADING = "Want me to keep you posted?";
export const NEWS_STEP_LEDE =
  "Pick what you care about. Twice a day I'll read the news and tell you what matters, in one line each.";
/** Yappy's box beside the heading, CSS px (the page's 72x72 canvas). */
const YAPPY_BOX = { w: 72, h: 72 };

function StepYappy() {
  const ref = useRef<HTMLCanvasElement | null>(null);
  useEffect(() => {
    const c = ref.current;
    if (!c) return;
    paintCrisp<HTMLCanvasElement>(c, FRIENDS.yappy, 0, undefined, YAPPY_BOX, window.devicePixelRatio, domBuffer, "news-step-yappy");
  }, []);
  return <canvas ref={ref} className="news-onb-yappy" data-news-step-yappy aria-hidden="true" />;
}

export function NewsStepView({
  value,
  onChange,
  busy,
  error,
  onSkip,
  onSave,
  onBack,
  stepIndex = 4,
  stepCount = 6,
}: {
  value: NewsPrefs;
  onChange: (p: NewsPrefs) => void;
  busy: boolean;
  error: string | null;
  onSkip: () => void;
  onSave: () => void;
  /** Escape goes back; the approved page draws no Back button on this step. */
  onBack: () => void;
  stepIndex?: number;
  stepCount?: number;
}) {
  return (
    <div
      className="onboard-step news-onb"
      data-onboard-step="news"
      onKeyDown={(e) => {
        if (e.key === "Escape" && !busy) {
          e.preventDefault();
          onBack();
        }
      }}
    >
      <div className="news-onb-body">
        <span className="news-onb-step" data-news-step-count={stepCount}>
          Step {stepIndex} of {stepCount}
        </span>
        <div className="news-onb-row">
          <StepYappy />
          <div>
            <h1 className="news-onb-title">{NEWS_STEP_HEADING}</h1>
            <p className="news-onb-lede">{NEWS_STEP_LEDE}</p>
          </div>
        </div>
        <NewsInterests value={value} onChange={onChange} disabled={busy} variant="onboarding" />
        <p className="news-onb-privacy">{NEWS_PRIVACY_LINE}</p>
        {error && (
          <p className="onboard-error" role="alert">
            {error}
          </p>
        )}
      </div>
      <div className="onboard-actions news-onb-foot">
        <button className="news-onb-btn" onClick={onSkip} disabled={busy} data-news-skip>
          Not now
        </button>
        <button className="primary news-onb-btn news-onb-btn--primary" onClick={onSave} disabled={busy || !hasAnyTopic(value)} data-news-save>
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

  // yap-1107-parity-news — start from what is already saved (a pick made in
  // Settings before onboarding finished), so the step never shows an empty
  // picker over real choices. A failed read keeps the empty picker.
  useEffect(() => {
    let live = true;
    loadNews()
      .then((v) => {
        if (live && v && hasAnyTopic(v)) setValue(prefsOf(v));
      })
      .catch((e) => console.warn("news onboarding prefs_read_failed", errorText(e)));
    return () => {
      live = false;
    };
  }, []);

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

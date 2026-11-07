// yap-1106-news-prefs-onboarding — the one-time Home card. It appears only
// when the cloud option is connected and the news offer was never answered
// (onboarding ran before the cloud was on, so the step was skipped). Either
// answer — "Not now" or a save — resolves the offer for good; after that the
// only way in is Settings → Yappy News. A failed read shows nothing: this is
// an offer, not a feature anyone is waiting on.
import { useEffect, useState } from "react";
import { useAppCtx } from "../appShell";
import NewsInterests from "./NewsInterests";
import {
  EMPTY_PREFS,
  NEWS_PRIVACY_LINE,
  dismissNews,
  hasAnyTopic,
  homeCardDue,
  loadNews,
  saveNews,
  type NewsPrefs,
} from "./newsPrefs";

export function NewsHomeCardView({
  open,
  value,
  busy,
  onOpen,
  onChange,
  onSkip,
  onSave,
}: {
  open: boolean;
  value: NewsPrefs;
  busy: boolean;
  onOpen: () => void;
  onChange: (p: NewsPrefs) => void;
  onSkip: () => void;
  onSave: () => void;
}) {
  return (
    <section className="card news-offer" data-news-home-card aria-labelledby="news-offer-title">
      <h3 id="news-offer-title">Want Yappy to bring you the news?</h3>
      <p className="muted">
        Your cloud option is connected, so Yappy can bring you a short briefing on the topics you pick, twice a day,
        with a link to every story.
      </p>
      {open && (
        <>
          <NewsInterests value={value} onChange={onChange} disabled={busy} />
          <p className="muted tiny">{NEWS_PRIVACY_LINE}</p>
        </>
      )}
      <div className="news-offer-actions">
        <button className="ghost" onClick={onSkip} disabled={busy} data-news-skip>
          Not now
        </button>
        {open ? (
          <button className="primary" onClick={onSave} disabled={busy || !hasAnyTopic(value)} data-news-save>
            {busy ? "Saving…" : "Keep me posted"}
          </button>
        ) : (
          <button className="primary" onClick={onOpen} data-news-open>
            Pick topics
          </button>
        )}
      </div>
    </section>
  );
}

export default function NewsHomeCard() {
  const { toast, toastError } = useAppCtx();
  const [due, setDue] = useState(false);
  const [open, setOpen] = useState(false);
  const [value, setValue] = useState<NewsPrefs>(EMPTY_PREFS);
  const [busy, setBusy] = useState(false);

  useEffect(() => {
    let live = true;
    loadNews()
      .then((v) => {
        if (live) setDue(homeCardDue(v));
      })
      .catch((e) => console.warn("news home card read failed", e));
    return () => {
      live = false;
    };
  }, []);

  if (!due) return null;

  async function skip() {
    setBusy(true);
    try {
      await dismissNews("home");
      setDue(false);
    } catch (e) {
      toastError(e, "Yap couldn't save that. Try Not now again.");
    } finally {
      setBusy(false);
    }
  }

  async function save() {
    setBusy(true);
    try {
      await saveNews({ ...value, enabled: true }, "home");
      setDue(false);
      toast("Saved. Change your news topics any time in Settings → Yappy News.");
    } catch (e) {
      toastError(e, "Yap couldn't save your news topics. Try again in a moment.");
    } finally {
      setBusy(false);
    }
  }

  return (
    <NewsHomeCardView
      open={open}
      value={value}
      busy={busy}
      onOpen={() => setOpen(true)}
      onChange={setValue}
      onSkip={() => void skip()}
      onSave={() => void save()}
    />
  );
}

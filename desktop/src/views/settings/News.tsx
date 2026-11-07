// yap-1106-news-prefs-onboarding — Settings → Yappy News. The SAME record the
// onboarding step and the one-time Home card write (`news_prefs.rs`). Shows
// on or off, the topics, and the last refresh time, plus the one privacy
// sentence. With the cloud option off the feature does not exist: the switch
// and picker are disabled and the one action is the way to connect it.
//
// BEFORE/AFTER PNGs (integrator, headless only — never the real app): with the
// Vite dev server up, run YAP_SMOKE_VIEWS=Settings node scripts/smoke-windowed.mjs
// on origin/main (before: no Yappy News tab) and on this branch (after);
// `captureSettingsTabs` clicks every tab, so the new pane lands as
// settings-news-<size>.png.
import { useEffect, useState, type ReactNode } from "react";
import { useAppCtx } from "../../appShell";
import { errorText } from "../../errors";
import NewsInterests from "../../news/NewsInterests";
import NewsQuoteKey from "../../news/NewsQuoteKey";
import {
  NEWS_PRIVACY_LINE,
  lastRefreshLine,
  loadNews,
  prefsOf,
  samePrefs,
  saveNews,
  statusLine,
  topicLabels,
  type NewsPrefs,
  type NewsView,
} from "../../news/newsPrefs";

export function NewsSettingsView({
  view,
  draft,
  saving,
  now,
  onChange,
  onSave,
  onRevert,
  onConnect,
  advanced,
}: {
  view: NewsView;
  draft: NewsPrefs;
  saving: boolean;
  now?: number;
  onChange: (p: NewsPrefs) => void;
  onSave: () => void;
  onRevert: () => void;
  onConnect: () => void;
  /** yap-1106-fb-14-news-quote-key — the Advanced section (optional quote key). */
  advanced?: ReactNode;
}) {
  const dirty = !samePrefs(draft, prefsOf(view));
  const saved = topicLabels(view);
  return (
    <section className="settings-panel" data-settings-pane="news">
      <div className="panel" data-row>
        <h3>Briefing</h3>
        <p data-news-status={view.active ? "on" : "off"}>
          <strong>{view.active ? "On" : "Off"}</strong> · {statusLine(view)}
        </p>
        <label className="toggle">
          <input
            type="checkbox"
            checked={draft.enabled}
            disabled={!view.cloudConnected || saving}
            onChange={(e) => onChange({ ...draft, enabled: e.target.checked })}
          />
          <span>
            <strong>Bring me the news</strong> — a short briefing on Home, one line per story, with a link to each.
          </span>
        </label>
        {!view.cloudConnected && (
          <p className="settings-hint">
            Yappy News needs the cloud option.{" "}
            <button type="button" className="ghost" onClick={onConnect}>
              Connect it in Advanced
            </button>
          </p>
        )}
        <p className="muted tiny" data-news-last-refresh>
          {lastRefreshLine(view.lastRefreshMs, now)}
        </p>
      </div>

      <div className="panel" data-row>
        <h3>Your topics</h3>
        <p className="muted" data-news-topics>
          {saved.length > 0 ? saved.join(", ") : "No topics yet."}
        </p>
        <NewsInterests value={draft} onChange={onChange} disabled={!view.cloudConnected || saving} />
        <p className="settings-hint">{NEWS_PRIVACY_LINE}</p>
      </div>

      {advanced}

      <div className="settings-footer">
        <button type="button" className="ghost" onClick={onRevert} disabled={!dirty || saving}>
          Undo changes
        </button>
        <button type="button" className="primary" onClick={onSave} disabled={!dirty || saving} data-news-save>
          {saving ? "Saving…" : "Save news settings"}
        </button>
      </div>
    </section>
  );
}

export default function News() {
  const { setSettingsTab, toast, toastError } = useAppCtx();
  const [view, setView] = useState<NewsView | null>(null);
  const [draft, setDraft] = useState<NewsPrefs | null>(null);
  const [loadError, setLoadError] = useState<string | null>(null);
  const [saving, setSaving] = useState(false);

  function read() {
    setLoadError(null);
    loadNews()
      .then((v) => {
        setView(v);
        setDraft(prefsOf(v));
      })
      .catch((e) => setLoadError(errorText(e)));
  }

  useEffect(read, []);

  async function save() {
    if (!draft) return;
    setSaving(true);
    try {
      const v = await saveNews(draft, "settings");
      setView(v);
      setDraft(prefsOf(v));
      toast("News settings saved.");
    } catch (e) {
      toastError(e, "Yap couldn't save your news settings. Try again in a moment.");
    } finally {
      setSaving(false);
    }
  }

  if (loadError)
    return (
      <section className="settings-panel" data-settings-pane="news">
        <div className="panel" data-row>
          <h3>Briefing</h3>
          <p className="onboard-error" role="alert">
            Yap couldn't read your news settings. {loadError}
          </p>
          <button type="button" className="ghost" onClick={read}>
            Read again
          </button>
        </div>
      </section>
    );
  if (!view || !draft)
    return (
      <section className="settings-panel" data-settings-pane="news">
        <div className="panel" data-row aria-busy="true">
          <h3>Briefing</h3>
          <p className="muted">Reading your news settings…</p>
        </div>
      </section>
    );
  return (
    <NewsSettingsView
      view={view}
      draft={draft}
      saving={saving}
      onChange={setDraft}
      onSave={() => void save()}
      onRevert={() => setDraft(prefsOf(view))}
      onConnect={() => setSettingsTab("advanced")}
      advanced={<NewsQuoteKey />}
    />
  );
}

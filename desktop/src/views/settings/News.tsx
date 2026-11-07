// yap-1106-news-prefs-onboarding — Settings → Yappy News. The SAME record the
// onboarding step and the one-time Home card write (`news_prefs.rs`). Shows
// on or off, "How often" (yap-1106-news-refresh-setting) with the last refresh
// time next to it, the topics, plus the one privacy
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
  CADENCE_OPTIONS,
  NEWS_PRIVACY_LINE,
  cadenceOf,
  lastRefreshLine,
  loadNews,
  nextTapAllowedAt,
  refreshNews,
  saveCadence,
  prefsOf,
  revealCloudRoute,
  samePrefs,
  saveNews,
  statusLine,
  topicLabels,
  watchCloudRoute,
  type NewsCadence,
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
  onCadence,
  cadenceSaving = false,
  onRefresh,
  refreshing = false,
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
  /** yap-1106-news-refresh-setting — "How often" saves on pick. */
  onCadence?: (c: NewsCadence) => void;
  cadenceSaving?: boolean;
  /** "Refresh now" (a tap; the 30-minute floor still holds). */
  onRefresh?: () => void;
  refreshing?: boolean;
}) {
  const dirty = !samePrefs(draft, prefsOf(view));
  const saved = topicLabels(view);
  const cadence = cadenceOf(view);
  const waitUntil = nextTapAllowedAt(view.lastRefreshMs, now);
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
      </div>

      <fieldset className="panel news-cadence" data-row data-news-cadence={cadence}>
        <legend>
          <h3>How often</h3>
        </legend>
        {CADENCE_OPTIONS.map((o) => (
          <label key={o.id} className="news-cadence-option">
            <input
              type="radio"
              name="news-cadence"
              value={o.id}
              checked={cadence === o.id}
              disabled={!view.cloudConnected || cadenceSaving || !onCadence}
              onChange={() => onCadence?.(o.id)}
            />
            <span>
              <strong>{o.label}</strong>
              <span className="muted tiny"> — {o.hint}</span>
            </span>
          </label>
        ))}
        <p className="settings-hint">Never more often than every 30 minutes, and never during a meeting, a screen share, Do Not Disturb or low battery.</p>
        <div className="news-cadence-last">
          <p className="muted tiny" data-news-last-refresh aria-live="polite">
            {lastRefreshLine(view.lastRefreshMs, now)}
          </p>
          {onRefresh && (
            <button
              type="button"
              className="ghost"
              onClick={onRefresh}
              disabled={!view.active || refreshing || waitUntil != null}
              title={waitUntil != null ? "News was refreshed less than 30 minutes ago." : undefined}
              data-news-refresh
            >
              {refreshing ? "Refreshing…" : "Refresh now"}
            </button>
          )}
        </div>
      </fieldset>

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
  const [cadenceSaving, setCadenceSaving] = useState(false);
  const [refreshing, setRefreshing] = useState(false);

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

  // yap-1107-news-honours-cloud-key — a cloud key saved or removed (or the
  // mode changed) in Advanced re-reads this view in place: the switch and the
  // topic picker turn on (or off) without a remount. Unsaved topic edits are
  // kept; only the server half (cloud, active, last refresh) moves.
  useEffect(
    () =>
      watchCloudRoute((v) => {
        setLoadError(null);
        setView(v);
        setDraft((d) => d ?? prefsOf(v));
      }),
    [],
  );

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

  // yap-1106-news-refresh-setting — "How often" is its own record, saved on
  // pick, so an unsaved topic draft is untouched.
  async function pickCadence(c: NewsCadence) {
    if (!view || c === cadenceOf(view)) return;
    setCadenceSaving(true);
    try {
      const v = await saveCadence(c);
      setView(v);
      toast("News refresh setting saved.");
    } catch (e) {
      toastError(e, "Yap couldn't save how often news refreshes. Try again in a moment.");
    } finally {
      setCadenceSaving(false);
    }
  }

  async function refreshNow() {
    setRefreshing(true);
    try {
      const out = await refreshNews("tap");
      setView(out.view);
      toast(out.ran ? "News refreshed." : (out.message ?? "News isn't due yet."));
    } catch (e) {
      toastError(e, "Yap couldn't refresh the news. Try again in a moment.");
    } finally {
      setRefreshing(false);
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
      onConnect={() => {
        setSettingsTab("advanced");
        void revealCloudRoute();
      }}
      advanced={<NewsQuoteKey />}
      onCadence={(c) => void pickCadence(c)}
      cadenceSaving={cadenceSaving}
      onRefresh={() => void refreshNow()}
      refreshing={refreshing}
    />
  );
}

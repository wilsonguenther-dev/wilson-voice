// yap-1106-fb-14-news-quote-key — Settings → Yappy News → Advanced: an
// optional own key for the quote feed. Nothing needs a key: with none, the
// free tier applies (tickers show as headlines from the public feeds). With a
// Finnhub or Alpha Vantage key, tickers get prices at that provider's limit,
// never more than once per ticker per 30 minutes. The key goes to the
// Keychain; this screen only ever shows "Key saved" and its last four
// characters, and the field is cleared the moment it is saved.
//
// BEFORE/AFTER PNGs (integrator, headless only — never the real app): with the
// Vite dev server up, YAP_SMOKE_VIEWS=Settings node scripts/smoke-windowed.mjs
// on loop3/integration-8 (before: no Advanced section) and on this branch
// (after: open the Advanced disclosure in settings-news-<size>.png).
import { useEffect, useId, useState } from "react";
import { useAppCtx } from "../appShell";
import { errorText } from "../errors";
import {
  QUOTE_KEY_LEAVES_LABEL,
  clearQuoteKey,
  initialQuoteProvider,
  loadQuoteKey,
  quoteTierLine,
  saveQuoteKey,
  type QuoteKeyView,
  type QuoteProvider,
} from "./newsPrefs";

export function NewsQuoteKeyPanel({
  view,
  busy,
  onSave,
  onClear,
}: {
  view: QuoteKeyView;
  busy: boolean;
  onSave: (provider: QuoteProvider, key: string) => Promise<boolean>;
  onClear: () => void;
}) {
  const [provider, setProvider] = useState<QuoteProvider>(initialQuoteProvider(view));
  const [key, setKey] = useState("");
  const fieldId = useId();
  const chosen = view.providers.find((p) => p.id === provider);
  const canSave = key.trim().length > 0 && !busy;

  async function submit() {
    if (!canSave) return;
    if (await onSave(provider, key)) setKey("");
  }

  return (
    <div className="news-quote-key" data-news-quote-key data-tier={view.tier}>
      <h4>Stock prices</h4>
      <p className="muted" data-news-quote-tier>
        {quoteTierLine(view)}
      </p>
      {view.error && (
        <p className="onboard-error" role="alert">
          {view.error}
        </p>
      )}
      <fieldset className="news-quote-providers" disabled={busy}>
        <legend>Optional: your own quote key</legend>
        {view.providers.map((p) => (
          <label key={p.id} className="toggle inline">
            <input
              type="radio"
              name="news-quote-provider"
              value={p.id}
              checked={provider === p.id}
              onChange={() => setProvider(p.id)}
            />
            <span>{p.label}</span>
          </label>
        ))}
      </fieldset>
      <label htmlFor={fieldId} className="news-quote-field">
        <span>{chosen ? `${chosen.label} key` : "Quote key"}</span>
        <input
          id={fieldId}
          type="password"
          autoComplete="off"
          spellCheck={false}
          value={key}
          disabled={busy}
          placeholder={view.saved && view.provider === provider ? `Key saved, ending ${view.hint}` : "Paste your key"}
          onChange={(e) => setKey(e.target.value)}
          onKeyDown={(e) => {
            if (e.key === "Enter") {
              e.preventDefault();
              void submit();
            }
          }}
          data-news-quote-input
        />
      </label>
      <p className="settings-hint" data-news-quote-leaves>
        <strong>{view.leavesLabel || QUOTE_KEY_LEAVES_LABEL}</strong>
        {chosen ? ` (${chosen.host}). Stored in your Keychain.` : ". Stored in your Keychain."}
      </p>
      <div className="news-quote-actions">
        {view.saved && (
          <button type="button" className="ghost" onClick={onClear} disabled={busy} data-news-quote-clear>
            Remove key
          </button>
        )}
        <button type="button" className="primary" onClick={() => void submit()} disabled={!canSave} data-news-quote-save>
          {busy ? "Saving…" : view.saved ? "Replace key" : "Save key"}
        </button>
      </div>
    </div>
  );
}

export default function NewsQuoteKey() {
  const { toast, toastError } = useAppCtx();
  const [view, setView] = useState<QuoteKeyView | null>(null);
  const [loadError, setLoadError] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);

  function read() {
    setLoadError(null);
    loadQuoteKey()
      .then(setView)
      .catch((e) => setLoadError(errorText(e)));
  }

  useEffect(read, []);

  async function save(provider: QuoteProvider, key: string): Promise<boolean> {
    setBusy(true);
    try {
      setView(await saveQuoteKey(provider, key));
      toast("Quote key saved in your Keychain.");
      return true;
    } catch (e) {
      toastError(e, "Yap couldn't save the quote key. Check it and try again.");
      return false;
    } finally {
      setBusy(false);
    }
  }

  async function clear() {
    setBusy(true);
    try {
      setView(await clearQuoteKey());
      toast("Quote key removed. Tickers use the free sources again.");
    } catch (e) {
      toastError(e, "Yap couldn't remove the quote key. Try again in a moment.");
    } finally {
      setBusy(false);
    }
  }

  return (
    <details className="panel news-advanced" data-row data-news-advanced>
      <summary>
        <h3>Advanced</h3>
      </summary>
      {loadError ? (
        <div>
          <p className="onboard-error" role="alert">
            Yap couldn't read your quote key settings. {loadError}
          </p>
          <button type="button" className="ghost" onClick={read}>
            Read again
          </button>
        </div>
      ) : !view ? (
        <p className="muted" aria-busy="true">
          Reading your quote key settings…
        </p>
      ) : (
        <NewsQuoteKeyPanel view={view} busy={busy} onSave={save} onClear={() => void clear()} />
      )}
    </details>
  );
}

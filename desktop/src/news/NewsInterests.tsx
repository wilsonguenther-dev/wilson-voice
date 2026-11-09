// yap-1106-news-prefs-onboarding — the ONE interest picker. The onboarding
// step, the one-time Home card and Settings → Yappy News all render this, so
// the three can never drift apart. Controlled: the parent owns the prefs and
// decides when to save. Keyboard: chips are buttons (Space/Enter toggle,
// aria-pressed), each text field adds on Enter, every saved token has its own
// labelled remove button.
import { useState } from "react";
import {
  TOPIC_CHIPS,
  addCustom,
  addTicker,
  removeCustom,
  removeTicker,
  toggleTopic,
  type NewsPrefs,
} from "./newsPrefs";

export default function NewsInterests({
  value,
  onChange,
  disabled = false,
  variant = "stacked",
}: {
  value: NewsPrefs;
  onChange: (next: NewsPrefs) => void;
  disabled?: boolean;
  /** yap-1107-parity-news — "onboarding" draws the approved page's step: the
   *  chips with no heading, then two one-line fields (tickers with their tokens
   *  inline, then "Anything else?"). "stacked" is Settings and the Home card. */
  variant?: "stacked" | "onboarding";
}) {
  const [ticker, setTicker] = useState("");
  const [custom, setCustom] = useState("");
  const [error, setError] = useState<string | null>(null);

  function add(kind: "ticker" | "custom") {
    const raw = kind === "ticker" ? ticker : custom;
    if (!raw.trim()) return;
    const r = kind === "ticker" ? addTicker(value, raw) : addCustom(value, raw);
    if (!r.ok) {
      setError(r.error);
      return;
    }
    setError(null);
    onChange(r.prefs);
    if (kind === "ticker") setTicker("");
    else setCustom("");
  }

  if (variant === "onboarding") {
    return (
      <div className="news-interests news-interests--onboarding" data-news-interests data-news-variant="onboarding">
        <fieldset className="news-group" disabled={disabled}>
          <legend className="sr-only">Topics</legend>
          <div className="news-chips" role="group" aria-label="Topics">
            {TOPIC_CHIPS.map((c) => {
              const on = value.topics.includes(c.id);
              return (
                <button
                  key={c.id}
                  type="button"
                  className={on ? "chip selected" : "chip"}
                  aria-pressed={on}
                  data-news-chip={c.id}
                  onClick={() => onChange(toggleTopic(value, c.id))}
                >
                  {c.label}
                </button>
              );
            })}
          </div>
        </fieldset>
        <fieldset className="news-field" disabled={disabled} data-news-field="tickers">
          <legend className="sr-only">Tickers you follow</legend>
          <span className="news-field-label" aria-hidden="true">
            Tickers you follow
          </span>
          {value.tickers.map((t) => (
            <button
              key={t}
              type="button"
              className="news-field-token"
              aria-label={`Remove ${t}`}
              title={`Remove ${t}`}
              onClick={() => onChange(removeTicker(value, t))}
            >
              {t}
            </button>
          ))}
          <input
            type="text"
            className="news-field-input"
            aria-label="Add a ticker"
            value={ticker}
            maxLength={12}
            autoCapitalize="characters"
            spellCheck={false}
            onChange={(e) => setTicker(e.target.value)}
            onKeyDown={(e) => {
              if (e.key === "Enter") {
                e.preventDefault();
                add("ticker");
              }
            }}
          />
          <button type="button" className="news-field-add" onClick={() => add("ticker")} disabled={!ticker.trim()}>
            add
          </button>
        </fieldset>
        <fieldset className="news-field" disabled={disabled} data-news-field="custom">
          <legend className="sr-only">Anything else?</legend>
          {value.custom.map((t) => (
            <button
              key={t}
              type="button"
              className="news-field-token"
              aria-label={`Remove ${t}`}
              title={`Remove ${t}`}
              onClick={() => onChange(removeCustom(value, t))}
            >
              {t}
            </button>
          ))}
          <input
            type="text"
            className="news-field-input"
            aria-label="Add your own topic"
            placeholder={'Anything else? Type a topic, like "Austin real estate" or "Tauri releases"'}
            value={custom}
            maxLength={80}
            onChange={(e) => setCustom(e.target.value)}
            onKeyDown={(e) => {
              if (e.key === "Enter") {
                e.preventDefault();
                add("custom");
              }
            }}
          />
        </fieldset>
        {error && (
          <p className="news-error" role="alert">
            {error}
          </p>
        )}
      </div>
    );
  }

  return (
    <div className="news-interests" data-news-interests>
      <fieldset className="news-group" disabled={disabled}>
        <legend>Topics</legend>
        <div className="news-chips" role="group" aria-label="Topics">
          {TOPIC_CHIPS.map((c) => {
            const on = value.topics.includes(c.id);
            return (
              <button
                key={c.id}
                type="button"
                className={on ? "chip selected" : "chip"}
                aria-pressed={on}
                data-news-chip={c.id}
                onClick={() => onChange(toggleTopic(value, c.id))}
              >
                {c.label}
              </button>
            );
          })}
        </div>
      </fieldset>

      <fieldset className="news-group" disabled={disabled}>
        <legend>Tickers you follow</legend>
        {value.tickers.length > 0 && (
          <ul className="news-tokens" aria-label="Tickers you follow">
            {value.tickers.map((t) => (
              <li key={t} className="news-token">
                <span>{t}</span>
                <button
                  type="button"
                  className="ghost news-token-remove"
                  aria-label={`Remove ${t}`}
                  onClick={() => onChange(removeTicker(value, t))}
                >
                  ×
                </button>
              </li>
            ))}
          </ul>
        )}
        <div className="news-add">
          <input
            type="text"
            aria-label="Add a ticker"
            placeholder="NVDA"
            value={ticker}
            maxLength={12}
            autoCapitalize="characters"
            spellCheck={false}
            onChange={(e) => setTicker(e.target.value)}
            onKeyDown={(e) => {
              if (e.key === "Enter") {
                e.preventDefault();
                add("ticker");
              }
            }}
          />
          <button type="button" className="ghost" onClick={() => add("ticker")} disabled={!ticker.trim()}>
            Add
          </button>
        </div>
      </fieldset>

      <fieldset className="news-group" disabled={disabled}>
        <legend>Anything else?</legend>
        {value.custom.length > 0 && (
          <ul className="news-tokens" aria-label="Your own topics">
            {value.custom.map((t) => (
              <li key={t} className="news-token">
                <span>{t}</span>
                <button
                  type="button"
                  className="ghost news-token-remove"
                  aria-label={`Remove ${t}`}
                  onClick={() => onChange(removeCustom(value, t))}
                >
                  ×
                </button>
              </li>
            ))}
          </ul>
        )}
        <div className="news-add">
          <input
            type="text"
            aria-label="Add your own topic"
            placeholder={'Type a topic, like "Austin real estate"'}
            value={custom}
            maxLength={80}
            onChange={(e) => setCustom(e.target.value)}
            onKeyDown={(e) => {
              if (e.key === "Enter") {
                e.preventDefault();
                add("custom");
              }
            }}
          />
          <button type="button" className="ghost" onClick={() => add("custom")} disabled={!custom.trim()}>
            Add
          </button>
        </div>
      </fieldset>

      {error && (
        <p className="news-error" role="alert">
          {error}
        </p>
      )}
    </div>
  );
}

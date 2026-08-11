import { useId, useState } from "react";
import YappySprite from "./YappySprite";
import {
  activationError,
  daysLeft,
  FOUNDING_CODE,
  FOUNDING_PRICE_LABEL,
  KEEP_FOREVER_LINE,
  PRICE_LABEL,
  statusCopy,
  storedKeyProblem,
  TRIAL_WARN_DAYS,
  trialCountdown,
  type LicenseStatus,
} from "./status";

/**
 * YP3 — Settings → License.
 *
 * One screen that answers the three questions a person actually has: what am I
 * on right now, how do I pay, and where do I put the key I already bought. It
 * is deliberately presentational — every effect (invoke, toast, revocation
 * refresh) belongs to `App.tsx`, which is what lets the dev harness in
 * `desktop/dev/license-preview.tsx` render all three states without a backend.
 */
export default function LicensePanel({
  status,
  onBuy,
  onActivate,
  onDeactivate,
}: {
  status: LicenseStatus | null;
  /** Opens Stripe's hosted checkout (Rust `open_purchase_page`). */
  onBuy: () => Promise<void> | void;
  /** Rejects with the backend's `{code, message}` when the key is refused. */
  onActivate: (key: string) => Promise<void>;
  onDeactivate: () => Promise<void>;
}) {
  const [draft, setDraft] = useState("");
  const [busy, setBusy] = useState(false);
  const [problem, setProblem] = useState<string | null>(null);
  const [ok, setOk] = useState(false);
  const keyId = useId();

  if (!status) {
    return (
      <section className="settings-panel">
        <h2 className="settings-section">
          License
          <span className="sub">Reading this Mac’s license…</span>
        </h2>
      </section>
    );
  }

  const copy = statusCopy(status);
  const days = daysLeft(status);
  const storedProblem = storedKeyProblem(status);

  async function activate() {
    const key = draft.trim();
    if (!key) {
      // The one message the backend cannot write, because nothing reached it.
      setProblem("Paste your license key first — it is the long line in your purchase email.");
      setOk(false);
      return;
    }
    setBusy(true);
    setProblem(null);
    try {
      await onActivate(key);
      setDraft("");
      setOk(true);
    } catch (e) {
      setProblem(activationError(e));
      setOk(false);
    } finally {
      setBusy(false);
    }
  }

  async function buy() {
    try {
      await onBuy();
    } catch (e) {
      setProblem(activationError(e));
    }
  }

  return (
    <section className="settings-panel">
      <h2 className="settings-section">
        License
        <span className="sub">
          What Yap costs, what your trial has left, and where your key goes.
        </span>
      </h2>

      {/* ── The state of this Mac, in one card ── */}
      <div className={`panel license-card license-${status.state}`}>
        <div className="license-card-body">
          <span className="license-eyebrow">{copy.eyebrow}</span>
          <h3>{copy.headline}</h3>
          <p>{copy.body}</p>
        </div>
        {status.state === "trial" && (
          <div
            className={
              days <= TRIAL_WARN_DAYS ? "license-countdown urgent" : "license-countdown"
            }
            aria-label={trialCountdown(days)}
          >
            <strong>{days}</strong>
            <span>{days === 1 ? "day" : "days"}</span>
          </div>
        )}
        {status.state === "licensed" && <YappySprite size={64} className="license-mascot" />}
      </div>

      {/* ── A stored key that granted nothing. Only ever shown when there is
             something the person can do about it. ── */}
      {storedProblem && (
        <div className="banner warn" role="status">
          <span>{storedProblem}</span>
        </div>
      )}

      {/* ── Buy ── */}
      {status.state !== "licensed" && (
        <div className="panel license-buy">
          <div>
            <h3>Buy Yap</h3>
            <p>
              <strong className="license-price">{PRICE_LABEL}</strong> once. Not a
              subscription, not an account — one key, yours for good, and it activates
              up to three Macs. Checkout opens at Stripe in your browser; the key comes
              straight back by email.
            </p>
            <p className="license-promo">
              Founding price <strong className="license-price">{FOUNDING_PRICE_LABEL}</strong>{" "}
              — enter <code>{FOUNDING_CODE}</code> in the promo box at checkout.
            </p>
          </div>
          <button type="button" className="primary license-buy-btn" onClick={buy}>
            Buy Yap — {PRICE_LABEL} once
          </button>
        </div>
      )}

      {/* ── Activate ── */}
      <div className="panel">
        <h3>{status.state === "licensed" ? "Replace your key" : "Already bought Yap?"}</h3>
        <p>
          Paste the key from your purchase email. It is verified on this Mac — Yap does
          not phone home to check it, and it keeps working offline forever.
        </p>
        <div className="field license-key-field">
          <label htmlFor={keyId}>License key</label>
          <textarea
            id={keyId}
            rows={2}
            spellCheck={false}
            autoCapitalize="off"
            autoCorrect="off"
            className="license-key"
            placeholder="eyJ2Ijox…"
            value={draft}
            onChange={(e) => {
              setDraft(e.target.value);
              setProblem(null);
              setOk(false);
            }}
            onKeyDown={(e) => {
              // A key is one long line, so Enter means "activate", not newline.
              if (e.key === "Enter" && !e.shiftKey) {
                e.preventDefault();
                if (!busy) activate();
              }
            }}
          />
        </div>
        <div className="license-actions">
          <button type="button" className="primary" disabled={busy} onClick={activate}>
            {busy ? "Checking…" : "Activate"}
          </button>
          {status.state === "licensed" && (
            <button
              type="button"
              className="ghost danger"
              onClick={() => {
                setProblem(null);
                setOk(false);
                onDeactivate();
              }}
            >
              Remove from this Mac
            </button>
          )}
        </div>
        {problem && (
          <p className="license-problem" role="alert">
            {problem}
          </p>
        )}
        {ok && (
          <p className="license-ok" role="status">
            Activated. Yap is yours — thank you.
          </p>
        )}
        <p className="license-lost">
          Lost the email? Reply to your Stripe receipt and we will send the key again.
        </p>
      </div>

      {/* ── The promise, and the small print that proves it ── */}
      <div className="panel license-promise">
        <p>{KEEP_FOREVER_LINE}</p>
        <p className="license-fine">
          {status.state === "licensed" ? (
            <>
              Plan <span className="mono-num">{status.plan}</span> · seats{" "}
              <span className="mono-num">{status.seats}</span> · key{" "}
              <span className="mono-num">{status.kid}</span>
            </>
          ) : (
            <>
              Trial length <span className="mono-num">14</span> days · price{" "}
              <span className="mono-num">{PRICE_LABEL}</span> one time · seats{" "}
              <span className="mono-num">3</span>
            </>
          )}
        </p>
      </div>
    </section>
  );
}

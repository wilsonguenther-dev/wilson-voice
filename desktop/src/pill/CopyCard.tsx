// yap-p0-insertion-bridge-fallback DO 2 — the Copy card on the pill. Mounted
// by float-main.tsx (float.html), beside whichever capsule style is active.
import { useCallback, useEffect, useRef, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { holdPillHitbox } from "./drag";
import {
  COPY_CARD_CLEAR_EVENT,
  COPY_CARD_TTL_MS,
  INSERT_COPY_CARD_EVENT,
  blockedLine,
  canRepair,
  cardHint,
  cardPreview,
  isCopyCard,
  settingsPaneFor,
  type CopyCardPayload,
} from "./copyCardModel";

export default function CopyCard() {
  const [card, setCard] = useState<CopyCardPayload | null>(null);
  const [copied, setCopied] = useState(false);
  const ref = useRef<HTMLDivElement | null>(null);

  const show = useCallback((c: unknown) => {
    if (!isCopyCard(c)) return;
    setCard(c);
    setCopied(c.copied);
  }, []);

  useEffect(() => {
    let dead = false;
    const unsubs: Array<() => void> = [];
    listen<CopyCardPayload>(INSERT_COPY_CARD_EVENT, (e) => show(e.payload)).then((u) =>
      dead ? u() : unsubs.push(u),
    );
    // A new take started: drop the card WITHOUT dismiss_copy_card (no hide),
    // which also hands the hitbox back to the capsule.
    listen(COPY_CARD_CLEAR_EVENT, () => setCard(null)).then((u) =>
      dead ? u() : unsubs.push(u),
    );
    // A pill webview that mounted after the emit still gets the card.
    invoke<CopyCardPayload | null>("pending_copy_card").then((c) => {
      if (!dead) show(c);
    }).catch(() => {});
    return () => {
      dead = true;
      unsubs.forEach((u) => u());
    };
  }, [show]);

  const dismiss = useCallback(() => {
    setCard(null);
    invoke("dismiss_copy_card").catch(() => {});
  }, []);

  useEffect(() => {
    if (!card) return;
    // The float window is click-through outside the reported hitbox; while the
    // card is up the card IS the hitbox, so Copy and Repair can be clicked.
    const el = ref.current;
    if (el) {
      const r = el.getBoundingClientRect();
      holdPillHitbox({ x: r.left, y: r.top, w: r.width, h: r.height });
    }
    const t = setTimeout(dismiss, COPY_CARD_TTL_MS);
    return () => {
      clearTimeout(t);
      holdPillHitbox(null);
    };
  }, [card, dismiss]);

  if (!card) return null;

  const copy = async () => {
    try {
      await invoke("copy_entry", { text: card.text });
      setCopied(true);
    } catch {
      setCopied(false);
    }
  };
  const repairable = canRepair(card);
  const repair = () => {
    // Never reachable without a service: Rust only names one when neither
    // Accessibility nor PostEvent is working (paste.rs repair_service_for).
    if (!repairable) return;
    invoke("repair_permission", { service: card.repairService }).catch(() => {});
    dismiss();
  };

  const blocked = blockedLine(card);
  const pane = settingsPaneFor(card);
  const openSettings = () => {
    if (!pane) return;
    invoke("open_privacy_settings", { pane }).catch(() => {});
  };

  return (
    <div
      ref={ref}
      className="copy-card"
      role="dialog"
      aria-label="Transcript ready to paste"
      data-copy-card={card.reason}
      title={card.detail}
    >
      {blocked && (
        <p className="copy-card-blocked" role="alert" data-blocked={card.blockedPermissions?.join(",")}>
          {blocked}
        </p>
      )}
      <p className="copy-card-text">{cardPreview(card.text)}</p>
      <p className="copy-card-hint" role="status">{cardHint(copied, repairable, card.reason)}</p>
      <div className="copy-card-actions">
        <button type="button" className="copy-card-copy" onClick={() => void copy()}>
          {copied ? "Copied" : "Copy"}
        </button>
        {pane && (
          <button type="button" className="copy-card-settings" onClick={openSettings}>
            Open Settings
          </button>
        )}
        {repairable && (
          <button type="button" className="copy-card-repair" onClick={repair}>
            Repair
          </button>
        )}
        <button type="button" className="copy-card-close" aria-label="Dismiss" onClick={dismiss}>
          ×
        </button>
      </div>
    </div>
  );
}

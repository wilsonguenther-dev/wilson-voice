/**
 * yap-call-1 — the call offer window. Renders the SHIPPING surface with
 * `renderToStaticMarkup` (no DOM, no window) and presses its buttons by
 * walking the element tree; the answers go through an injected `call`, so the
 * exact Tauri command + arguments are pinned.
 */
import { describe, expect, it, vi } from "vitest";
import { createElement, type ReactElement, type ReactNode } from "react";
import { renderToStaticMarkup } from "react-dom/server";
import { CONSENT_PILL_LINE } from "../meetings/consent";
import { CALL_ENDED_HEADLINE, CallOfferSurface, acceptOffer, chooseOffer } from "./CallOffer";
import {
  ACCEPT_CALL_OFFER,
  CALL_OFFER_CHOOSE,
  COUNTDOWN_MS,
  type CallOfferView,
} from "./callOfferModel";

const ZOOM: CallOfferView = {
  phase: "offering",
  offerId: "app:us.zoom.xos:4242#1",
  appName: "Zoom",
  bundleId: "us.zoom.xos",
  copy: "Zoom call — record it?",
  siteLine: null,
  yourSideOnly: false,
  countdownMsLeft: COUNTDOWN_MS,
};

const FACETIME: CallOfferView = {
  ...ZOOM,
  offerId: "app:com.apple.FaceTime:77#1",
  appName: "FaceTime",
  bundleId: "com.apple.FaceTime",
  copy: "FaceTime call — record your side?",
  yourSideOnly: true,
};

/** Expand function components so their host elements can be walked. */
function walk(node: ReactNode, out: ReactElement[] = []): ReactElement[] {
  if (Array.isArray(node)) node.forEach((n) => walk(n, out));
  else if (node && typeof node === "object" && "props" in node) {
    const el = node as ReactElement<{ children?: ReactNode }>;
    if (typeof el.type === "function") {
      walk((el.type as (p: unknown) => ReactNode)(el.props), out);
    } else {
      out.push(el);
      walk(el.props.children, out);
    }
  }
  return out;
}

function surface(view: CallOfferView, elapsedMs: number, onRecord = vi.fn(), onChoose = vi.fn()) {
  return createElement(CallOfferSurface, { view, elapsedMs, onRecord, onChoose });
}

function button(tree: ReactNode, className: string) {
  const b = walk(tree).find(
    (el) => el.type === "button" && (el.props as { className?: string }).className === className,
  );
  if (!b) throw new Error(`no button .${className}`);
  return b as ReactElement<{ onClick: () => void }>;
}

describe("CallOffer — yap-call-1", () => {
  it("renders the offer copy and CONSENT_PILL_LINE, with Record / Not now / Never", () => {
    const html = renderToStaticMarkup(surface(ZOOM, 0));
    expect(html).toContain("Zoom call — record it?");
    expect(html).toContain(CONSENT_PILL_LINE.replace("'", "&#x27;"));
    expect(html).toContain("Record");
    expect(html).toContain("Not now");
    expect(html).toContain("Never for Zoom");
  });

  it("counts down 10 s", () => {
    expect(COUNTDOWN_MS).toBe(10_000);
    const html = renderToStaticMarkup(surface(ZOOM, 0));
    expect(html).toContain('data-seconds="10"');
    expect(html).toContain('aria-label="10 seconds"');
    // Still the offer just before the ring runs out.
    expect(renderToStaticMarkup(surface(ZOOM, 9_999))).toContain('data-call-offer="offer"');
  });

  it("the chip is present after 10 s, and the offer is gone (no second banner)", () => {
    const html = renderToStaticMarkup(surface(ZOOM, 10_000));
    expect(html).toContain('data-call-offer="chip"');
    expect(html).toContain("Record Zoom call");
    expect(html).not.toContain(CONSENT_PILL_LINE.replace("'", "&#x27;"));
    expect(html).not.toContain("Not now");
    // The backend's own Chip phase draws the same chip.
    expect(renderToStaticMarkup(surface({ ...ZOOM, phase: "chip" }, 0))).toContain("Record Zoom call");
  });

  it("the chip records with the SAME live offer id", async () => {
    const onRecord = vi.fn();
    button(surface({ ...ZOOM, phase: "chip" }, 0, onRecord), "call-offer-chip").props.onClick();
    expect(onRecord).toHaveBeenCalledTimes(1);
    const call = vi.fn().mockResolvedValue(null);
    await acceptOffer({ ...ZOOM, phase: "chip" }, call);
    expect(call).toHaveBeenCalledWith(ACCEPT_CALL_OFFER, { offerId: "app:us.zoom.xos:4242#1" });
  });

  it("Never persists: it goes to call_offer_choose with choice never for this offer", async () => {
    const onChoose = vi.fn();
    button(surface(ZOOM, 0, vi.fn(), onChoose), "call-offer-never").props.onClick();
    expect(onChoose).toHaveBeenCalledWith("never");
    const call = vi.fn().mockResolvedValue(null);
    await chooseOffer(ZOOM, "never", call);
    expect(call).toHaveBeenCalledWith(CALL_OFFER_CHOOSE, {
      offerId: "app:us.zoom.xos:4242#1",
      choice: "never",
    });
  });

  it("Not now is the only answer the countdown gives (never Record)", () => {
    const onRecord = vi.fn();
    const onChoose = vi.fn();
    button(surface(ZOOM, 0, onRecord, onChoose), "call-offer-notnow").props.onClick();
    expect(onChoose).toHaveBeenCalledWith("not_now");
    expect(onRecord).not.toHaveBeenCalled();
  });

  it("FaceTime copy says 'record your side?'", () => {
    const html = renderToStaticMarkup(surface(FACETIME, 0));
    expect(html).toContain("FaceTime call — record your side?");
    expect(html).toContain("Never for FaceTime");
  });

  it("a browser call with no title match shows 'site unknown'", () => {
    const html = renderToStaticMarkup(
      surface({ ...ZOOM, appName: "Browser", copy: "Browser call — record it?", siteLine: "site unknown" }, 0),
    );
    expect(html).toContain("Browser call — record it?");
    expect(html).toContain("site unknown");
  });

  it("asks 'Call ended — stop recording?' with Stop / Keep going, and stops only on Stop", () => {
    const ended = { ...ZOOM, phase: "end_prompt" as const };
    const html = renderToStaticMarkup(surface(ended, 0));
    expect(html).toContain(CALL_ENDED_HEADLINE);
    expect(CALL_ENDED_HEADLINE).toBe("Call ended — stop recording?");
    const onChoose = vi.fn();
    button(surface(ended, 0, vi.fn(), onChoose), "call-offer-keep").props.onClick();
    expect(onChoose).toHaveBeenLastCalledWith("keep_going");
    button(surface(ended, 0, vi.fn(), onChoose), "call-offer-stop").props.onClick();
    expect(onChoose).toHaveBeenLastCalledWith("stop");
  });

  it("draws nothing while idle or recording, or without a live id", () => {
    expect(renderToStaticMarkup(surface({ ...ZOOM, phase: "idle" }, 0))).toBe("");
    expect(renderToStaticMarkup(surface({ ...ZOOM, phase: "recording" }, 0))).toBe("");
    expect(renderToStaticMarkup(surface({ ...ZOOM, offerId: null }, 0))).toBe("");
  });
});

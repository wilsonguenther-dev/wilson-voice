/**
 * yap-notetaker-detect — the pill's offer. Renders the SHIPPING view (no DOM:
 * `renderToStaticMarkup`, like TranscriptList.test.tsx) and presses its
 * buttons by walking the element tree, since the view has no hooks.
 */
import { describe, expect, it, vi } from "vitest";
import { renderToStaticMarkup } from "react-dom/server";
import type { ReactElement, ReactNode } from "react";
import { CONSENT_PILL_LINE } from "../meetings/consent";
import { MeetingOfferView, respondToOffer } from "./MeetingOfferCard";
import {
  MEETING_OFFER_RESPOND,
  OFFER_HEADLINE,
  isMeetingOffer,
  offerLines,
  type MeetingOffer,
  type OfferChoice,
} from "./meetingOffer";

const OFFER: MeetingOffer = { key: "cal:EV1:1800000000", title: "Design review", source: "calendar" };

/** Every element in a rendered tree (host elements only; the view has no children components). */
function walk(node: ReactNode, out: ReactElement[] = []): ReactElement[] {
  if (Array.isArray(node)) node.forEach((n) => walk(n, out));
  else if (node && typeof node === "object" && "props" in node) {
    const el = node as ReactElement<{ children?: ReactNode }>;
    out.push(el);
    walk(el.props.children, out);
  }
  return out;
}

function button(tree: ReactNode, className: string) {
  const b = walk(tree).find(
    (el) => el.type === "button" && (el.props as { className?: string }).className === className,
  );
  if (!b) throw new Error(`no button .${className}`);
  return b as ReactElement<{ onClick: () => void; children: ReactNode }>;
}

describe("meeting offer — copy", () => {
  it("renders the headline, the meeting title and CONSENT_PILL_LINE", () => {
    const html = renderToStaticMarkup(<MeetingOfferView offer={OFFER} onChoose={() => {}} />);
    expect(html).toContain(OFFER_HEADLINE);
    expect(OFFER_HEADLINE).toBe("Record this meeting?");
    expect(html).toContain("Design review");
    expect(CONSENT_PILL_LINE).toBe("Let them know you're recording.");
    // React escapes the apostrophe; compare against the escaped form.
    expect(html).toContain(CONSENT_PILL_LINE.replace("'", "&#x27;"));
    expect(html).toContain(">Record<");
    expect(html).toContain(">Not now<");
  });

  it("omits the title row when the title is unknown or blank", () => {
    for (const title of [null, "   "]) {
      const html = renderToStaticMarkup(
        <MeetingOfferView offer={{ ...OFFER, title }} onChoose={() => {}} />,
      );
      expect(html).not.toContain("meeting-offer-title");
      expect(html).toContain(CONSENT_PILL_LINE.replace("'", "&#x27;"));
    }
    expect(offerLines({ ...OFFER, title: "  Standup " }).title).toBe("Standup");
  });

  it("does not use the long CONSENT_NOTICE's 'Nothing is uploaded' claim", () => {
    const html = renderToStaticMarkup(<MeetingOfferView offer={OFFER} onChoose={() => {}} />);
    expect(html).not.toContain("Nothing is uploaded");
  });
});

describe("meeting offer — answers", () => {
  it("Not now emits snooze; Record emits record", () => {
    const chosen: OfferChoice[] = [];
    const tree = MeetingOfferView({ offer: OFFER, onChoose: (c) => chosen.push(c) });
    button(tree, "meeting-offer-snooze").props.onClick();
    expect(chosen).toEqual(["snooze"]);
    button(tree, "meeting-offer-record").props.onClick();
    expect(chosen).toEqual(["snooze", "record"]);
  });

  it("an answer reaches Rust as meeting_offer_respond {key, choice}", async () => {
    const call = vi.fn().mockResolvedValue(null);
    await respondToOffer(OFFER, "snooze", call);
    expect(call).toHaveBeenCalledWith(MEETING_OFFER_RESPOND, { key: OFFER.key, choice: "snooze" });
    expect(MEETING_OFFER_RESPOND).toBe("meeting_offer_respond");
  });

  it("only accepts a well-formed offer payload", () => {
    expect(isMeetingOffer(OFFER)).toBe(true);
    expect(isMeetingOffer({ ...OFFER, title: null, source: "app" })).toBe(true);
    expect(isMeetingOffer({ ...OFFER, key: "" })).toBe(false);
    expect(isMeetingOffer({ ...OFFER, source: "auto" })).toBe(false);
    expect(isMeetingOffer(null)).toBe(false);
  });
});

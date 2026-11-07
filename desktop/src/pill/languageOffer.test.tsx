/**
 * WF-12 — the pill's "Sounded like Spanish — Add Spanish?" card. Renders the
 * SHIPPING view (no DOM: `renderToStaticMarkup`, like meetingOffer.test.tsx)
 * and presses its buttons by walking the element tree, since the view has no
 * hooks. The 7-day rule itself is Rust's (`languages::wf12_offer_tests`).
 */
import { describe, expect, it } from "vitest";
import { renderToStaticMarkup } from "react-dom/server";
import type { ReactElement, ReactNode } from "react";
import LanguageOfferCard, { LanguageOfferView, respondToLanguageOffer } from "./LanguageOfferCard";
import { CARD_CLASSES, CARD_SELECTOR } from "./state";
import {
  LANGUAGE_OFFER_EVENT,
  LANGUAGE_OFFER_QUIET_DAYS,
  LANGUAGE_OFFER_RESPOND,
  SAMPLE_LANGUAGE_OFFER,
  forcedLanguageOffer,
  isLanguageOffer,
  languageOfferArgs,
  languageOfferLines,
  type LanguageOffer,
  type LanguageOfferChoice,
} from "./languageOffer";
import { matrixLanguageOfferScene } from "./forced";

const OFFER: LanguageOffer = {
  code: "es",
  name: "Spanish",
  headline: "Sounded like Spanish — Add Spanish?",
};

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
  return b as ReactElement<{ onClick: () => void; disabled?: boolean; children: ReactNode }>;
}

describe("language offer — wire", () => {
  it("names match the Rust constants", () => {
    expect(LANGUAGE_OFFER_EVENT).toBe("language_offer");
    expect(LANGUAGE_OFFER_RESPOND).toBe("language_offer_respond");
    expect(LANGUAGE_OFFER_QUIET_DAYS).toBe(7);
  });

  it("accepts a well-formed offer and rejects anything else", () => {
    expect(isLanguageOffer(OFFER)).toBe(true);
    expect(isLanguageOffer(null)).toBe(false);
    expect(isLanguageOffer({ ...OFFER, code: "spanish" })).toBe(false);
    expect(isLanguageOffer({ ...OFFER, name: "" })).toBe(false);
    expect(isLanguageOffer({ code: "es", name: "Spanish" })).toBe(false);
  });
});

describe("language offer — copy", () => {
  it("renders the headline with Add and Not now", () => {
    const html = renderToStaticMarkup(<LanguageOfferView offer={OFFER} onChoose={() => {}} />);
    expect(html).toContain("Sounded like Spanish — Add Spanish?");
    expect(html).toContain(">Add<");
    expect(html).toContain(">Not now<");
    expect(html).toContain('role="dialog"');
    expect(languageOfferLines(OFFER).headline).toBe(OFFER.headline);
  });

  it("shows an error as an alert and keeps both answers", () => {
    const html = renderToStaticMarkup(
      <LanguageOfferView offer={OFFER} error="Yap couldn't add Spanish." onChoose={() => {}} />,
    );
    expect(html).toContain('role="alert"');
    expect(html).toContain("Yap couldn&#x27;t add Spanish.");
    expect(html).toContain(">Add<");
  });

  it("disables both buttons while an answer is in flight", () => {
    const tree = LanguageOfferView({ offer: OFFER, busy: true, onChoose: () => {} });
    expect(button(tree, "language-offer-add").props.disabled).toBe(true);
    expect(button(tree, "language-offer-later").props.disabled).toBe(true);
  });
});

describe("language offer — answers", () => {
  it("Add emits add; Not now emits notNow", () => {
    const chosen: LanguageOfferChoice[] = [];
    const tree = LanguageOfferView({ offer: OFFER, onChoose: (c) => chosen.push(c) });
    button(tree, "language-offer-later").props.onClick();
    expect(chosen).toEqual(["notNow"]);
    button(tree, "language-offer-add").props.onClick();
    expect(chosen).toEqual(["notNow", "add"]);
  });

  it("each answer is one language_offer_respond call with the code", async () => {
    const calls: Array<[string, Record<string, unknown>]> = [];
    const call = async (cmd: string, args: Record<string, unknown>) => {
      calls.push([cmd, args]);
      return null;
    };
    await respondToLanguageOffer(OFFER, "add", call);
    await respondToLanguageOffer(OFFER, "notNow", call);
    expect(calls).toEqual([
      ["language_offer_respond", { code: "es", add: true }],
      ["language_offer_respond", { code: "es", add: false }],
    ]);
    expect(languageOfferArgs(OFFER, "add")).toEqual({ code: "es", add: true });
  });
});

describe("language offer — matrix scene", () => {
  it("draws the sample only for langOffer=1", () => {
    expect(forcedLanguageOffer("?phase=idle&langOffer=1")).toEqual(SAMPLE_LANGUAGE_OFFER);
    expect(forcedLanguageOffer("?phase=idle")).toBeNull();
    expect(isLanguageOffer(SAMPLE_LANGUAGE_OFFER)).toBe(true);
  });

  it("the matrix row's query raises the same card the row expects", () => {
    const row = matrixLanguageOfferScene();
    expect(row.phase).toBe("idle");
    expect(row.phaseId).toBe("idle");
    expect(row.id).toBe("lang-offer");
    const offer = forcedLanguageOffer("?phase=idle&" + row.query);
    expect(offer?.code).toBe(row.offer);
  });
});

describe("language offer — one-pill card row (fix-b1-2)", () => {
  it("card_selector_matches_a_rendered_language_offer_card", () => {
    // The SHIPPING card (not the view): the element float-main's
    // useCardOnScreen measures and float.css places is its outermost box.
    const html = renderToStaticMarkup(<LanguageOfferCard forced={OFFER} />);
    const outer = /^<div class="([^"]+)"/.exec(html);
    expect(outer, html).not.toBeNull();
    const classes = outer![1].split(/\s+/);
    expect(classes).toContain("language-offer-wrap");
    // CARD_SELECTOR is a plain class list: the card matches when one of its
    // outer classes is on it.
    const selectorClasses = CARD_SELECTOR.split(",").map((part) => part.trim().replace(/^\./, ""));
    expect(selectorClasses).toEqual([...CARD_CLASSES]);
    expect(selectorClasses.some((c) => classes.includes(c))).toBe(true);
    // ...and the card inside it is the dialog, so the measured box holds all of it.
    expect(html).toContain('data-language-offer="es"');
  });

  it("no_offer_renders_no_card_so_the_row_is_not_reserved", () => {
    expect(renderToStaticMarkup(<LanguageOfferCard forced={null} />)).toBe("");
  });
});

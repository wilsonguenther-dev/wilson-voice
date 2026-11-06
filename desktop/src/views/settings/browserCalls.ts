/**
 * yap-call-2 — Settings > Meetings copy for calls in a browser. Mirrors
 * `call_titles.rs` (`FRONT_TAB_LINE`, `AX_OFF_LINE`, `SITE_UNKNOWN_NAME`);
 * `browserCalls.test.ts` keeps the two in step.
 */

/** The honest limit: a Chromium window's title is its FRONT tab. */
export const FRONT_TAB_LINE =
  "Yap reads the front tab's title; a call in a background tab is not offered";

/** Shown only while Accessibility is off; links the Permissions view. */
export const AX_OFF_LINE = "Turn on Accessibility so Yap can tell which site the call is on";

/** What an offer calls a browser call whose site Yap cannot read. */
export const SITE_UNKNOWN_NAME = "Browser call (site unknown)";

export const BROWSER_CALLS_LINE =
  "In Chrome, Arc, Edge and Brave, Yap tells a Meet, Zoom, Teams, Slack, Discord, Whereby, Webex or Around call apart by the window's title, and only while the browser is using the microphone. The title is checked on this Mac and never saved.";

export const SAFARI_LINE = "Calls in Safari and Firefox are not offered yet.";

/**
 * Which lines the panel shows. `accessibility` is `null` while the permission
 * report is loading: the Accessibility line waits for a real `false` so it
 * never flashes for someone who already granted it.
 */
export function browserCallsLines(accessibility: boolean | null): {
  intro: string;
  frontTab: string;
  axOff: string | null;
  safari: string;
} {
  return {
    intro: BROWSER_CALLS_LINE,
    frontTab: FRONT_TAB_LINE,
    axOff: accessibility === false ? AX_OFF_LINE : null,
    safari: SAFARI_LINE,
  };
}

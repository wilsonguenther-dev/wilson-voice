/**
 * yap-1106-meeting-export-and-leftovers — the export actions are present in
 * BOTH places (the list row and the detail header), each names the same three
 * actions, every command they call exists in lib.rs, and Settings › Meetings
 * has "Open meetings folder". No DOM: `renderToStaticMarkup` + source text,
 * the same way `NotetakerSetupCard.test.tsx` pins its wire.
 */
import { describe, expect, it } from "vitest";
import { renderToStaticMarkup } from "react-dom/server";
import libRs from "../../src-tauri/src/lib.rs?raw";
import meetingsView from "../views/Meetings.tsx?raw";
import settingsMeetings from "../views/settings/Meetings.tsx?raw";
import ExportMenu, { EXPORT_ACTIONS } from "./ExportMenu";

describe("ExportMenu", () => {
  it("offers Export Markdown, Copy as Markdown and Show in Finder, in that order", () => {
    const html = renderToStaticMarkup(
      <ExportMenu meetingId="m1" onAction={() => {}} defaultOpen />,
    );
    expect(html).toContain('role="menu"');
    const labels = EXPORT_ACTIONS.map((a) => a.label);
    expect(labels).toEqual(["Export Markdown…", "Copy as Markdown", "Show in Finder"]);
    let last = -1;
    for (const label of labels) {
      const at = html.indexOf(label);
      expect(at, label).toBeGreaterThan(last);
      last = at;
    }
    expect(html).toContain('data-export-action="export"');
    expect(html).toContain('data-export-action="copy"');
    expect(html).toContain('data-export-action="reveal"');
  });

  it("stays closed until the trigger is pressed", () => {
    const html = renderToStaticMarkup(<ExportMenu meetingId="m1" onAction={() => {}} />);
    expect(html).toContain('aria-haspopup="menu"');
    expect(html).toContain('aria-expanded="false"');
    expect(html).not.toContain('role="menu"');
  });

  it("every command the menu names is a Tauri command in lib.rs", () => {
    for (const a of EXPORT_ACTIONS) {
      expect(libRs, a.command).toMatch(new RegExp(`fn ${a.command}\\(`));
      expect(libRs, `${a.command} registered`).toContain(`            ${a.command},`);
    }
  });

  it("the Meetings view mounts the menu in the list row AND the detail header", () => {
    const mounts = meetingsView.match(/<ExportMenu\b/g) ?? [];
    expect(mounts.length).toBe(2);
    // The old single-action buttons are gone: one export surface, not two.
    expect(meetingsView).not.toMatch(/>\s*Export Markdown\s*</);
    // Each action reaches its handler.
    expect(meetingsView).toContain('case "copy"');
    expect(meetingsView).toContain('case "reveal"');
    expect(meetingsView).toContain('case "export"');
  });

  it("Settings › Meetings opens the meetings folder and lets the folder change", () => {
    expect(settingsMeetings).toContain("Open meetings folder");
    expect(settingsMeetings).toContain('"open_meeting_export_dir"');
    expect(settingsMeetings).toContain('"set_meeting_export_dir"');
    expect(libRs).toContain("fn open_meeting_export_dir(");
    expect(libRs).toContain("fn set_meeting_export_dir(");
    expect(libRs).toContain("fn meeting_export_location(");
  });
});

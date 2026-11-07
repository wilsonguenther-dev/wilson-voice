/**
 * yap-1106-meeting-export-and-leftovers — the export menu names the three
 * actions, every command they call exists in lib.rs, and Settings › Meetings
 * has "Open meetings folder". No DOM: `renderToStaticMarkup` + source text,
 * the same way `NotetakerSetupCard.test.tsx` pins its wire.
 *
 * yap-1107-meetings-row-actions-no-overlay — the menu lives in the detail
 * header (and the row's right-click menu), never in a hover bar on the row;
 * its list is placed by the pure `placeMenu` so all of it is on screen.
 */
import { describe, expect, it } from "vitest";
import { renderToStaticMarkup } from "react-dom/server";
import libRs from "../../src-tauri/src/lib.rs?raw";
import meetingsView from "../views/Meetings.tsx?raw";
import settingsMeetings from "../views/settings/Meetings.tsx?raw";
import ExportMenu, { EXPORT_ACTIONS, FloatingMenu, MENU_MARGIN, exportItems, placeMenu } from "./ExportMenu";

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

  it("the Meetings view mounts the trigger in the detail header only, and the row menu offers export + Delete", () => {
    const mounts = meetingsView.match(/<ExportMenu\b/g) ?? [];
    expect(mounts.length).toBe(1);
    const head = meetingsView.slice(meetingsView.indexOf('className="meeting-detail-head"'), meetingsView.indexOf("<DetailTabs"));
    expect(head).toContain("<ExportMenu");
    expect(head).toContain("Delete meeting");
    // The list row has no hover action bar: no Export trigger and no Delete button in it.
    const row = meetingsView.slice(meetingsView.indexOf('className={`meeting-row${'), meetingsView.indexOf("</li>"));
    expect(row).not.toContain("<ExportMenu");
    expect(row).not.toContain("requestDelete(");
    expect(row).not.toContain('className="actions"');
    // Right-click on the row opens the same three export choices plus Delete (through the Undo).
    expect(row).toContain("onContextMenu={(e) => openRowMenu(e, m.id, m.title)}");
    const menu = meetingsView.slice(meetingsView.indexOf("<FloatingMenu"));
    expect(menu).toContain("exportItems(rowMenu.id, onExportAction, meetingBusy)");
    expect(menu).toContain("requestDelete(rowMenu.id, rowMenu.title)");
    // The old single-action buttons are gone: one export surface, not two.
    expect(meetingsView).not.toMatch(/>\s*Export Markdown\s*</);
    // Each action reaches its handler.
    expect(meetingsView).toContain('case "copy"');
    expect(meetingsView).toContain('case "reveal"');
    expect(meetingsView).toContain('case "export"');
  });

  it("the row menu renders every export choice and a danger Delete", () => {
    const html = renderToStaticMarkup(
      <FloatingMenu
        anchor={() => ({ left: 10, right: 10, top: 10, bottom: 10 })}
        align="start"
        label="Actions for Retro"
        name="row"
        meetingId="m1"
        items={[...exportItems("m1", () => {}), { key: "delete", label: "Delete meeting", danger: true, onSelect: () => {} }]}
        onClose={() => {}}
      />,
    );
    expect(html).toContain('role="menu"');
    expect(html).toContain('aria-label="Actions for Retro"');
    for (const a of EXPORT_ACTIONS) expect(html).toContain(`data-export-action="${a.action}"`);
    expect(html).toMatch(/class="danger"[^>]*data-menu-item="delete"/);
  });

  it("exportItems maps each action to onAction with the meeting id", () => {
    const seen: string[] = [];
    const items = exportItems("m9", (action, id) => seen.push(`${action}:${id}`));
    expect(items.map((i) => i.label)).toEqual(EXPORT_ACTIONS.map((a) => a.label));
    for (const i of items) i.onSelect();
    expect(seen).toEqual(["export:m9", "copy:m9", "reveal:m9"]);
  });

  describe("placeMenu keeps the whole menu inside the viewport", () => {
    const vp = { width: 1190, height: 773 };
    const size = { width: 200, height: 120 };
    const inside = (r: { left: number; top: number }) => {
      expect(r.left).toBeGreaterThanOrEqual(MENU_MARGIN);
      expect(r.top).toBeGreaterThanOrEqual(MENU_MARGIN);
      expect(r.left + size.width).toBeLessThanOrEqual(vp.width - MENU_MARGIN);
      expect(r.top + size.height).toBeLessThanOrEqual(vp.height - MENU_MARGIN);
    };
    it("opens below a header trigger, right edges aligned", () => {
      const r = placeMenu({ left: 1000, top: 60, right: 1080, bottom: 88 }, size, vp, "end");
      expect(r).toEqual({ left: 880, top: 92, flipped: false });
      inside(r);
    });
    it("flips above a trigger near the bottom", () => {
      const r = placeMenu({ left: 500, top: 700, right: 580, bottom: 728 }, size, vp, "end");
      expect(r.flipped).toBe(true);
      expect(r.top + size.height).toBeLessThanOrEqual(700);
      inside(r);
    });
    it("clamps a right-click at the window's right edge", () => {
      const r = placeMenu({ left: 1185, top: 300, right: 1185, bottom: 300 }, size, vp, "start", 0);
      expect(r.left).toBe(vp.width - size.width - MENU_MARGIN);
      inside(r);
    });
    it("clamps a trigger hugging the left edge", () => {
      const r = placeMenu({ left: 2, top: 40, right: 60, bottom: 60 }, size, vp, "end");
      expect(r.left).toBe(MENU_MARGIN);
      inside(r);
    });
    it("pins to the margin when neither side has room", () => {
      const tall = { width: 200, height: 700 };
      const r = placeMenu({ left: 300, top: 380, right: 380, bottom: 400 }, tall, vp, "end");
      expect(r.top).toBe(vp.height - tall.height - MENU_MARGIN);
    });
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

// yap-1106-meeting-export-and-leftovers — the export menu a meeting carries:
// one trigger, three actions, each a Rust command. The action list is data so
// the vitest can pin the three, and that every command named here exists in
// lib.rs.
//
// yap-1107-meetings-row-actions-no-overlay — the list is a FloatingMenu: it is
// rendered into document.body (a portal) at position:fixed from the trigger's
// rect, so no ancestor (an overflow:hidden row, the scrolling detail pane) can
// clip it, and it stays open while the pointer travels from the trigger into
// it. It closes on an outside press, Escape, or a choice — never on
// mouse-leave. The same FloatingMenu is the list row's right-click menu.
import { createPortal } from "react-dom";
import { useCallback, useEffect, useLayoutEffect, useRef, useState, type RefObject } from "react";
import { meetingsLogLine, shortId } from "./meetingsLayout";

export type ExportAction = "export" | "copy" | "reveal";

/** Label + the Tauri command each action invokes. Order = menu order. */
export const EXPORT_ACTIONS: ReadonlyArray<{ action: ExportAction; label: string; command: string }> = [
  { action: "export", label: "Export Markdown…", command: "export_meeting_markdown" },
  { action: "copy", label: "Copy as Markdown", command: "copy_meeting_markdown" },
  { action: "reveal", label: "Show in Finder", command: "reveal_meeting_export" },
];

export const EXPORT_TRIGGER_LABEL = "Export";

/** The viewport gap a menu always keeps, and the trigger-to-menu gap (tokens: --space-2 / --space-1). */
export const MENU_MARGIN = 8;
export const MENU_GAP = 4;

export interface AnchorRect {
  left: number;
  top: number;
  right: number;
  bottom: number;
}

/**
 * Pure: where a `size` menu goes so that ALL of it is inside the viewport.
 * `end` aligns the menu's right edge to the anchor's (a trigger at the right of
 * a header); `start` its left edge (a right-click point). Below the anchor
 * first; above it when below would overflow; pinned to the margin when neither
 * side has room.
 */
export function placeMenu(
  anchor: AnchorRect,
  size: { width: number; height: number },
  viewport: { width: number; height: number },
  align: "start" | "end" = "end",
  gap: number = MENU_GAP,
  margin: number = MENU_MARGIN,
): { left: number; top: number; flipped: boolean } {
  const wanted = align === "end" ? anchor.right - size.width : anchor.left;
  const left = Math.min(Math.max(margin, wanted), Math.max(margin, viewport.width - size.width - margin));
  let top = anchor.bottom + gap;
  let flipped = false;
  if (top + size.height > viewport.height - margin) {
    const above = anchor.top - gap - size.height;
    if (above >= margin) {
      top = above;
      flipped = true;
    } else {
      top = Math.max(margin, viewport.height - size.height - margin);
    }
  }
  return { left, top, flipped };
}

export interface MenuItem {
  key: string;
  label: string;
  onSelect: () => void;
  danger?: boolean;
  disabled?: boolean;
  /** Export items carry their action so tests and the capture tool can find them. */
  exportAction?: ExportAction;
}

export type MenuCloseReason = "outside" | "escape" | "select";

interface FloatingMenuProps {
  /** Read live on open, scroll and resize: the trigger's rect or a click point. */
  anchor: () => AnchorRect | null;
  align?: "start" | "end";
  /** Accessible name of the menu, e.g. "Export this meeting". */
  label: string;
  /** Telemetry token: which menu this is (`export`, `row`). */
  name: string;
  /** The meeting it acts on (short id goes to the log line, never the title). */
  meetingId: string;
  items: ReadonlyArray<MenuItem>;
  onClose: (reason: MenuCloseReason) => void;
  /** Presses inside this element (the trigger) are not "outside". */
  ignore?: RefObject<HTMLElement | null>;
}

/** Keys the menu owns while open, so the Meetings view's list keys don't also fire. */
const MENU_KEYS = new Set(["Escape", "ArrowDown", "ArrowUp", "Home", "End", "Backspace", "Delete"]);

export function FloatingMenu({ anchor, align = "end", label, name, meetingId, items, onClose, ignore }: FloatingMenuProps) {
  const list = useRef<HTMLDivElement | null>(null);
  const anchorRef = useRef(anchor);
  anchorRef.current = anchor;
  const closeRef = useRef(onClose);
  closeRef.current = onClose;
  // Server render (vitest renderToStaticMarkup) has no document: the list is
  // rendered inline until the first client effect, then moves to the portal.
  const [mounted, setMounted] = useState(false);
  const [pos, setPos] = useState<{ left: number; top: number } | null>(null);
  useEffect(() => setMounted(true), []);

  const place = useCallback((): boolean | null => {
    const a = anchorRef.current();
    const el = list.current;
    if (!a || !el) return null;
    const r = placeMenu(
      a,
      { width: el.offsetWidth, height: el.offsetHeight },
      { width: window.innerWidth, height: window.innerHeight },
      align,
      align === "start" ? 0 : MENU_GAP,
    );
    setPos((p) => (p && p.left === r.left && p.top === r.top ? p : { left: r.left, top: r.top }));
    return r.flipped;
  }, [align]);

  // Placed before paint (the first frame is visibility:hidden, never at 0,0).
  useLayoutEffect(() => {
    if (!mounted) return;
    const flipped = place();
    console.info(meetingsLogLine("menu_open", { menu: name, id: shortId(meetingId), flipped: Boolean(flipped) }));
    list.current?.querySelector<HTMLButtonElement>('[role="menuitem"]:not(:disabled)')?.focus({ preventScroll: true });
    // Opened once per mount; the name/id of an open menu do not change.
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [mounted, place]);

  useEffect(() => {
    if (!mounted) return;
    const onDown = (e: MouseEvent) => {
      const t = e.target as Node;
      if (list.current?.contains(t) || ignore?.current?.contains(t)) return;
      closeRef.current("outside");
    };
    const onKey = (e: KeyboardEvent) => {
      if (!MENU_KEYS.has(e.key)) return;
      // Captured at the document, so the view's window listener never sees it.
      e.preventDefault();
      e.stopPropagation();
      if (e.key === "Escape") {
        closeRef.current("escape");
        return;
      }
      const enabled = [...(list.current?.querySelectorAll<HTMLButtonElement>('[role="menuitem"]:not(:disabled)') ?? [])];
      if (enabled.length === 0) return;
      const at = enabled.indexOf(document.activeElement as HTMLButtonElement);
      const next =
        e.key === "Home" ? 0
        : e.key === "End" ? enabled.length - 1
        : e.key === "ArrowDown" ? (at + 1) % enabled.length
        : e.key === "ArrowUp" ? (at <= 0 ? enabled.length - 1 : at - 1)
        : -1;
      if (next >= 0) enabled[next].focus();
    };
    const onMove = () => void place();
    document.addEventListener("mousedown", onDown, true);
    document.addEventListener("keydown", onKey, true);
    window.addEventListener("resize", onMove);
    window.addEventListener("scroll", onMove, true);
    return () => {
      document.removeEventListener("mousedown", onDown, true);
      document.removeEventListener("keydown", onKey, true);
      window.removeEventListener("resize", onMove);
      window.removeEventListener("scroll", onMove, true);
    };
  }, [mounted, place, ignore]);

  const body = (
    <div
      ref={list}
      className="floating-menu"
      role="menu"
      aria-label={label}
      data-floating-menu={name}
      style={pos ? { left: pos.left, top: pos.top } : { left: 0, top: 0, visibility: "hidden" }}
    >
      {items.map((item) => (
        <button
          key={item.key}
          type="button"
          role="menuitem"
          className={item.danger ? "danger" : undefined}
          disabled={item.disabled}
          data-menu-item={item.key}
          data-export-action={item.exportAction}
          onClick={() => {
            closeRef.current("select");
            item.onSelect();
          }}
        >
          {item.label}
        </button>
      ))}
    </div>
  );
  if (!mounted || typeof document === "undefined") return body;
  return createPortal(body, document.body);
}

/** The three export choices as menu items for `meetingId`. */
export function exportItems(
  meetingId: string,
  onAction: (action: ExportAction, meetingId: string) => void,
  disabled?: boolean,
): MenuItem[] {
  return EXPORT_ACTIONS.map((a) => ({
    key: a.action,
    label: a.label,
    exportAction: a.action,
    disabled,
    onSelect: () => onAction(a.action, meetingId),
  }));
}

interface Props {
  /** Which meeting the actions act on — forwarded to `onAction`. */
  meetingId: string;
  onAction: (action: ExportAction, meetingId: string) => void;
  disabled?: boolean;
  /** The trigger's look: the detail header uses the primary style. */
  primary?: boolean;
  /** Tests only: render the menu already open (no DOM to click in). */
  defaultOpen?: boolean;
}

export default function ExportMenu({ meetingId, onAction, disabled, primary, defaultOpen }: Props) {
  const [open, setOpen] = useState(Boolean(defaultOpen));
  const trigger = useRef<HTMLButtonElement | null>(null);
  const anchor = useCallback(() => trigger.current?.getBoundingClientRect() ?? null, []);
  const onClose = useCallback((reason: MenuCloseReason) => {
    setOpen(false);
    // Escape hands focus back to the trigger, the way a native menu does.
    if (reason === "escape") trigger.current?.focus();
  }, []);
  return (
    <div className="export-menu" data-export-menu={meetingId}>
      <button
        ref={trigger}
        type="button"
        className={primary ? "primary" : "ghost"}
        disabled={disabled}
        aria-haspopup="menu"
        aria-expanded={open}
        onClick={() => setOpen((o) => !o)}
      >
        {EXPORT_TRIGGER_LABEL} ▾
      </button>
      {open && (
        <FloatingMenu
          anchor={anchor}
          align="end"
          label="Export this meeting"
          name="export"
          meetingId={meetingId}
          items={exportItems(meetingId, onAction)}
          onClose={onClose}
          ignore={trigger}
        />
      )}
    </div>
  );
}

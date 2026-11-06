/**
 * WF-18 — the Shortcuts editor that cannot conflict: the TypeScript twin of
 * Rust `shortcuts::editor`, pure so vitest covers it. The Settings screen uses
 * it to show a warning BEFORE saving; `save_settings` re-runs the Rust rules,
 * so the backend stays the authority (a refusal there reaches the toast).
 *
 * Rules: up to four shortcuts per always-global action; never a reserved macOS
 * combo (⌘C, ⌘V, ⌘Tab …, the shared reservedChords.json list); a chord another
 * action holds is an overlap warning and is not applied; the last shortcut of
 * an action cannot be removed; Reset to default puts back the shipped chord.
 */
import type { AppSettings, ChordSetting, ExtraBinding } from "./appTypes";
import { formatChord, reservedChordReason } from "./hotkeyRecorder";

export const MAX_BINDINGS_PER_ACTION = 4;
export const LAST_BINDING_REFUSAL =
  "every action keeps at least one shortcut — add another before removing this one";

export type EditableAction =
  | "dictate_toggle"
  | "cancel"
  | "record_meeting"
  | "hands_free_toggle"
  | "copy_last"
  | "open_scratchpad";

type PrimaryField =
  | "dictateToggle"
  | "cancelChord"
  | "meetingChord"
  | "handsFreeChord"
  | "copyLastChord"
  | "scratchpadChord";

const chord = (code: string, ctrl: boolean, alt: boolean, shift: boolean, cmd: boolean): ChordSetting =>
  ({ code, ctrl, alt, shift, cmd });

/** Each editable action: title, primary settings field, shipped default (Rust `shortcuts::ALL`). */
export const ACTIONS: ReadonlyArray<{
  action: EditableAction;
  title: string;
  field: PrimaryField;
  default: ChordSetting;
  multi: boolean;
}> = [
  { action: "dictate_toggle", title: "Dictate (toggle)", field: "dictateToggle", default: chord("Space", true, true, false, true), multi: false },
  { action: "cancel", title: "Cancel take", field: "cancelChord", default: chord("Escape", false, false, false, false), multi: false },
  { action: "record_meeting", title: "Record meeting", field: "meetingChord", default: chord("KeyM", true, false, false, true), multi: true },
  { action: "hands_free_toggle", title: "Hands-free (toggle)", field: "handsFreeChord", default: chord("KeyH", true, false, false, true), multi: true },
  { action: "copy_last", title: "Copy last transcript", field: "copyLastChord", default: chord("KeyC", true, false, false, true), multi: true },
  { action: "open_scratchpad", title: "Open Scratchpad", field: "scratchpadChord", default: chord("KeyJ", true, false, false, true), multi: true },
];

/** Fixed Yap chords no action may take (paste last ⌃⌘V, undo AI edit ⌃⌘Z, legacy ⌘⇧V). */
const FIXED: ReadonlyArray<ChordSetting> = [
  chord("KeyV", true, false, false, true),
  chord("KeyZ", true, false, false, true),
  chord("KeyV", false, false, true, true),
];

export const sameChord = (a: ChordSetting, b: ChordSetting): boolean =>
  a.code === b.code && !!a.ctrl === !!b.ctrl && !!a.alt === !!b.alt && !!a.shift === !!b.shift && !!a.cmd === !!b.cmd;

const spec = (a: EditableAction) => ACTIONS.find((x) => x.action === a)!;

/** Every shortcut `action` holds: primary (or default) first, then its extras. */
export function bindingsOf(s: Partial<AppSettings>, action: EditableAction): ChordSetting[] {
  const sp = spec(action);
  const out: ChordSetting[] = [(s[sp.field] as ChordSetting | null | undefined) ?? sp.default];
  if (sp.multi) {
    for (const e of s.extraChords ?? []) {
      if (e.action === action && !out.some((c) => sameChord(c, e.chord))) out.push(e.chord);
    }
  }
  return out;
}

export type Verdict =
  | { kind: "ok" }
  | { kind: "overlaps"; text: string; other: EditableAction }
  | { kind: "refused"; text: string };

/** Judge adding `candidate` to `action` (same order and words as Rust `editor::check_add`). */
export function checkAdd(s: Partial<AppSettings>, action: EditableAction, candidate: ChordSetting): Verdict {
  const sp = spec(action);
  const current = bindingsOf(s, action);
  if (!sp.multi) return { kind: "refused", text: `${sp.title} has one shortcut — change it instead of adding another` };
  if (current.length >= MAX_BINDINGS_PER_ACTION) {
    return { kind: "refused", text: `${sp.title} already has ${MAX_BINDINGS_PER_ACTION} shortcuts — remove one first` };
  }
  const label = formatChord(candidate);
  if (current.some((c) => sameChord(c, candidate))) {
    return { kind: "refused", text: `${label} is already one of ${sp.title}'s shortcuts` };
  }
  const reserved = reservedChordReason(candidate, false);
  if (reserved) return { kind: "refused", text: reserved };
  if (FIXED.some((c) => sameChord(c, candidate))) {
    return { kind: "refused", text: `${label} is already a Yap shortcut — pick another chord` };
  }
  for (const other of ACTIONS) {
    if (other.action === action) continue;
    if (bindingsOf(s, other.action).some((c) => sameChord(c, candidate))) {
      return {
        kind: "overlaps",
        other: other.action,
        text: `${label} is already ${other.title} — remove it there first, or pick another chord`,
      };
    }
  }
  return { kind: "ok" };
}

/** Write a binding list for `action` back into settings (primary field + extras). */
export function withBindings(s: AppSettings, action: EditableAction, list: ChordSetting[]): AppSettings {
  const sp = spec(action);
  const [first, ...rest] = list;
  const primary = first && !sameChord(first, sp.default) ? first : null;
  const others = (s.extraChords ?? []).filter((e) => e.action !== action);
  const mine: ExtraBinding[] = sp.multi
    ? rest.map((c) => ({ action: action as ExtraBinding["action"], chord: c }))
    : [];
  return { ...s, [sp.field]: primary, extraChords: [...others, ...mine] };
}

/** Add `candidate` when the verdict allows it; the verdict is returned either way. */
export function addBinding(s: AppSettings, action: EditableAction, candidate: ChordSetting): { next: AppSettings | null; verdict: Verdict } {
  const verdict = checkAdd(s, action, candidate);
  if (verdict.kind !== "ok") return { next: null, verdict };
  return { next: withBindings(s, action, [...bindingsOf(s, action), candidate]), verdict };
}

/** Remove shortcut `index` of `action`; the last one is refused. */
export function removeAt(s: AppSettings, action: EditableAction, index: number): { next: AppSettings | null; error: string | null } {
  const list = bindingsOf(s, action);
  if (index < 0 || index >= list.length) return { next: null, error: `no shortcut at position ${index + 1}` };
  if (list.length <= 1) return { next: null, error: LAST_BINDING_REFUSAL };
  return { next: withBindings(s, action, list.filter((_, i) => i !== index)), error: null };
}

/** Reset one action to its shipped chord (its extras go too). */
export function resetToDefault(s: AppSettings, action: EditableAction): AppSettings {
  return withBindings(s, action, [spec(action).default]);
}

/** Reset every editable action to its shipped chord. */
export function resetAllToDefault(s: AppSettings): AppSettings {
  return ACTIONS.reduce((acc, a) => resetToDefault(acc, a.action), { ...s, extraChords: [] } as AppSettings);
}

/** The live line under the hold rows: "Double-tap fn for hands-free" (Rust `editor::hands_free_line`). */
export function handsFreeLine(pttBinding: string | null | undefined): string {
  const key = pttBinding === "fn" ? "fn" : pttBinding === "both" || pttBinding === "fn_or_fn_control" ? "fn or fn⌃" : "fn⌃";
  return `Double-tap ${key} for hands-free`;
}

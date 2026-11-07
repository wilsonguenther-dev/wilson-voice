import { useEffect, useMemo, useRef, useState, type KeyboardEvent as ReactKeyboardEvent } from "react";
import BootNote from "../BootNote";
import LanguageSettings from "../LanguageSettings";
import Companion from "./settings/Companion";
import Notifications from "./settings/Notifications";
import News from "./settings/News";
import Dictation from "./settings/Dictation";
import Snippets from "./settings/Snippets";
import Audio from "./settings/Audio";
import Shortcut from "./settings/Shortcut";
import Advanced from "./settings/Advanced";
import Privacy from "./settings/Privacy";
import Meetings from "./settings/Meetings";
import MeetingPrefs from "./settings/MeetingPrefs";
import License from "./settings/License";
import Diagnostics from "./settings/Diagnostics";
import ModelProviders from "../settings/ModelProviders";
import CloudRouteSettings from "../settings/CloudRoute";
import { useAppCtx } from "../appShell";
import { viewState } from "../viewState";
import { ErrorState, LoadingState } from "../ViewStates";
import {
  LIST_SECTIONS,
  SECTION_HOME_TAB,
  type SettingsBlock,
  type SettingsHit,
  type SettingsSection,
  searchSettings,
  sectionDef,
  sectionForTab,
  stepSection,
} from "../settingsIndex";

/** How long a search jump keeps its highlight (spec §1.4: 2 s). */
export const HIT_HIGHLIGHT_MS = 2000;

const prefersReducedMotion = () =>
  typeof window !== "undefined" &&
  typeof window.matchMedia === "function" &&
  window.matchMedia("(prefers-reduced-motion: reduce)").matches;

/** A modal layer owns Escape; Settings only answers it when none is open. */
const modalOpen = () =>
  typeof document !== "undefined" &&
  document.querySelector('[role="dialog"], [aria-modal="true"], dialog[open]') != null;

/** One block of a section: a pane, or one part of a pane. */
function Block({ id, openDictionary }: { id: SettingsBlock; openDictionary: () => void }) {
  switch (id) {
    case "shortcut":
      return <Shortcut />;
    case "microphone":
      return <Audio part="input" />;
    case "language":
      return (
        <div className="panel" data-row>
          <h3>Language</h3>
          {/* Y10-A: the language SET, driven by the installed model's probe. */}
          <LanguageSettings />
        </div>
      );
    case "startup":
      return <Advanced part="startup" />;
    case "dictation":
      return <Dictation />;
    case "audio-cleanup":
      return <Audio part="cleanup" />;
    case "snippets":
      return (
        <>
          <BootNote keys={["list_snippets"]} />
          <Snippets />
        </>
      );
    case "dictionary-link":
      return (
        <div className="panel" data-row>
          <h3>Dictionary</h3>
          <p className="settings-hint">
            Words and names Yap should always spell your way live on their own screen.
          </p>
          <div className="actions">
            <button type="button" onClick={openDictionary}>
              Open Dictionary
            </button>
          </div>
        </div>
      );
    case "meetings":
      return <Meetings part="call" />;
    case "meeting-kinds":
      return <MeetingPrefs part="kinds" />;
    case "meeting-retention":
      return <Privacy part="retention" />;
    case "speakers":
      return <MeetingPrefs part="speakers" />;
    case "meeting-exports":
      return <Meetings part="exports" />;
    case "companion":
      return <Companion />;
    case "notifications":
      return <Notifications />;
    case "news":
      // yap-1106-news-prefs-onboarding — briefing on/off, topics, last refresh.
      return <News />;
    case "speech-models":
      return <Advanced part="models" />;
    case "providers":
      // yap-byom-0 — where notes and polish run, and whether that leaves this Mac;
      // yap-1106-byom-cloud-fast-path — your own key's cloud route sits with it.
      return (
        <>
          <ModelProviders />
          <CloudRouteSettings />
        </>
      );
    case "privacy":
      return <Privacy part="main" />;
    case "license":
      return <License />;
    case "diagnostics":
      return <Diagnostics />;
  }
}

/**
 * The main column while a search is typed: one button per matching control
 * (label, the section it lives in, its help line), or the empty state with
 * Clear search. Return in the field jumps to the first hit.
 */
export function SearchResults({
  query,
  hits,
  onJump,
  onClear,
}: {
  query: string;
  hits: readonly SettingsHit[];
  onJump: (hit: SettingsHit) => void;
  onClear: () => void;
}) {
  return (
    <div className="settings-results" aria-live="polite">
      {hits.length === 0 ? (
        <div className="settings-empty" data-settings-empty>
          <p>No setting matches '{query.trim()}'.</p>
          <button type="button" onClick={onClear}>
            Clear search
          </button>
        </div>
      ) : (
        <ul className="settings-result-list">
          {hits.map((h) => (
            <li key={`${h.block}:${h.label}`}>
              <button type="button" className="settings-result" data-settings-hit={h.block} onClick={() => onJump(h)}>
                <strong>{h.label}</strong>
                <span className="settings-result-where">{sectionDef(h.section).label}</span>
                <span className="settings-hint">{h.help}</span>
              </button>
            </li>
          ))}
        </ul>
      )}
    </div>
  );
}

/**
 * yap-1106-settings-redesign (§14) — a slim left list of five sections with a
 * search field above it, one main column that scrolls on its own, and a pinned
 * footer with "Account & help" and Done. Done (or Escape) returns to the screen
 * you came from. Search covers every control's label and help text and jumps
 * to the control, highlighting it for 2 s.
 */
export default function Settings() {
  const { bootError, booting, leaveSettings, refreshAll, setNav, settings, settingsTab, setSettingsTab } =
    useAppCtx();
  const [query, setQuery] = useState("");
  const [pending, setPending] = useState<SettingsHit | null>(null);
  const searchRef = useRef<HTMLInputElement>(null);
  const mainRef = useRef<HTMLDivElement>(null);
  const listRef = useRef<HTMLElement>(null);
  const section = sectionForTab(settingsTab);
  const def = sectionDef(section);
  const hits = useMemo(() => searchSettings(query), [query]);
  const searching = query.trim().length > 0;

  const goSection = (id: SettingsSection) => {
    if (id !== section) console.info(`ui settings_section to=${id}`);
    setSettingsTab(SECTION_HOME_TAB[id]);
    mainRef.current?.scrollTo?.({ top: 0 });
  };
  const jump = (hit: SettingsHit) => {
    console.info(`ui settings_jump section=${hit.section} block=${hit.block}`);
    setQuery("");
    setSettingsTab(SECTION_HOME_TAB[hit.section]);
    setPending(hit);
  };
  const clearSearch = () => {
    setQuery("");
    searchRef.current?.focus();
  };

  // The jump lands after the section has rendered: find the control by its
  // label inside its block, bring it to the middle, focus it, flash it for 2 s.
  useEffect(() => {
    if (!pending) return;
    const raf = requestAnimationFrame(() => {
      const block = mainRef.current?.querySelector<HTMLElement>(`[data-settings-block="${pending.block}"]`);
      if (!block) {
        console.info(`ui settings_jump_missed block=${pending.block}`);
        setPending(null);
        return;
      }
      const want = pending.label.toLowerCase();
      const labelEl = Array.from(block.querySelectorAll<HTMLElement>("h3, h4, strong, .field > span, button")).find(
        (el) => (el.textContent ?? "").trim().toLowerCase().startsWith(want),
      );
      const target = labelEl?.closest<HTMLElement>(".toggle, .field") ?? labelEl?.closest<HTMLElement>(".panel") ?? block;
      target.scrollIntoView?.({ block: "center", behavior: prefersReducedMotion() ? "auto" : "smooth" });
      target.querySelector<HTMLElement>("input, select, textarea, button")?.focus({ preventScroll: true });
      target.classList.add("settings-hit");
      // The flash outlives this effect run on purpose (setPending(null) re-runs
      // it); if the node unmounts first, the class goes with it.
      setTimeout(() => target.classList.remove("settings-hit"), HIT_HIGHLIGHT_MS);
      setPending(null);
    });
    return () => cancelAnimationFrame(raf);
  }, [pending, section]);

  // One telemetry line per search the user settles on — never the query text.
  useEffect(() => {
    if (!searching) return;
    const t = setTimeout(() => console.info(`ui settings_search hits=${hits.length}`), 600);
    return () => clearTimeout(t);
  }, [searching, hits.length, query]);

  // ⌘F focuses search; Escape clears the search, then leaves Settings — unless
  // a dialog is open or the key belongs to another field.
  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      if ((e.metaKey || e.ctrlKey) && !e.altKey && e.key.toLowerCase() === "f") {
        e.preventDefault();
        searchRef.current?.focus();
        searchRef.current?.select();
        return;
      }
      if (e.key !== "Escape" || e.defaultPrevented || modalOpen()) return;
      const t = e.target as HTMLElement | null;
      const inOtherField =
        t != null && t !== searchRef.current && /^(INPUT|TEXTAREA|SELECT)$/.test(t.tagName ?? "");
      if (inOtherField) return;
      if (searchRef.current && searchRef.current.value.trim()) {
        setQuery("");
        return;
      }
      leaveSettings();
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, [leaveSettings]);

  const onListKey = (e: ReactKeyboardEvent<HTMLElement>) => {
    if (e.key !== "ArrowDown" && e.key !== "ArrowUp") return;
    e.preventDefault();
    const next = stepSection(section, e.key === "ArrowDown" ? 1 : -1);
    goSection(next);
    listRef.current?.querySelector<HTMLButtonElement>(`[data-section="${next}"]`)?.focus();
  };

  // PANEL 2026-09-12 — Settings has no empty state either: there is nothing it
  // could be empty OF. Loading + error, and its settled state is the sections.
  // Y5-B fix round (#192 BLOCKING) - bootError is shared by refreshAll's 11
  // calls plus loadHistory, so it only replaces THIS view when this view's own
  // data never landed. A later refresh that fails (model-ready edge,
  // silent_capture) keeps already-loaded data on screen; the bridge message
  // is in status, the same as on main.
  const state = viewState(settings, booting, settings == null ? bootError : null);
  if (state === "error")
    return (
      <ErrorState
        data-error-state="settings"
        view="settings"
        error={bootError}
        actionLabel="Read settings again"
        onAction={() => void refreshAll()}
      />
    );
  if (state === "loading")
    return <LoadingState data-loading-state="settings" noun="settings" rows={5} onRetry={() => void refreshAll()} />;
  return (
    <div className="settings" data-settings-section={section}>
      <aside className="settings-side">
        <div className="settings-search" role="search">
          <input
            ref={searchRef}
            type="search"
            placeholder="Search settings"
            aria-label="Search settings"
            aria-controls="settings-main"
            value={query}
            onChange={(e) => setQuery(e.target.value)}
            onKeyDown={(e) => {
              if (e.key === "Enter" && hits[0]) {
                e.preventDefault();
                jump(hits[0]);
              }
            }}
          />
        </div>
        <nav className="settings-list" aria-label="Settings sections" ref={listRef} onKeyDown={onListKey}>
          {LIST_SECTIONS.map((s) => (
            <button
              key={s.id}
              type="button"
              data-section={s.id}
              className={section === s.id && !searching ? "settings-list-item active" : "settings-list-item"}
              aria-current={section === s.id ? "page" : undefined}
              tabIndex={section === s.id || (section === "account" && s.id === LIST_SECTIONS[0].id) ? 0 : -1}
              onClick={() => {
                setQuery("");
                goSection(s.id);
              }}
            >
              {s.label}
            </button>
          ))}
        </nav>
      </aside>

      <div className="settings-main" id="settings-main" ref={mainRef}>
        {searching ? (
          <SearchResults query={query} hits={hits} onJump={jump} onClear={clearSearch} />
        ) : (
          <>
            <header className="settings-head">
              <h2>{def.label}</h2>
              <p className="settings-hint">{def.hint}</p>
            </header>
            {def.blocks.map((b) => (
              <div key={b} className="settings-block" data-settings-block={b}>
                <Block id={b} openDictionary={() => setNav("dictionary")} />
              </div>
            ))}
          </>
        )}
      </div>

      <footer className="settings-done-bar">
        <button
          type="button"
          className={section === "account" && !searching ? "ghost settings-account active" : "ghost settings-account"}
          aria-current={section === "account" ? "page" : undefined}
          onClick={() => {
            setQuery("");
            goSection("account");
          }}
        >
          Account &amp; help
        </button>
        <button type="button" className="primary" onClick={() => leaveSettings()}>
          Done
        </button>
      </footer>
    </div>
  );
}

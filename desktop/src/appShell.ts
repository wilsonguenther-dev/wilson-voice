// Y5-G — the shell's state machine, lifted verbatim out of App.tsx. App.tsx is
// now composition only; the seven view modules read this through AppContext.
// No state field was renamed, no effect reordered, no listener duplicated.
import { createContext, useContext, useCallback, useEffect, useMemo, useRef, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { useModelSetup, usePolishModel } from "./ModelSetup";
import { checkForUpdate, installUpdate, type UpdateInfo } from "./updater";
import { errorText, isLicenseRequired } from "./errors";
import { useFormattingOptions } from "./formatting";
import { IDLE_MEETING, MEETING_EVENT, type MeetingStatus } from "./pill/meeting";
import { type MeetingConsent } from "./meetings/consent";
import { type SystemAudioSetup } from "./meetings/systemAudio";
import type { SupportBundlePreview, SupportSendOutcome } from "./support/bundle";
import { watchMeetingConsent } from "./meetings/consentWatch";
import { chipFor, shouldWarnTrial, trialWarningText, daysLeft as trialDaysLeft, type LicenseStatus } from "./license/status";
import { type Nav, type SettingsTab, readTrialWarnedFor, writeTrialWarnedFor, HISTORY_LIMIT, type AppSettings, type AppStatus, type PermissionReport, type TranscriptEntry, type FailedDictation, type CrashEvent, type EngineStatus, type Insights, type Meeting, type KindChoice, type MeetingDetail, type DayCount, monthName, type DictEntry, type DictCandidate, type Snippet, type ScratchNote } from "./appTypes";

export function useAppShell() {
  const [nav, setNav] = useState<Nav>("home");
  const [userName, setUserName] = useState("");
  useEffect(() => {
    invoke<string>("user_display_name").then(setUserName).catch(() => {});
  }, []);
  // YV27 — which Settings sub-panel is showing (segmented sub-nav).
  const [settingsTab, setSettingsTab] = useState<SettingsTab>("companion");
  const [status, setStatus] = useState<AppStatus>({
    recording: false,
    busy: false,
    lastError: null,
    message: "Loading…",
    accessibility: false,
    hotkeyRegistered: false,
    modelReady: false,
    secureInputBlocked: false,
    secureInputDetail: null,
  });
  // YP3 — the entitlement this Mac has right now, and whether the warm purchase
  // sheet is up. `null` until the first `license_status` lands: the chip and the
  // License tab render nothing rather than guessing "trial" for a paying user.
  const [license, setLicense] = useState<LicenseStatus | null>(null);
  const [buyPrompt, setBuyPrompt] = useState(false);
  const [perms, setPerms] = useState<PermissionReport | null>(null);
  const [history, setHistory] = useState<TranscriptEntry[]>([]);
  // YV52 — failed takes whose audio is still recoverable, and the one the live
  // error toast is offering "Retry" for (cleared with the toast itself).
  const [failed, setFailed] = useState<FailedDictation[]>([]);
  const [retryId, setRetryId] = useState<string | null>(null);
  const [retrying, setRetrying] = useState<string | null>(null);
  // YV74 — the transcript of a take whose auto-paste produced no read receipt.
  // The backend only claims "Pasted" when the target app demonstrably read the
  // clipboard; when it didn't, the toast carries a "Copy again" action for this
  // row, because the transcript may no longer be on the clipboard (the user can
  // copy something themselves while we wait, and their copy is left alone).
  const [copyAgainId, setCopyAgainId] = useState<string | null>(null);
  // YV64 — crashes Yap read back off disk at startup. Shown in Settings →
  // Privacy & Diagnostics → Stability; an UNacknowledged one raises the single
  // launch toast below (once per launch, never a modal).
  const [crashes, setCrashes] = useState<CrashEvent[]>([]);
  // YV75 — the engine snapshot behind Privacy & Diagnostics, read when that tab
  // is opened (no timer: the answer is only interesting while it is on screen).
  const [engine, setEngine] = useState<EngineStatus | null>(null);
  const [insights, setInsights] = useState<Insights | null>(null);
  const [dailySeries, setDailySeries] = useState<DayCount[]>([]);
  const [monthlySeries, setMonthlySeries] = useState<DayCount[]>([]);
  const [dictionary, setDictionary] = useState<DictEntry[]>([]);
  // YV47 — pending "Yap noticed: …" suggestions mined from corrections, and the
  // dictionary row currently being edited in place.
  const [candidates, setCandidates] = useState<DictCandidate[]>([]);
  const [editingTermId, setEditingTermId] = useState<string | null>(null);
  const [editTerm, setEditTerm] = useState("");
  const [editPreferred, setEditPreferred] = useState("");
  // YV47 — the history entry open in "Fix transcription", and its draft text.
  const [fixingId, setFixingId] = useState<string | null>(null);
  const [fixDraft, setFixDraft] = useState("");
  /** Y4-G — which history row has its "see what changed" panel open. */
  const [diffId, setDiffId] = useState<string | null>(null);
  /**
   * Y4-G — thumbs the user set this session, keyed by take id. The row itself
   * carries the stored value; this is the optimistic overlay so the button
   * reacts immediately instead of waiting for a history refetch.
   */
  const [feedbackEdits, setFeedbackEdits] = useState<
    Record<string, number | null>
  >({});
  // YV48 — saved snippets plus the "add" draft and the row being edited.
  const [snippets, setSnippets] = useState<Snippet[]>([]);
  const [newTrigger, setNewTrigger] = useState("");
  const [newExpansion, setNewExpansion] = useState("");
  const [editingSnippetId, setEditingSnippetId] = useState<string | null>(null);
  const [editTrigger, setEditTrigger] = useState("");
  const [editExpansion, setEditExpansion] = useState("");
  const [scratch, setScratch] = useState<ScratchNote[]>([]);
  // YV94 — the Meetings tab. Loaded when the tab is opened rather than in
  // `refreshAll`: meetings are rare compared to dictations, each list row costs
  // a segment-count subquery, and nothing on any other screen reads them.
  const [meetings, setMeetings] = useState<Meeting[]>([]);
  const [meetingQuery, setMeetingQuery] = useState("");
  // The meeting whose transcript is open. `null` = the list.
  const [openMeeting, setOpenMeeting] = useState<MeetingDetail | null>(null);
  const [meetingBusy, setMeetingBusy] = useState(false);
  /**
   * YV125 — the answer to "what kind of meeting is this?", held only until the
   * next start. It begins as `unknown` (the skip answer) so the record button
   * works exactly as it did for a user who never looks at the picker: this is a
   * hint that improves diarization, never a gate.
   */
  const [meetingKind, setMeetingKind] = useState<string>("unknown");
  /**
   * The picker's options, fetched from Rust rather than written out again here
   * — the tray submenu renders the SAME `meeting_control::KIND_PICKER`, so a
   * re-worded or added choice cannot appear on one surface and not the other.
   * Empty until it arrives (and if the call fails), which hides the picker and
   * leaves the record button working on the skip answer.
   */
  const [meetingKinds, setMeetingKinds] = useState<KindChoice[]>([]);
  // YV95 — is a meeting recording right now? Driven by the backend's 1 Hz
  // `meeting` emit (OS-12 fix 1: no timer in the webview), plus one
  // `meeting_status` call on mount so a window opened mid-meeting is correct
  // immediately rather than a second later.
  const [meetingStatus, setMeetingStatus] = useState<MeetingStatus>(IDLE_MEETING);
  // Delete is destructive and irreversible (rows, search index and the audio),
  // so the button arms first and the second click commits.
  const [confirmDeleteMeeting, setConfirmDeleteMeeting] = useState<string | null>(
    null,
  );
  // YV96 — the one-time capture notice. `null` until the backend answers, which
  // is why `shouldOpenNotice` treats `null` as "do not open".
  const [consent, setConsent] = useState<MeetingConsent | null>(null);
  // YV102 — what Yap currently believes about system-audio permission, and
  // whether this Mac can hold that permission at all (YV101's 14.4 gate,
  // answered by `notetaker_status`). Both `null` until the backend replies;
  // `setupState` treats that as "not run", never as "denied".
  const [sysAudio, setSysAudio] = useState<SystemAudioSetup | null>(null);
  const [sysAudioGate, setSysAudioGate] = useState<{
    available: boolean;
    message: string;
  }>({
    available: true,
    message: "System audio capture requires macOS 14.4 or later",
  });
  // The pre-warm is a real 200 ms CoreAudio tap. The button disables while it
  // runs so a double-press cannot open two.
  const [sysAudioBusy, setSysAudioBusy] = useState(false);
  // Why the sheet is open, not just whether: `recording` is the first-capture
  // showing (a meeting is running behind it), `review` is a deliberate re-read
  // from Settings → Privacy. The sheet says the true thing in each case, and
  // this avoids mirroring YV95's meeting status into a second piece of state.
  const [consentOpen, setConsentOpen] = useState<null | "recording" | "review">(
    null,
  );
  // Read by the `meeting` subscription, which is deliberately subscribed ONCE:
  // re-subscribing whenever the consent state changes would drop ticks. Synced
  // in an effect rather than written during render, which React reserves for
  // itself.
  // YV98 — the crash-report sheet. `null` means closed; an open sheet with a
  // `null` preview is the second or two the backend spends building the bundle.
  // The preview is REAL redacted content, so it is never fabricated here.
  const [supportOpen, setSupportOpen] = useState(false);
  const [supportPreview, setSupportPreview] =
    useState<SupportBundlePreview | null>(null);
  const [supportBusy, setSupportBusy] = useState(false);
  const consentRef = useRef<MeetingConsent | null>(null);
  useEffect(() => {
    consentRef.current = consent;
  }, [consent]);
  const [settings, setSettings] = useState<AppSettings | null>(null);
  // YV62 — the signature block is edited locally and saved on blur. A save per
  // keystroke would rewrite settings.json on every character of a multi-line
  // block; `null` means "show whatever is stored".
  const [signatureDraft, setSignatureDraft] = useState<string | null>(null);
  const [query, setQuery] = useState("");
  const [flash, setFlash] = useState<string | null>(null);
  const [bootError, setBootError] = useState<string | null>(null);
  const [newTerm, setNewTerm] = useState("");
  const [newPreferred, setNewPreferred] = useState("");
  const [noteTitle, setNoteTitle] = useState("Scratchpad");
  const [noteBody, setNoteBody] = useState("");
  const [activeNoteId, setActiveNoteId] = useState<string | null>(null);
  // YV15 — key-capture for the push-to-talk shortcut. `capturing` arms a global
  // keydown listener; `captureHint` shows the live result / validation message.
  const [capturing, setCapturing] = useState(false);
  const [captureHint, setCaptureHint] = useState<string | null>(null);
  // YV44 — the pending release, if the check found one the user hasn't skipped.
  // Non-blocking: it renders as a banner and nothing downloads until they click.
  const [update, setUpdate] = useState<UpdateInfo | null>(null);
  const [installing, setInstalling] = useState(false);
  const [installedVersion, setInstalledVersion] = useState<string | null>(null);
  // YV78 — clearing history now scrubs the file (FTS rebuild + VACUUM), which
  // on a large history is not instant. The button stays disabled until the
  // command resolves so nothing reports success while bytes are still on disk.
  const [clearing, setClearing] = useState(false);
  // YV54 — silent model setup. The onboarding overlay owns the auto-download
  // during first run, so this instance only takes it over once the user IS
  // onboarded: an install that lands here with no model (fresh profile, a
  // deleted file, an update on a machine that skipped setup) starts fetching
  // the catalog's recommendation on its own, and says so with the same slim
  // ribbon. It also backs the Settings → Advanced picker.
  const modelSetup = useModelSetup({
    autoDownload: settings?.onboarded === true,
  });

  // SEC-C — the OPTIONAL polish model. No `autoDownload` twin: nothing here
  // fetches 1.1 GB unless the user presses the button in Settings → Advanced.
  const polishSetup = usePolishModel();

  // Y4-H — the level labels, their one-line descriptions, and the three named
  // polish speeds, all built backend-side from `CleanupLevel::runs_*`.
  const formatting = useFormattingOptions();

  // Read the live query without making `refreshAll` depend on it — otherwise the
  // mount effect that registers event listeners re-runs on every keystroke,
  // tearing down + re-adding all listeners and dropping events in the gap.
  const queryRef = useRef(query);
  useEffect(() => {
    queryRef.current = query;
  }, [query]);

  // YV15 — while the "Set shortcut" control is armed, record the next combo the
  // user presses and map it to a supported push-to-talk binding. The dictation
  // engine binds the fn (Globe) key via a CGEvent tap, so only fn / fn⌃ are wired
  // end-to-end — Control is the reliably-detectable half of the fn⌃ gesture, so a
  // Control-inclusive combo maps to fn_control. Anything else (bare letters, or a
  // modifier we can't wire) is reported rather than silently persisted.
  useEffect(() => {
    if (!capturing) return;
    function onKey(e: KeyboardEvent) {
      e.preventDefault();
      e.stopPropagation();
      if (e.key === "Escape") {
        setCapturing(false);
        setCaptureHint("Cancelled — shortcut unchanged.");
        return;
      }
      const fn =
        e.key === "Fn" ||
        e.code === "Fn" ||
        (typeof e.getModifierState === "function" && e.getModifierState("Fn"));
      const ctrl = e.ctrlKey;
      const isModifierKey =
        e.key === "Control" ||
        e.key === "Fn" ||
        e.key === "Meta" ||
        e.key === "Alt" ||
        e.key === "Shift";
      if (ctrl && !e.metaKey && !e.altKey) {
        // fn⌃ gesture — Control is the detectable half; fn is required to talk.
        applyBinding("fn_control");
        setCaptureHint("Shortcut set to fn + Control (fn⌃).");
        setCapturing(false);
      } else if (fn) {
        applyBinding("fn");
        setCaptureHint(
          "Shortcut set to fn. Tip: set Keyboard → “Press 🌐 to → Do Nothing” so fn doesn’t open emoji.",
        );
        setCapturing(false);
      } else if (!isModifierKey) {
        setCaptureHint(
          "That’s not a hold key. Hold fn, or fn together with Control.",
        );
      } else {
        setCaptureHint(
          "Only fn-based shortcuts are wired for dictation. Hold fn, or fn + Control.",
        );
      }
    }
    window.addEventListener("keydown", onKey, true);
    return () => window.removeEventListener("keydown", onKey, true);
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [capturing, settings]);

  const loadHistory = useCallback(async (q?: string) => {
    const h = await invoke<TranscriptEntry[]>("get_history", {
      query: q || null,
      limit: HISTORY_LIMIT,
    });
    setHistory(h);
  }, []);

  // YV52 — the recoverable takes. Kept separate from `loadHistory` because the
  // backend purges expired clips on this call, so it must not ride the
  // search-as-you-type debounce.
  const loadFailed = useCallback(async () => {
    setFailed(await invoke<FailedDictation[]>("list_failed_dictations"));
  }, []);

  // YV94 — the Meetings list. `query` searches segment text through FTS5 and the
  // title through LIKE, so a meeting is findable by what it was called AND by
  // what was said in it.
  const loadMeetings = useCallback(async (q?: string) => {
    try {
      setMeetings(
        await invoke<Meeting[]>("list_meetings", { query: q || null }),
      );
    } catch (e) {
      console.error(e);
    }
  }, []);

  const openMeetingDetail = useCallback(async (id: string) => {
    try {
      setOpenMeeting(await invoke<MeetingDetail>("get_meeting", { id }));
      setConfirmDeleteMeeting(null);
    } catch (e) {
      console.error(e);
    }
  }, []);

  const refreshPerms = useCallback(async () => {
    try {
      setPerms(await invoke<PermissionReport>("get_permissions"));
    } catch (e) {
      console.error(e);
    }
  }, []);

  const refreshAll = useCallback(async () => {
    try {
      const [
        s,
        st,
        ins,
        daily,
        monthly,
        dict,
        cands,
        snips,
        notes,
        fails,
        crashRows,
      ] = await Promise.all([
          invoke<AppSettings>("get_settings"),
          invoke<AppStatus>("get_status"),
          invoke<Insights>("get_insights"),
          invoke<DayCount[]>("daily_series", { days: 365 }),
          invoke<DayCount[]>("monthly_series", { months: 12 }),
          invoke<DictEntry[]>("list_dictionary"),
          invoke<DictCandidate[]>("list_dict_candidates"),
          invoke<Snippet[]>("list_snippets"),
          invoke<ScratchNote[]>("list_scratch"),
          invoke<FailedDictation[]>("list_failed_dictations"),
          invoke<CrashEvent[]>("list_crash_events"),
        ]);
      setSettings(s);
      setStatus(st);
      setInsights(ins);
      setDailySeries(daily);
      setMonthlySeries(monthly);
      setDictionary(dict);
      setCandidates(cands);
      setSnippets(snips);
      setScratch(notes);
      setFailed(fails);
      setCrashes(crashRows);
      setBootError(null);
      await loadHistory(queryRef.current);
      await refreshPerms();
    } catch (e) {
      setBootError(String(e));
      setStatus((p) => ({
        ...p,
        message: `Bridge error: ${e}`,
        lastError: String(e),
      }));
    }
  }, [loadHistory, refreshPerms]);

  // YV94 — load (and search) the Meetings list only while that tab is open. The
  // 200 ms debounce is the same shape History's search uses: one query per pause
  // in typing, not one per keystroke.
  useEffect(() => {
    if (nav !== "meetings") return;
    let dead = false;
    const t = setTimeout(() => {
      if (!dead) loadMeetings(meetingQuery);
    }, meetingQuery ? 200 : 0);
    return () => {
      dead = true;
      clearTimeout(t);
    };
  }, [nav, meetingQuery, loadMeetings]);

  // YV96 — the one-time capture notice: read the stored acknowledgement once on
  // mount, then watch for a meeting actually starting.
  //
  // The subscription itself lives in `meetings/consentWatch.ts` so it can be
  // tested across the process boundary it depends on: the event name and the
  // payload field are a contract with Rust that no compiler checks, and a rename
  // on either side would leave this sheet unreachable with the build still
  // green. `shouldOpenNotice` inside it is idempotent, so the 1 Hz tick that
  // follows the first one costs a boolean and changes nothing.
  useEffect(
    () =>
      watchMeetingConsent({
        currentConsent: () => consentRef.current,
        onConsent: setConsent,
        onOpen: () => setConsentOpen("recording"),
      }),
    [],
  );

  // YV102 — the system-audio setup state, read once on mount alongside YV101's
  // 14.4 gate.
  //
  // Two reads, not one, because they answer different questions: `notetaker_status`
  // says whether this Mac COULD hold the permission (macOS 14.4+), and
  // `system_audio_setup` says what Yap currently believes about whether it DOES.
  // A macOS 13 Mac is "cannot" forever and must never be offered the step; a
  // macOS 15 Mac that has not run it yet is a call to action.
  //
  // Both failures are swallowed to their safe direction: an unanswered gate
  // stays `available` (the affordance shows and the backend refuses if it must
  // — `run_system_audio_setup` re-checks the gate itself), and an unanswered
  // setup read stays `null`, which `setupState` renders as "not run", never as
  // a denial.
  const refreshSystemAudio = useCallback(async () => {
    try {
      setSysAudio(await invoke<SystemAudioSetup>("system_audio_setup"));
    } catch {
      /* stays null → "not run", the safe direction */
    }
  }, []);

  useEffect(() => {
    let dead = false;
    invoke<{
      systemAudioAvailable: boolean;
      systemAudioMessage: string | null;
    }>("notetaker_status")
      .then((s) => {
        if (dead) return;
        setSysAudioGate((prev) => ({
          available: s.systemAudioAvailable,
          message: s.systemAudioMessage ?? prev.message,
        }));
      })
      .catch(() => {});
    refreshSystemAudio();
    return () => {
      dead = true;
    };
  }, [refreshSystemAudio]);

  /**
   * YV102 — run the pre-warm. **This is the permission request**; there is no
   * other one (finding OS-10).
   *
   * The contextual copy is already on screen when this fires, which is the
   * entire deliverable: macOS's alert steals focus the moment the tap starts,
   * and it only ever appears once per install.
   */
  const runSystemAudioSetup = useCallback(async () => {
    setSysAudioBusy(true);
    try {
      setSysAudio(await invoke<SystemAudioSetup>("run_system_audio_setup"));
    } catch (e) {
      toast(String(e));
    } finally {
      setSysAudioBusy(false);
    }
  }, []);

  // YV95 — the meeting status subscription. One `invoke` on mount plus the
  // backend's 1 Hz `meeting` event; when a meeting ENDS, the Meetings list is
  // reloaded so the row the user just recorded is there without a refresh.
  useEffect(() => {
    let dead = false;
    const unsubs: Array<() => void> = [];
    invoke<MeetingStatus>("meeting_status")
      .then((s) => { if (!dead) setMeetingStatus(s); })
      .catch(() => {});
    // YV125 — the three kind choices, from the one list Rust publishes.
    invoke<KindChoice[]>("meeting_kind_choices")
      .then((c) => { if (!dead) setMeetingKinds(c); })
      .catch(() => {});
    listen<MeetingStatus>(MEETING_EVENT, (e) => {
      setMeetingStatus((prev) => {
        if (prev.recording && !e.payload.recording) {
          // The meeting just closed out — pull the row it wrote.
          loadMeetings(meetingQuery);
          refreshInsights();
        }
        return e.payload;
      });
    }).then((u) => (dead ? u() : unsubs.push(u)));
    return () => { dead = true; unsubs.forEach((u) => u()); };
    // `loadMeetings`/`meetingQuery` are read inside the updater; re-subscribing
    // on every keystroke in the search box would drop events mid-meeting, so
    // this deliberately subscribes ONCE.
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, []);

  // YV75 — refresh the engine snapshot (ASR model + polish sidecar) whenever
  // Privacy & Diagnostics is opened. A sidecar that was cold a minute ago is
  // usually warm by now, so the answer is read at the moment it is asked for.
  useEffect(() => {
    if (nav !== "settings" || settingsTab !== "privacy") return;
    let dead = false;
    invoke<EngineStatus>("engine_status")
      .then((e) => !dead && setEngine(e))
      .catch(() => !dead && setEngine(null));
    return () => {
      dead = true;
    };
  }, [nav, settingsTab]);

  useEffect(() => {
    refreshAll();
    // A synchronous cleanup can run before these listen() promises resolve
    // (StrictMode double-mount). A `dead` flag unsubscribes any listener that
    // lands after teardown so no native listener leaks.
    let dead = false;
    const unsubs: Array<() => void> = [];
    listen<AppStatus>("status", (e) => setStatus(e.payload)).then((u) =>
      dead ? u() : unsubs.push(u),
    );
    // PERM-E — the watcher emits this ONLY on a transition, so this re-read
    // happens when a grant actually changed and never on a timer.
    listen("permission_changed", () => {
      void refreshPerms();
    }).then((u) => (dead ? u() : unsubs.push(u)));
    listen<boolean>("recording", (e) =>
      setStatus((s) => ({
        ...s,
        recording: e.payload,
        message: e.payload
          ? "Recording… release fn⌃ or click Stop"
          : s.message,
      })),
    ).then((u) => (dead ? u() : unsubs.push(u)));
    listen<TranscriptEntry>("transcript", async (e) => {
      setHistory((h) =>
        [e.payload, ...h.filter((x) => x.id !== e.payload.id)].slice(
          0,
          HISTORY_LIMIT,
        ),
      );
      try {
        setInsights(await invoke("get_insights"));
        setDictionary(await invoke("list_dictionary"));
      } catch {
        /* ignore */
      }
    }).then((u) => (dead ? u() : unsubs.push(u)));
    listen<string>("paste_outcome", (e) => {
      setFlash(e.payload);
      setCopyAgainId(null);
      setTimeout(() => setFlash(null), 2800);
    }).then((u) => (dead ? u() : unsubs.push(u)));
    // YV74 — the ⌘V went out but nothing read the clipboard, so the transcript
    // never reached the app the user was typing into. It arrives right after
    // the `paste_outcome` line that explains why, and turns that toast into an
    // action: one click puts the text back on the clipboard.
    listen<string>("paste_failed", (e) => {
      const id = e.payload;
      setCopyAgainId(id);
      setTimeout(() => setCopyAgainId((c) => (c === id ? null : c)), 2800);
    }).then((u) => (dead ? u() : unsubs.push(u)));
    // YV33 — a failed take must be visible IN the app, not only as a macOS
    // notification the user may have muted (or never sees while Yap is focused).
    // YV52 — the take's audio is kept when it fails, so the payload carries the
    // recoverable row: the toast turns into "… Retry", and the take shows up in
    // the History recovery section until it is retried or discarded.
    listen<{ message: string; failed?: FailedDictation | null }>(
      "transcript_error",
      (e) => {
        setFlash(e.payload?.message || "Transcription failed");
        setTimeout(() => setFlash(null), 4000);
        const row = e.payload?.failed;
        if (!row) return;
        setFailed((f) => [row, ...f.filter((x) => x.id !== row.id)]);
        setRetryId(row.id);
        setTimeout(() => setRetryId((id) => (id === row.id ? null : id)), 4000);
      },
    ).then((u) => (dead ? u() : unsubs.push(u)));
    // PERM-D — a take that captured DIGITAL SILENCE. This is NOT a transcription
    // failure (nothing was ever transcribed) and NOT the no-speech toast (the
    // user did nothing wrong), so it gets its own channel and its own action:
    // the permission screen when macOS is the reason, and the input-device
    // sentence when the grant is fine and the device is the suspect.
    listen<{
      code: string;
      status: string;
      needsPermission: boolean;
      message: string;
      failed?: FailedDictation | null;
    }>("take_failed", (e) => {
      const p = e.payload;
      if (!p || p.code !== "silent_capture") return;
      setFlash(p.message || "That take recorded silence.");
      setTimeout(() => setFlash(null), 5000);
      // The grant is the reason — a toast that vanishes is the wrong surface.
      // Show the screen that can actually fix it.
      if (p.needsPermission) {
        setNav("permissions");
        refreshAll();
      }
      // The clip was preserved (YV52 lifecycle), so History offers it back.
      const row = p.failed;
      if (!row) return;
      setFailed((f) => [row, ...f.filter((x) => x.id !== row.id)]);
    }).then((u) => (dead ? u() : unsubs.push(u)));
    // Y3-D — the user cancelled a take. NOT an error channel: no red state, no
    // notification. The clip was parked under the recovery lifecycle, so the
    // row rides along and History offers "transcribe it after all".
    listen<{ message: string; failed?: FailedDictation | null }>(
      "take_cancelled",
      (e) => {
        const p = e.payload;
        setFlash(p?.message || "Cancelled");
        setTimeout(() => setFlash(null), 2500);
        const row = p?.failed;
        if (!row) return;
        setFailed((f) => [row, ...f.filter((x) => x.id !== row.id)]);
      },
    ).then((u) => (dead ? u() : unsubs.push(u)));
    // Menu-bar "Settings…" jumps the app to the Settings view (YV26).
    listen<string>("navigate", (e) => {
      const dest = e.payload as Nav;
      setNav(dest);
    }).then((u) => (dead ? u() : unsubs.push(u)));
    // Tray "Keyboard Shortcuts…" jumps straight to the Shortcut sub-tab.
    listen<string>("settings-tab", (e) => {
      setSettingsTab(e.payload as SettingsTab);
    }).then((u) => (dead ? u() : unsubs.push(u)));
    // YV65 — dragging the pill to a screen edge persists the new dock itself, so
    // the Screen position picker has to follow the pill rather than the other way
    // round. Mirror ONLY that field: the rest of this screen is edited live and
    // must never be overwritten by a broadcast landing mid-edit.
    listen<AppSettings>("settings", (e) => {
      const dock = e.payload?.pillPosition;
      if (!dock) return;
      setSettings((s) => (s && s.pillPosition !== dock ? { ...s, pillPosition: dock } : s));
    }).then((u) => (dead ? u() : unsubs.push(u)));
    // YP3 — licensing. `license` is emitted on activation, removal and every
    // revocation refresh that changed something; `license_required` is emitted
    // by the gate itself, which is the ONLY way the hotkey / pill / tray paths
    // (none of which can surface a returned error) can say why nothing
    // happened. Both are read-only here: the sheet is a prompt, never a lock.
    invoke<LicenseStatus>("license_status")
      .then((s) => setLicense(s))
      .catch(() => {
        /* a licensing read that fails must never block the app from booting */
      });
    listen<LicenseStatus>("license", (e) => setLicense(e.payload)).then((u) =>
      dead ? u() : unsubs.push(u),
    );
    listen<LicenseStatus>("license_required", (e) => {
      setLicense(e.payload);
      setBuyPrompt(true);
    }).then((u) => (dead ? u() : unsubs.push(u)));
    return () => {
      dead = true;
      unsubs.forEach((u) => u());
    };
  }, [refreshAll]);

  useEffect(() => {
    const t = setTimeout(() => {
      loadHistory(query).catch(() => {});
    }, 200);
    return () => clearTimeout(t);
  }, [query, loadHistory]);

  // YV54 — a silently-downloaded model landing changes what the BACKEND reports
  // (status `modelReady`, the Permissions ASR row), and selecting one emits
  // `settings`, not `status`. Re-read once on the false→true edge only, so a
  // launch that already had a model does not pay for a second boot refresh.
  const modelIsReady = modelSetup.ready;
  const wasModelReady = useRef<boolean | null>(null);
  useEffect(() => {
    const prev = wasModelReady.current;
    wasModelReady.current = modelIsReady;
    if (prev === false && modelIsReady) refreshAll();
  }, [modelIsReady, refreshAll]);

  // YV64 — if the backend read a crash off disk that the user has not seen yet,
  // say so ONCE, quietly. A crash that already happened is not worth a modal or
  // a second interruption, so this is the same flash strip every other
  // background message uses, guarded by a ref so a later refresh (a model
  // landing, a reconnect) cannot repeat it within the same launch.
  const crashToastShown = useRef(false);
  useEffect(() => {
    if (crashToastShown.current) return;
    if (!crashes.some((c) => !c.acknowledged)) return;
    crashToastShown.current = true;
    setFlash("Yap had a problem last session — see Diagnostics");
    // Same fire-and-forget shape as the other background flashes: no cleanup,
    // so a later `crashes` update cannot cancel the clear and strand the strip.
    setTimeout(() => setFlash(null), 6000);
  }, [crashes]);

  // YP3 — the trial's ONE warning.
  //
  // A countdown that reappears at every launch, or every day of the last week,
  // is nagware, and the person it annoys most is the one who already decided to
  // buy. So: a single flash, the first time the trial is inside three days,
  // recorded against that trial's expiry so it can never fire twice — not on
  // the next launch, not on the last day. Everything else about the trial lives
  // in the always-on chip, which says nothing until you look at it.
  const trialWarnShown = useRef(false);
  useEffect(() => {
    if (trialWarnShown.current || !license) return;
    if (!shouldWarnTrial(license, readTrialWarnedFor())) return;
    trialWarnShown.current = true;
    writeTrialWarnedFor(license.trial_expires_at_ms);
    setFlash(trialWarningText(trialDaysLeft(license)));
    setTimeout(() => setFlash(null), 6000);
  }, [license]);

  // YV44 — one launch-time check that ONLY looks. The backend gates it on the
  // `checkUpdates` setting and on "skip this version", returns the release
  // without touching a byte of it, and answers null when there is nothing to
  // offer. A failed check (offline, endpoint down) is swallowed: an update is a
  // convenience, never something to interrupt the user with at startup.
  useEffect(() => {
    let dead = false;
    checkForUpdate()
      .then((found) => {
        if (!dead) setUpdate(found);
      })
      .catch(() => {
        /* silent by design — the manual check in Settings reports failures */
      });
    return () => {
      dead = true;
    };
  }, []);

  async function toast(msg: string) {
    setFlash(msg);
    setTimeout(() => setFlash(null), 2000);
  }

  // Every mutating command returns Result<_, String>; on Err the promise rejects.
  // Without a catch the UI silently no-ops (and throws an unhandled rejection),
  // so a failed paste/save/delete looks identical to success. Surface it.
  // YP2: `manual_toggle` can now reject with a structured `{code, message}` —
  // the license gate needs a sentence, not `[object Object]`. Only STARTING a
  // dictation can be refused; a take already running always gets to finish.
  // YP3: and when the reason IS the license, a 2-second toast is the wrong
  // surface — that person needs a way to buy, not a sentence that vanishes.
  // The gate also emits `license_required`, so the sheet is usually already up
  // by the time this lands; setting a boolean twice is a no-op.
  async function toggleRecord() {
    try {
      await invoke("manual_toggle");
    } catch (e) {
      if (isLicenseRequired(e)) setBuyPrompt(true);
      else toast(errorText(e));
    }
  }

  // ── YV94 · meeting actions ──

  /**
   * YV96 — close the one-time notice, and record that it was shown.
   *
   * Every exit routes here — the button, Esc, a click on the backdrop — because
   * a one-time notice is about display, not assent: there is no version of
   * "closed it" that means the user did not see it. Nothing on the capture path
   * waits on this call, so a failed write costs a second showing and never a
   * blocked recording.
   */
  async function closeConsentNotice() {
    setConsentOpen(null);
    if (consent && !consent.shouldShow) return;
    // Close the door before the round-trip, not after: the `meeting` tick
    // arrives every second, and a write that took longer than that would
    // re-open the sheet the user just closed. If the write then fails, this
    // session stays quiet (they have seen it) and the next launch shows it
    // again — the safe direction to be wrong in.
    consentRef.current = {
      shouldShow: false,
      acknowledgedAt: null,
      blocksRecording: false,
    };
    try {
      setConsent(await invoke<MeetingConsent>("acknowledge_meeting_consent"));
    } catch {
      /* shown again next time, which is the safe direction to fail in */
    }
  }

  /**
   * YV98 — open the crash report, which means BUILD it first.
   *
   * The sheet opens immediately with a null preview rather than after the
   * build, so a slow log read reads as "building" instead of a dead button.
   * Nothing is on disk yet either way: `preview_support_bundle` works in
   * memory, and the bytes it returns are the bytes `send_support_bundle` will
   * write — that is the only reason showing them means anything.
   */

  /**
   * Write it, then hand it to the user's mail client — or, when AppKit says it
   * cannot drive one, to Finder and the clipboard. Both outcomes report what
   * actually happened, because "sent" would be a lie in both: the compose path
   * opens a window the user still has to send, and the reveal path never opened
   * a message at all.
   */
  async function sendSupportBundle() {
    setSupportBusy(true);
    try {
      const outcome = await invoke<SupportSendOutcome>("send_support_bundle");
      setSupportOpen(false);
      setSupportPreview(null);
      toast(outcome.message);
    } catch (e) {
      toast(String(e));
    } finally {
      setSupportBusy(false);
    }
  }

  /**
   * Export one meeting as Markdown. PDF was cut for v1 (finding #33): webview
   * print pagination over a 3-hour transcript is a real time sink for a feature
   * nobody has asked for, and a .md opens in every editor and note app.
   */

  /**
   * YV95 — start or stop a meeting. The Meetings tab's button, the tray item,
   * ⌃⌘M and the pill's stop control all reach the SAME backend toggle, so the
   * four can never disagree about what pressing them does.
   */

  /**
   * Delete a meeting: rows, search index and the audio, in one go. `secure_delete`
   * means the pages are physically overwritten, so on a long meeting this is real
   * work — the button stays disabled until the backend answers rather than
   * reporting success while bytes are still on disk (the YV78 lesson).
   */


  // ── YP3 · licensing actions ──
  // The URL is NOT here: `open_purchase_page` takes no argument and opens the
  // compile-time `license::PAYMENT_LINK_URL`, so no string the webview can
  // influence ever reaches a process launch.
  async function buyYap() {
    try {
      await invoke("open_purchase_page");
    } catch (e) {
      toast(errorText(e));
    }
  }

  /** Rejects with the backend's typed `{code, message}` so the panel can show it. */


  /** From the purchase sheet: land on the key box, not just the tab. */
  function openLicenseTab() {
    setBuyPrompt(false);
    setNav("settings");
    setSettingsTab("license");
  }

  async function copyText(text: string) {
    try {
      await invoke("copy_entry", { text });
      toast("Copied");
    } catch (e) {
      toast(String(e));
    }
  }

  async function pasteText(text: string) {
    try {
      const msg = await invoke<string>("paste_entry", { text });
      toast(msg);
    } catch (e) {
      toast(String(e));
    }
    await refreshPerms();
  }

  // Best-effort insights refresh — its failure must not toast an error for a
  // mutation that already succeeded.
  async function refreshInsights() {
    try {
      const [ins, daily, monthly] = await Promise.all([
        invoke<Insights>("get_insights"),
        invoke<DayCount[]>("daily_series", { days: 365 }),
        invoke<DayCount[]>("monthly_series", { months: 12 }),
      ]);
      setInsights(ins);
      setDailySeries(daily);
      setMonthlySeries(monthly);
    } catch {
      /* leave stale insights; the mutation itself succeeded */
    }
  }


  // YV52 — re-run ASR on a failed take's saved clip. On success the row leaves
  // the recovery list and becomes a normal history entry (the backend does the
  // conversion), with the recovered text on the clipboard.
  async function retryFailed(id: string) {
    if (retrying) return;
    setRetrying(id);
    try {
      const entry = await invoke<TranscriptEntry>("retry_failed_dictation", {
        id,
      });
      setFailed((f) => f.filter((x) => x.id !== id));
      setRetryId((cur) => (cur === id ? null : cur));
      setHistory((h) =>
        [entry, ...h.filter((x) => x.id !== entry.id)].slice(0, HISTORY_LIMIT),
      );
      toast(`Recovered ${entry.wordCount} words — copied to clipboard`);
      await refreshInsights();
    } catch (e) {
      toast(String(e));
      // The clip may have been dropped (audio gone) — resync either way.
      await loadFailed().catch(() => {});
    } finally {
      setRetrying(null);
    }
  }

  // YV52 — throw a failed take away: the row and its audio both go.

  // YV78 — this really does destroy the words (secure_delete + FTS rebuild +
  // VACUUM), so it can take a moment on a big history. Hold the button until
  // the command resolves rather than clearing the list optimistically.
  async function clearAll() {
    if (clearing) return;
    if (!confirm("Clear all transcript history from SQLite?")) return;
    setClearing(true);
    try {
      await invoke("clear_history");
      setHistory([]);
    } catch (e) {
      toast(String(e));
      return;
    } finally {
      setClearing(false);
    }
    await refreshInsights();
  }

  async function saveSettings(next: AppSettings) {
    try {
      await invoke("save_settings", { settings: next });
      setSettings(next);
      toast("Settings saved");
    } catch (e) {
      toast(String(e));
    }
  }

  // YV44 — the ONLY path that installs an update: the user clicked "Install
  // now" on the prompt. The new bundle applies on the next launch, so we say so
  // instead of restarting Yap out from under a dictation.
  async function installUpdateNow() {
    setInstalling(true);
    try {
      const version = await installUpdate();
      setUpdate(null);
      setInstalledVersion(version);
    } catch (e) {
      toast(String(e));
    } finally {
      setInstalling(false);
    }
  }

  // "Skip this version" — remembered in settings, so this exact release never
  // prompts again while a newer one still will.
  async function skipUpdateVersion() {
    if (!update || !settings) return;
    const version = update.version;
    const next: AppSettings = { ...settings, skippedUpdateVersion: version };
    setUpdate(null);
    try {
      await invoke("save_settings", { settings: next });
      setSettings(next);
      toast(`Skipping Yap ${version} — you'll hear about the next one`);
    } catch (e) {
      toast(String(e));
    }
  }

  // Settings → Advanced "Check for updates now". Unlike the launch check this
  // one always answers the user, including when the check itself failed.

  // YV15/YV22 — apply a push-to-talk binding + keep the human label in sync.
  // Shared by the preset chips and the key-capture control so both stay
  // consistent. The dictation engine (ptt_macos CGEvent tap) binds the fn (Globe)
  // key, so the persisted value is always one of the supported ids:
  // fn | fn_control | both. Persist via saveSettings so the change survives quit
  // AND re-binds the live PTT engine in-session — the backend save_settings
  // command calls ptt_macos::set_binding, so a remapped shortcut works
  // immediately instead of only after a relaunch.
  async function applyBinding(id: string) {
    if (!settings) return;
    await saveSettings({
      ...settings,
      pttBinding: id,
      hotkeyLabel: id === "fn" ? "fn" : id === "both" ? "fn / fn⌃" : "fn⌃",
    });
  }

  // YV9 — persist onboarding completion (+ optional calibration sample) so the
  // first-run flow does not re-appear on next launch. Uses saveSettings per spec.
  async function finishOnboarding(sample: string | null) {
    if (!settings) return;
    await saveSettings({
      ...settings,
      onboarded: true,
      calibrationSample: sample ?? settings.calibrationSample ?? null,
    });
  }

  // YV54 — the model picker's one home: Settings → Advanced. Every "get / manage
  // a speech model" affordance in the app lands here now that onboarding no
  // longer asks the question.
  function openModelSettings() {
    setNav("settings");
    setSettingsTab("advanced");
  }

  // "Replay onboarding" — clear the gate so the flow shows again.
  async function replayOnboarding() {
    if (!settings) return;
    await saveSettings({ ...settings, onboarded: false });
  }



  // YV47 — star / unstar "always bias". Re-lists because starring changes the
  // ranking the whole screen (and the decoder prompt) is ordered by.



  // YV48 — snippets CRUD. Every mutation re-lists so the order (enabled first,
  // then A→Z) matches what the matcher will actually run with.





  // YV47 — accept a mined suggestion into the dictionary, or hide it for good.


  // YV47 — "Fix transcription": the honest correction path. The user edits a
  // known transcript and Yap diffs the result, so what it learns is exactly
  // what they changed — no clipboard sniffing, no accessibility guesswork.
  /** The thumbs value to render: this session's edit, else the stored one. */

  /**
   * Y4-G — record a local-only verdict on one take.
   *
   * Written to SQLite and read by NOTHING today. It is the only way a future
   * rules change can be scored against real dissatisfaction rather than a
   * guess, and it never leaves this Mac: this invoke writes one row, and the
   * app ships no analytics SDK.
   */







  // YP3 — `null` until the first status lands, so the header never flashes a
  // guessed state at a paying customer.
  const licenseChip = license ? chipFor(license) : null;

  const pillClass = status.recording
    ? "status-pill recording"
    : status.busy
      ? "status-pill busy"
      : status.lastError
        ? "status-pill error"
        : "status-pill ready";

  const maxWeek = useMemo(
    () => Math.max(1, ...(insights?.wordsLast7.map((d) => d.words) ?? [1])),
    [insights],
  );

  // Daily words bar chart — last ~30 days (dailySeries is oldest-first, len 365).
  const daily30 = useMemo(() => dailySeries.slice(-30), [dailySeries]);
  const maxDaily30 = useMemo(
    () => Math.max(1, ...daily30.map((d) => d.words)),
    [daily30],
  );

  // GitHub-style activity heatmap — lay the contiguous 365-day series into
  // week-columns × weekday-rows, aligning the first cell to its real weekday.
  const heat = useMemo(() => {
    if (dailySeries.length === 0) {
      return {
        cols: 0,
        cells: [] as { d: DayCount; col: number; row: number }[],
        months: [] as { key: string; label: string; col: number }[],
        max: 0,
      };
    }
    const wd0 = new Date(dailySeries[0].date + "T12:00:00").getDay(); // 0=Sun
    const max = Math.max(1, ...dailySeries.map((d) => d.words));
    const cells = dailySeries.map((d, i) => {
      const g = wd0 + i;
      return { d, col: Math.floor(g / 7), row: g % 7 };
    });
    const cols = Math.ceil((wd0 + dailySeries.length) / 7);
    // Month ticks (YV55): label the column each month starts in, dropping any
    // label that would sit on top of the previous one or run off the last
    // column (a clipped half-word is worse than no tick).
    const months: { key: string; label: string; col: number }[] = [];
    let lastYm = "";
    for (const { d, col } of cells) {
      const ym = d.date.slice(0, 7);
      if (ym === lastYm) continue;
      lastYm = ym;
      const prev = months[months.length - 1];
      if (prev && col - prev.col < 3) continue;
      if (cols - col < 3) continue; // no room left to draw it in full
      months.push({ key: ym, label: monthName(ym), col });
    }
    return { cols, cells, months, max };
  }, [dailySeries]);
  const hasActivity = useMemo(
    () => dailySeries.some((d) => d.words > 0),
    [dailySeries],
  );

  // YV55 — one honest line for the exact window the heatmap draws, so the year
  // grid is read rather than counted.
  const heatSummary = useMemo(() => {
    let words = 0;
    let sessions = 0;
    let best: DayCount | null = null;
    for (const d of dailySeries) {
      words += d.words;
      sessions += d.sessions;
      if (!best || d.words > best.words) best = d;
    }
    return { words, sessions, best: best && best.words > 0 ? best : null };
  }, [dailySeries]);

  // Monthly words bar chart — YV55: an empty month is not information, so the
  // leading run of silent months collapses into one quiet line and only months
  // that carry words are charted. The current month is always a row (today's
  // ramp has to be visible even at zero) and the chart never shrinks below 3
  // rows, so a young install still reads as a trend rather than a single bar.
  const monthlyView = useMemo(() => {
    if (monthlySeries.length === 0) {
      return { rows: [] as DayCount[], quiet: 0, max: 1 };
    }
    const last = monthlySeries.length - 1;
    const shown = new Set<number>();
    monthlySeries.forEach((d, i) => {
      if (d.words > 0 || i === last) shown.add(i);
    });
    for (let i = last; i >= 0 && shown.size < 3; i--) shown.add(i);
    const rows = monthlySeries.filter((_, i) => shown.has(i));
    const first = Math.min(...shown);
    return {
      rows,
      quiet: first, // every month before the first shown one is silent
      max: Math.max(1, ...rows.map((d) => d.words)),
    };
  }, [monthlySeries]);

  // Top apps (YV55) — each row carries its own bar, scaled to the busiest app.
  const maxApp = useMemo(
    () => Math.max(1, ...(insights?.topApps.map((a) => a.words) ?? [1])),
    [insights],
  );

  const needsPerms = perms && !perms.allCriticalOk;

  return {
    activeNoteId, applyBinding, bootError, buyPrompt, buyYap, candidates,
    captureHint, capturing, clearAll, clearing, closeConsentNotice, confirmDeleteMeeting,
    consent, consentOpen, consentRef, copyAgainId, copyText, crashToastShown,
    crashes, daily30, dailySeries, dictionary, diffId, editExpansion,
    editPreferred, editTerm, editTrigger, editingSnippetId, editingTermId, engine,
    failed, feedbackEdits, finishOnboarding, fixDraft, fixingId, flash,
    formatting, hasActivity, heat, heatSummary, history, insights,
    installUpdateNow, installedVersion, installing, license, licenseChip, loadFailed,
    loadHistory, loadMeetings, maxApp, maxDaily30, maxWeek, meetingBusy,
    meetingKind, meetingKinds, meetingQuery, meetingStatus, meetings, modelIsReady,
    modelSetup, monthlySeries, monthlyView, nav, needsPerms, newExpansion,
    newPreferred, newTerm, newTrigger, noteBody, noteTitle, openLicenseTab,
    openMeeting, openMeetingDetail, openModelSettings, pasteText, perms, pillClass,
    polishSetup, query, queryRef, refreshAll, refreshInsights, refreshPerms,
    refreshSystemAudio, replayOnboarding, retryFailed, retryId, retrying, runSystemAudioSetup,
    saveSettings, scratch, sendSupportBundle, setActiveNoteId, setBootError, setBuyPrompt,
    setCandidates, setCaptureHint, setCapturing, setClearing, setConfirmDeleteMeeting, setConsent,
    setConsentOpen, setCopyAgainId, setCrashes, setDailySeries, setDictionary, setDiffId,
    setEditExpansion, setEditPreferred, setEditTerm, setEditTrigger, setEditingSnippetId, setEditingTermId,
    setEngine, setFailed, setFeedbackEdits, setFixDraft, setFixingId, setFlash,
    setHistory, setInsights, setInstalledVersion, setInstalling, setLicense, setMeetingBusy,
    setMeetingKind, setMeetingKinds, setMeetingQuery, setMeetingStatus, setMeetings, setMonthlySeries,
    setNav, setNewExpansion, setNewPreferred, setNewTerm, setNewTrigger, setNoteBody,
    setNoteTitle, setOpenMeeting, setPerms, setQuery, setRetryId, setRetrying,
    setScratch, setSettings, setSettingsTab, setSignatureDraft, setSnippets, setStatus,
    setSupportBusy, setSupportOpen, setSupportPreview, setSysAudio, setSysAudioBusy, setSysAudioGate,
    setUpdate, setUserName, settings, settingsTab, signatureDraft, skipUpdateVersion,
    snippets, status, supportBusy, supportOpen, supportPreview, sysAudio,
    sysAudioBusy, sysAudioGate, toast, toggleRecord, trialWarnShown, update,
    userName, wasModelReady,
  };
}

export type AppCtx = ReturnType<typeof useAppShell>;
export const AppContext = createContext<AppCtx | null>(null);

/** The one way a view reaches shell state. Throws if rendered outside App. */
export function useAppCtx(): AppCtx {
  const v = useContext(AppContext);
  if (!v) throw new Error("useAppCtx must be used inside <AppContext.Provider>");
  return v;
}

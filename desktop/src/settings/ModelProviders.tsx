/**
 * yap-byom-0 — Settings → Advanced → Model providers.
 *
 * Where meeting notes and polish are run: the bundled model on this Mac (the
 * default, always present, never deletable) or a provider the user adds. Every
 * row says plainly whether it sends text off this Mac ("Leaves this Mac: Yes —
 * <host>" / "No"), and a summary line at the top names every task that does.
 *
 * The webview never holds a saved key: the backend returns "key saved" + the
 * last four characters, and Test connection takes the provider ID only — never
 * a URL or a key — so a webview bug cannot aim a stored key at a new host.
 * Kinds this build has not wired end to end are HIDDEN (no "coming soon").
 */
import { useCallback, useEffect, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { useAppCtx } from "../appShell";
import { errorText } from "../errors";
import { type ActiveLeave, leaveLines } from "../meetings/consent";

export type ProviderKind = "bundled" | "open_ai_compatible";
export type Task = "notes" | "polish";

export interface ProviderView {
  id: string;
  kind: ProviderKind;
  label: string;
  baseUrl: string;
  model: string;
  contextTokens: number;
  timeoutS: number;
  leaves: boolean;
  leavesLabel: string;
  leavesHost: string | null;
  keySaved: boolean;
  keyHint: string;
  deletable: boolean;
}

export interface ProvidersView {
  providers: ProviderView[];
  notesProvider: string;
  polishProvider: string;
  wiredKinds: ProviderKind[];
  activeLeaves: ActiveLeave[];
}

export interface TestResult {
  providerId: string;
  ok: boolean;
  latencyMs: number;
  reply: string | null;
  error: string | null;
  keyHint: string;
}

export type TestState = { running: true } | { running: false; result: TestResult };

const KIND_LABEL: Record<ProviderKind, string> = {
  bundled: "Bundled",
  open_ai_compatible: "OpenAI-compatible",
};

/** Only the providers whose kind this build has wired. */
export function visibleProviders(view: ProvidersView): ProviderView[] {
  return view.providers.filter((p) => view.wiredKinds.includes(p.kind));
}

/** "Key saved ••••WXYZ" — never more of the key than its last four. */
export function keyLine(p: ProviderView): string | null {
  if (!p.keySaved) return null;
  return p.keyHint ? `Key saved ••••${p.keyHint}` : "Key saved";
}

/** The sentence a finished Test connection shows. */
export function testLine(r: TestResult): string {
  if (r.ok) return `Connected in ${r.latencyMs} ms — replied “${r.reply ?? ""}”`;
  return `Failed after ${r.latencyMs} ms — ${r.error ?? "unknown error"}`;
}

const pickValue = (id: string) => (id === "" ? "bundled" : id);

/** The pure view — no hooks, so tests render it without a window. */
export function ModelProvidersView({
  view,
  tests,
  onTest,
  onPick,
  onDelete,
}: {
  view: ProvidersView;
  tests: Record<string, TestState>;
  onTest: (id: string) => void;
  onPick: (task: Task, id: string) => void;
  onDelete: (id: string) => void;
}) {
  const rows = visibleProviders(view);
  const summary = leaveLines(view.activeLeaves);
  const picker = (task: Task, value: string, label: string, what: string) => (
    <label className="field" data-picker={task}>
      <span>
        <strong>{label}</strong> — {what}
      </span>
      <select value={pickValue(value)} onChange={(e) => onPick(task, e.target.value)}>
        {rows.map((p) => (
          <option key={p.id} value={p.id}>
            {p.label} (Leaves this Mac: {p.leavesLabel})
          </option>
        ))}
      </select>
    </label>
  );
  return (
    <div className="panel" data-row data-model-providers>
      <h3>Model providers</h3>
      {summary.length > 0 ? (
        <p className="settings-hint" role="status" data-leaves-summary>
          <strong>{summary.join(" · ")}</strong> — that text leaves this Mac.
        </p>
      ) : (
        <p className="settings-hint" data-leaves-summary>
          Notes and polish run on this Mac. Nothing leaves it.
        </p>
      )}
      {picker("notes", view.notesProvider, "Notes use", "sends the meeting transcript")}
      {picker("polish", view.polishProvider, "Polish uses", "sends every dictation")}
      <ul className="provider-list">
        {rows.map((p) => {
          const t = tests[p.id];
          const key = keyLine(p);
          return (
            <li key={p.id} data-provider={p.id}>
              <div>
                <strong>{p.label}</strong>{" "}
                <span className="muted">
                  {KIND_LABEL[p.kind]}
                  {p.model ? ` · ${p.model}` : ""}
                </span>
              </div>
              <div className={p.leaves ? "leaves-yes" : "leaves-no"}>
                Leaves this Mac: {p.leavesLabel}
              </div>
              {key && <div className="muted tiny">{key}</div>}
              <div className="row-actions">
                <button
                  type="button"
                  disabled={t?.running === true}
                  onClick={() => onTest(p.id)}
                >
                  {t?.running ? "Testing…" : "Test connection"}
                </button>
                {p.deletable && (
                  <button type="button" onClick={() => onDelete(p.id)}>
                    Delete
                  </button>
                )}
              </div>
              {t && !t.running && (
                <p className="muted tiny" role="status" data-test-result={t.result.ok ? "ok" : "failed"}>
                  {testLine(t.result)}
                </p>
              )}
            </li>
          );
        })}
      </ul>
    </div>
  );
}

/** The panel: reads the providers, runs Test connection, saves the picks. */
export default function ModelProviders() {
  const { toast } = useAppCtx();
  const [view, setView] = useState<ProvidersView | null>(null);
  const [loadError, setLoadError] = useState<string | null>(null);
  const [tests, setTests] = useState<Record<string, TestState>>({});

  const load = useCallback(() => {
    invoke<ProvidersView>("list_model_providers")
      .then((v) => {
        setView(v);
        setLoadError(null);
      })
      .catch((e: unknown) => setLoadError(errorText(e)));
  }, []);
  useEffect(load, [load]);

  const onTest = (id: string) => {
    setTests((m) => ({ ...m, [id]: { running: true } }));
    invoke<TestResult>("test_model_provider", { providerId: id })
      .then((result) => setTests((m) => ({ ...m, [id]: { running: false, result } })))
      .catch((e: unknown) => {
        setTests((m) => {
          const next = { ...m };
          delete next[id];
          return next;
        });
        toast(`Test connection failed: ${errorText(e)}`);
      });
  };
  const onPick = (task: Task, id: string) => {
    invoke<ProvidersView>("set_model_provider_choice", { task, providerId: id })
      .then(setView)
      .catch((e: unknown) => toast(`Could not change the provider: ${errorText(e)}`));
  };
  const onDelete = (id: string) => {
    invoke<ProvidersView>("delete_model_provider", { providerId: id })
      .then(setView)
      .catch((e: unknown) => toast(`Could not delete the provider: ${errorText(e)}`));
  };

  if (!view) {
    return (
      <div className="panel" data-row data-model-providers>
        <h3>Model providers</h3>
        <p className="settings-hint">
          {loadError ? `Yap could not read your model providers: ${loadError}` : "Reading…"}
        </p>
        {loadError && (
          <button type="button" onClick={load}>
            Try again
          </button>
        )}
      </div>
    );
  }
  return (
    <ModelProvidersView
      view={view}
      tests={tests}
      onTest={onTest}
      onPick={onPick}
      onDelete={onDelete}
    />
  );
}

// yap-p0-insertion-bridge-fallback DO 3 — the inline error + retry for a boot
// command whose data is secondary to the view it sits in (the view itself keeps
// rendering). A command that IS the view's data replaces the view through
// <ErrorState> instead; see views/Dictionary / Scratchpad / Home / Insights.
import { useAppCtx, type BootKey } from "./appShell";

const NOUN: Record<BootKey, string> = {
  get_settings: "settings",
  get_status: "status",
  get_insights: "stats",
  daily_series: "daily activity",
  monthly_series: "monthly activity",
  list_dictionary: "dictionary",
  list_dict_candidates: "suggested words",
  list_snippets: "snippets",
  list_scratch: "notes",
  list_failed_dictations: "failed takes",
  list_crash_events: "crash reports",
  get_history: "history",
};

export default function BootNote({ keys }: { keys: readonly BootKey[] }) {
  const { bootErrors, retryBoot } = useAppCtx();
  const failed = keys.filter((k) => typeof bootErrors?.[k] === "string");
  if (failed.length === 0) return null;
  return (
    <p className="boot-note" role="alert" data-boot-note={failed.join(" ")}>
      Couldn&rsquo;t load {failed.map((k) => NOUN[k]).join(", ")}: {bootErrors[failed[0]]}{" "}
      <button type="button" onClick={() => void retryBoot(failed)}>
        Retry
      </button>
    </p>
  );
}

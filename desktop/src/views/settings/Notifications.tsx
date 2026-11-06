// WF-17 — Settings › Notifications. Quiet mode for the pill's soft messages:
// three switches (tips, feature news, milestones). Errors, permission problems
// and recovery messages are not switches at all — they always show — and the
// category of every pill message lives in one table (`pill/live.ts`
// `PILL_MESSAGES`), so this pane cannot drift from what the pill does.
//
// BEFORE/AFTER PNGs (integrator, headless only — never the real app): with the
// Vite dev server up on :5273, run
//   YAP_SMOKE_VIEWS=Settings YAP_SMOKE_RUN=wf17-<before|after> node scripts/smoke-windowed.mjs
// on origin/main (before: no Notifications tab) and on this branch (after).
// `captureSettingsTabs` clicks every `.settings-subnav [role=tab]`, so the new
// tab lands as `settings-notifications-<size>.png` under docs/smoke-shots/.
// The all-on state is the fixture default (no `mutedPillCategories`).
import { useAppCtx } from "../../appShell";
import { parseMutedCategories, withMuted, type MutableCategory } from "../../pill/live";

const SWITCHES: readonly { id: MutableCategory; label: string; blurb: string }[] = [
  { id: "tip", label: "Tips", blurb: "A short lesson after a take, like where to click before you talk." },
  { id: "news", label: "Feature news", blurb: "A heads-up about something Yap can do that you haven't tried." },
  { id: "milestone", label: "Milestones", blurb: "A quick cheer when you pass 100 and 1,000 takes." },
];

export default function Notifications() {
  const { saveSettings, settings } = useAppCtx();
  if (!settings) return null;
  const muted = parseMutedCategories(settings.mutedPillCategories);
  return (
    <section className="settings-panel" data-settings-pane="notifications">
      <div className="panel" data-row>
        <h3>On the pill</h3>
        {SWITCHES.map((s) => (
          <label className="toggle" key={s.id} data-quiet-switch={s.id}>
            <input
              type="checkbox"
              checked={!muted.has(s.id)}
              onChange={(e) =>
                void saveSettings({
                  ...settings,
                  mutedPillCategories: withMuted(settings.mutedPillCategories, s.id, !e.target.checked),
                })
              }
            />
            <span>
              <strong>{s.label}</strong> — {s.blurb}
            </span>
          </label>
        ))}
        <p className="settings-hint">
          Errors, permission problems and recovery messages always show. Each tip appears at most once every 3 days, and 3
          times in all.
        </p>
      </div>
    </section>
  );
}

// yap-uiux-2026-10-16-MEETINGS — the Meetings view's needs-model state.
//
// A meeting is recorded by the capture path and transcribed by the same speech
// engine dictation uses. With no engine installed, recording still works but
// nothing turns the audio into a transcript. The walk's rule for that state:
// offer the ONE-CLICK install, never an error. So this is a single button that
// downloads (resumable, sha256-verified in Rust) and selects the catalog's
// pick — the same `autoPick` the silent first-run setup uses — and, while that
// runs, the app-wide ModelRibbon's live percentage (never a spinner).
import { autoPick, fmtSize, type ModelSetup } from "../ModelSetup";

export default function SpeechEngineNotice({ setup }: { setup: ModelSetup }) {
  // Until the catalog has been read, "no model" is not a fact yet.
  if (setup.ready || setup.models.length === 0) return null;
  const pick = autoPick(setup.models);
  if (!pick) return null;
  const install = () => void setup.download(pick.id);

  // While a download is running the app-wide ModelRibbon above every view
  // already carries its live percentage; a second bar here would be the same
  // fact twice.
  if (setup.downloading) return null;

  return (
    <div className="meetings-engine" data-needs-model={setup.error ? "retry" : "install"}>
      <div className="meetings-engine-copy">
        <strong>
          {setup.error
            ? "The speech engine didn’t finish installing."
            : "Transcripts need the speech engine."}
        </strong>
        <span className="tiny">
          {pick.downloaded
            ? "It is already on this Mac — one click turns it on."
            : `One download, ${fmtSize(pick.sizeBytes)}, runs on this Mac. Recording works meanwhile.`}
        </span>
      </div>
      <button type="button" className="primary" onClick={install}>
        {setup.error ? "Try again" : pick.downloaded ? "Turn on speech engine" : "Install speech engine"}
      </button>
    </div>
  );
}

import React, { useEffect, useState } from "react";
import ReactDOM from "react-dom/client";
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import "./float.css";

interface AppStatus {
  recording: boolean;
  busy: boolean;
  message: string;
}

function FloatPill() {
  const [status, setStatus] = useState<AppStatus>({
    recording: false,
    busy: false,
    message: "Ready",
  });

  useEffect(() => {
    invoke<AppStatus>("get_status")
      .then((s) =>
        setStatus({
          recording: s.recording,
          busy: s.busy,
          message: s.message,
        }),
      )
      .catch(() => {});
    const unsubs: Array<() => void> = [];
    listen<AppStatus>("status", (e) => {
      setStatus({
        recording: e.payload.recording,
        busy: e.payload.busy,
        message: e.payload.message,
      });
    }).then((u) => unsubs.push(u));
    listen<boolean>("recording", (e) => {
      setStatus((s) => ({
        ...s,
        recording: e.payload,
        message: e.payload ? "Listening…" : s.message,
      }));
    }).then((u) => unsubs.push(u));
    return () => unsubs.forEach((u) => u());
  }, []);

  const live = status.recording;
  const busy = status.busy;

  return (
    <div
      className={live ? "pill live" : busy ? "pill busy" : "pill"}
      data-tauri-drag-region
    >
      <button
        type="button"
        className="dictate"
        disabled={busy}
        onClick={() => invoke("manual_toggle")}
      >
        <span className="dot" />
        <span>{live ? "Stop" : busy ? "…" : "Dictate"}</span>
        <kbd>⌘⇧V</kbd>
      </button>
      <button
        type="button"
        className="open"
        title="Open Wilson Voice"
        onClick={() => invoke("show_main")}
      >
        ↗
      </button>
    </div>
  );
}

ReactDOM.createRoot(document.getElementById("root")!).render(
  <React.StrictMode>
    <FloatPill />
  </React.StrictMode>,
);

import React from "react";
import ReactDOM from "react-dom/client";
import App from "./App";
import { checkForUpdates } from "./updater";

ReactDOM.createRoot(document.getElementById("root") as HTMLElement).render(
  <React.StrictMode>
    <App />
  </React.StrictMode>,
);

// Fire-and-forget auto-update check (guarded; never blocks the UI).
void checkForUpdates();

import React from "react";
import ReactDOM from "react-dom/client";
import "./tokens.css";
import App from "./App";
// Y5-J — the one global focus ring. Vite hoists CSS shared by both entries
// AHEAD of each entry's own sheet, so this cannot win on source order: no rule
// anywhere may zero `outline` instead (src/a11y/semantics.test.ts).
import "./a11y/focus.css";

ReactDOM.createRoot(document.getElementById("root") as HTMLElement).render(
  <React.StrictMode>
    <App />
  </React.StrictMode>,
);

// YV44: no update work happens here any more. The check is owned by App (it is
// gated on the `checkUpdates` setting and only ever raises a prompt), and the
// install runs on the user's click — never silently at launch.

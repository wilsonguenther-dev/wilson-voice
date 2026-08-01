import React from "react";
import ReactDOM from "react-dom/client";
import App from "./App";

ReactDOM.createRoot(document.getElementById("root") as HTMLElement).render(
  <React.StrictMode>
    <App />
  </React.StrictMode>,
);

// YV44: no update work happens here any more. The check is owned by App (it is
// gated on the `checkUpdates` setting and only ever raises a prompt), and the
// install runs on the user's click — never silently at launch.

/**
 * DB-D — the Scratchpad window entry (scratchpad.html). Its own React tree,
 * like the pill: the panel opens over another app without bringing Yap's main
 * window (or its state) along.
 */
import React from "react";
import ReactDOM from "react-dom/client";
import "./tokens.css";
import "./scratchpad/scratchpad.css";
import ScratchpadWindow from "./scratchpad/ScratchpadWindow";

ReactDOM.createRoot(document.getElementById("root") as HTMLElement).render(
  <React.StrictMode>
    <ScratchpadWindow />
  </React.StrictMode>,
);

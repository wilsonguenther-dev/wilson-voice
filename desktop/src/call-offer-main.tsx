/**
 * yap-call-1 — the call-offer window's entry (call-offer.html). Its own React
 * tree and its own NSWindow, so Rust can content-protect THIS window (hidden
 * from screen capture / share) while the pill and its recording dot stay
 * visible in a share.
 */
import React from "react";
import ReactDOM from "react-dom/client";
import CallOffer from "./pill/CallOffer";
import "./pill/call-offer.css";

ReactDOM.createRoot(document.getElementById("root")!).render(
  <React.StrictMode>
    <CallOffer />
  </React.StrictMode>,
);

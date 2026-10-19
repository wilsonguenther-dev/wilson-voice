/// <reference types="vite/client" />

/** YP5 — injected by vite from package.json's `version` (see vite.config.ts). */
declare const __APP_VERSION__: string;
/** TC-1: build channel + head, folded in by vite.config.ts. */
declare const __YAP_BUILD__: { channel: string; sha: string; branch: string };

/**
 * WF-11 — the window's ONE language store, bound to the backend. The bar's
 * chip (src/pill/LanguageChip.tsx) and the Settings control
 * (src/LanguageSettings.tsx) both read it, and the tray's Languages submenu
 * writes through the same backend writer, whose LANGUAGES_CHANGED_EVENT this
 * store applies at once — so a pick on any surface shows on every other one
 * without waiting for a model probe. The logic lives (and is tested) in
 * languagePicker.ts `createLanguageStore`; this file is only the Tauri glue.
 */
import { useEffect, useSyncExternalStore } from "react";
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import {
  createLanguageStore,
  type LanguageOptions,
  type LanguageStore,
  type LanguageStoreState,
} from "./languagePicker";

export const languageStore: LanguageStore = createLanguageStore({
  options: () => invoke<LanguageOptions>("language_options"),
  write: (prefs) => invoke<unknown>("set_dictation_languages", { ...prefs }),
  listen: (event, cb) => listen(event, (e) => cb(e.payload)),
});

/** Subscribe a component to the store (and keep its listeners alive). */
export function useLanguageStore(): LanguageStoreState {
  useEffect(() => languageStore.start(), []);
  return useSyncExternalStore(languageStore.subscribe, languageStore.get, languageStore.get);
}

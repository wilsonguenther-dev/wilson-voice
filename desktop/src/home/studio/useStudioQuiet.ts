/**
 * yap-1106-sal-and-mochi — the quiet state for Home's studio: Mochi hangs on
 * the door during a Focus / Do Not Disturb or a screen share. It is the SAME
 * state the pill's friend visits obey (pill_quiet.rs): the `pill_quiet` event
 * (emitted to every window on a change) plus one `pill_quiet_state` read on
 * mount, so a Home opened after the last change still shows the right door.
 */
import { useEffect, useState } from "react";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";
import { invoke } from "@tauri-apps/api/core";
import { PILL_QUIET_EVENT, PILL_QUIET_STATE, parseQuietState } from "../../pill/friendVisits";

export interface StudioQuiet {
  screenSharing: boolean;
  doNotDisturb: boolean;
}

export const NOT_QUIET: StudioQuiet = { screenSharing: false, doNotDisturb: false };

export function useStudioQuiet(): StudioQuiet {
  const [quiet, setQuiet] = useState<StudioQuiet>(NOT_QUIET);
  useEffect(() => {
    let dead = false;
    let unlisten: UnlistenFn | null = null;
    listen<unknown>(PILL_QUIET_EVENT, (e) => {
      const q = parseQuietState(e.payload);
      console.info(`home studio_quiet screen_sharing=${q.screenSharing} dnd=${q.doNotDisturb}`);
      if (!dead) setQuiet(q);
    }).then(
      (u) => {
        if (dead) u();
        else unlisten = u;
      },
      (e: unknown) => console.warn(`home studio_quiet listen_failed reason=${e instanceof Error ? e.name : "unknown"}`),
    );
    invoke<unknown>(PILL_QUIET_STATE)
      .then((v) => {
        if (!dead) setQuiet(parseQuietState(v));
      })
      .catch((e: unknown) => console.warn(`home studio_quiet state_failed reason=${e instanceof Error ? e.name : "unknown"}`));
    return () => {
      dead = true;
      unlisten?.();
    };
  }, []);
  return quiet;
}

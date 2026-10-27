import { useCallback, useEffect, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import {
  builtInAlternative,
  moveInRanking,
  rankedOrder,
  type MicDevicesView,
  type MicNotice,
} from "./micRanking";

/**
 * PERM-H — the ranked microphone list. The highest-ranked ATTACHED mic is
 * what the next take records from; drag (or the arrows) to reorder, "Forget"
 * so an old headset stops winning. AirPods and a closed lid are called out,
 * once per selection, with the built-in mic offered instead.
 *
 * No Jabra-style wear detection: vendor-specific, out of scope.
 */
export default function MicrophoneRanking() {
  const [view, setView] = useState<MicDevicesView | null>(null);
  const [notices, setNotices] = useState<MicNotice[]>([]);
  const [error, setError] = useState<string | null>(null);
  const [dragging, setDragging] = useState<string | null>(null);

  const refresh = useCallback(() => {
    invoke<MicDevicesView>("mic_devices")
      .then((v) => {
        setView(v);
        setError(null);
      })
      .catch((e) => setError(String(e)));
  }, []);

  useEffect(() => {
    refresh();
    const onFocus = () => refresh();
    window.addEventListener("focus", onFocus);
    return () => window.removeEventListener("focus", onFocus);
  }, [refresh]);

  const select = async (name: string) => {
    try {
      setNotices(await invoke<MicNotice[]>("mic_rank_select", { name }));
    } catch (e) {
      setError(String(e));
    }
    refresh();
  };
  const reorder = async (order: string[]) => {
    try {
      await invoke("mic_rank_reorder", { order });
    } catch (e) {
      setError(String(e));
    }
    refresh();
  };
  const forget = async (name: string) => {
    try {
      await invoke<boolean>("mic_rank_forget", { name });
      setNotices((n) => n.filter((x) => x.device !== name));
    } catch (e) {
      setError(String(e));
    }
    refresh();
  };

  if (!view) {
    return error ? <p className="settings-hint">Couldn't read your microphones: {error}</p> : null;
  }
  const order = rankedOrder(view.rows);
  const builtIn = builtInAlternative(view.rows);
  const winner = view.rows.find((r) => r.wins);

  return (
    <div className="panel" data-row data-mic-ranking>
      <h3>Microphones</h3>
      <p className="settings-hint">
        Yap records from the highest mic on this list that's plugged in. Drag to reorder.
        {winner ? (
          <>
            {" "}Next recording uses <strong>{winner.name}</strong>.
          </>
        ) : null}
      </p>
      {view.lidClosed && winner?.class === "built_in" ? (
        <p className="notice" role="status">
          Your Mac's lid is closed, so its built-in mic is muffled. Open the lid or plug in a mic.
        </p>
      ) : null}
      {notices.map((n) => (
        <p className="notice" role="status" key={`${n.kind}-${n.device}`}>
          <strong>{n.title}.</strong> {n.body}
          {n.kind === "airpods" && builtIn ? (
            <>
              {" "}
              <button className="ghost" onClick={() => select(builtIn.name)}>
                Use {builtIn.name}
              </button>
            </>
          ) : null}
        </p>
      ))}
      <ol className="mic-ranking" aria-label="Microphone preference order">
        {view.rows.map((row) => {
          const ranked = row.rank !== null;
          const at = ranked ? (row.rank as number) : order.length;
          return (
            <li
              key={row.name}
              className="profile-row"
              draggable
              aria-current={row.wins ? "true" : undefined}
              onDragStart={() => setDragging(row.name)}
              onDragEnd={() => setDragging(null)}
              onDragOver={(e) => e.preventDefault()}
              onDrop={(e) => {
                e.preventDefault();
                if (dragging && dragging !== row.name) {
                  reorder(moveInRanking(order, dragging, at));
                }
                setDragging(null);
              }}
            >
              <span>
                {ranked ? `${(row.rank as number) + 1}. ` : ""}
                <strong>{row.name}</strong>
                {row.wins ? " — in use" : ""}
                {!row.present ? <span className="tiny muted"> (not connected)</span> : null}
                {row.class === "bluetooth" ? <span className="tiny muted"> · Bluetooth, lower quality</span> : null}
              </span>
              <span className="snip-actions">
                {ranked ? (
                  <>
                    <button
                      className="ghost"
                      aria-label={`Move ${row.name} up`}
                      disabled={row.rank === 0}
                      onClick={() => reorder(moveInRanking(order, row.name, (row.rank as number) - 1))}
                    >
                      ↑
                    </button>
                    <button
                      className="ghost"
                      aria-label={`Move ${row.name} down`}
                      disabled={row.rank === order.length - 1}
                      onClick={() => reorder(moveInRanking(order, row.name, (row.rank as number) + 1))}
                    >
                      ↓
                    </button>
                    <button className="ghost" onClick={() => forget(row.name)}>
                      Forget
                    </button>
                  </>
                ) : (
                  <button className="ghost" onClick={() => select(row.name)}>
                    Use this mic
                  </button>
                )}
              </span>
            </li>
          );
        })}
      </ol>
      {view.rows.length === 0 ? <p className="settings-hint">No microphone is connected.</p> : null}
      {error ? <p className="settings-hint" role="alert">{error}</p> : null}
    </div>
  );
}

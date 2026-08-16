/**
 * YV129 — dev tooling: the "who is this?" chip row, on demand.
 *
 * Every state worth reviewing needs something you cannot produce at a desk: a
 * six-person far-field recording that has been clustered, a roster of enrolled
 * voices, and a match that landed between the bands. This mounts the REAL
 * component inside the app's own chrome against four of them, which is why a
 * screenshot of this page is a screenshot of the shipping UI.
 *
 * Same rules as `meetings/preview.tsx`: it is a BUILD ENTRY behind
 * `YAP_DEV_TOOLING=1`, so no shipped build carries it.
 *
 *   YAP_DEV_TOOLING=1 npm run build   # then dist/dev/speaker-chips-preview.html
 */
import React, { useState } from "react";
import ReactDOM from "react-dom/client";
import SpeakerChipRow from "./SpeakerChipRow";
import {
  chipRow,
  type ChipFloor,
  type ClusterDecisionView,
  type SpeakerProfileView,
} from "./speakerChips";
import "../App.css";

/** YV126's ranking floor. A parameter here exactly as it is in the crate. */
const FLOOR: ChipFloor = { minSpeechSeconds: 30, minTurns: 3 };

const ROSTER: SpeakerProfileView[] = [
  { id: "p_wilson", displayName: "Wilson", isMe: true },
  { id: "p_jeisil", displayName: "Jeisil", isMe: false },
  { id: "p_aidan", displayName: "Aidan", isMe: false },
];

function cluster(
  clusterIndex: number,
  speechSeconds: number,
  turns: number,
): ClusterDecisionView["cluster"] {
  return {
    clusterIndex,
    label: `Speaker ${clusterIndex + 1}`,
    speechSeconds,
    turns,
  };
}

/** The six-person classroom: four questions, two quiet voices as Other. */
const CLASSROOM: ClusterDecisionView[] = [
  { cluster: cluster(0, 612, 24), result: { kind: "new" } },
  { cluster: cluster(1, 210, 18), result: { kind: "new" } },
  { cluster: cluster(2, 95, 11), result: { kind: "new" } },
  { cluster: cluster(3, 33, 4), result: { kind: "new" } },
  { cluster: cluster(4, 12, 3), result: { kind: "new" } },
  { cluster: cluster(5, 44, 2), result: { kind: "new" } },
];

/** One voice near a known profile: the suggestion arrives pre-selected. */
const SUGGESTED: ClusterDecisionView[] = [
  {
    cluster: cluster(1, 210, 9),
    result: {
      kind: "suggested",
      profileId: "p_jeisil",
      score: 0.71,
      conditionKey: "laptop_mic_near",
    },
  },
];

/** Everybody auto-confirmed: the row draws nothing rather than an empty panel. */
const ALL_KNOWN: ClusterDecisionView[] = [
  {
    cluster: cluster(0, 400, 20),
    result: {
      kind: "known",
      profileId: "p_wilson",
      score: 0.94,
      conditionKey: "laptop_mic_near",
    },
  },
];

const SCENES = ["classroom", "suggested", "all-known", "only-quiet"] as const;
type Scene = (typeof SCENES)[number];

function sceneFromHash(): Scene {
  const h = window.location.hash.replace("#", "") as Scene;
  return SCENES.includes(h) ? h : "classroom";
}

function decisionsFor(scene: Scene): ClusterDecisionView[] {
  switch (scene) {
    case "suggested":
      return SUGGESTED;
    case "all-known":
      return ALL_KNOWN;
    case "only-quiet":
      return [{ cluster: cluster(7, 6, 1), result: { kind: "new" } }];
    default:
      return CLASSROOM;
  }
}

function Preview() {
  const [scene, setScene] = useState<Scene>(sceneFromHash());
  const [picked, setPicked] = useState<string>("nothing picked yet");
  const row = chipRow(true, decisionsFor(scene), [], ROSTER, FLOOR);

  return (
    <div className="app">
      <div className="meeting-detail">
        <div className="actions">
          {SCENES.map((s) => (
            <button
              key={s}
              className={s === scene ? "primary" : "ghost"}
              onClick={() => {
                window.location.hash = s;
                setScene(s);
              }}
            >
              {s}
            </button>
          ))}
        </div>
        <SpeakerChipRow
          row={row}
          onPick={(clusterIndex, profileId) =>
            setPicked(`cluster ${clusterIndex} → ${profileId}`)
          }
          onNew={(clusterIndex) => setPicked(`cluster ${clusterIndex} → + new`)}
        />
        <p className="card-meta">{picked}</p>
      </div>
    </div>
  );
}

ReactDOM.createRoot(document.getElementById("root") as HTMLElement).render(
  <React.StrictMode>
    <Preview />
  </React.StrictMode>,
);

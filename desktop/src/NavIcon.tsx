/**
 * yap-1106-settings-redesign — the sidebar rail's icons. One 18px stroke icon
 * per screen, drawn in currentColor so the active / muted nav tones apply.
 * Under 900px the rail shows only these; the label stays for screen readers.
 */
import type { Nav } from "./appTypes";

const PATHS: Partial<Record<Nav, string>> = {
  home: "M3.5 10.5 12 3.5l8.5 7V20a1 1 0 0 1-1 1H15v-6H9v6H4.5a1 1 0 0 1-1-1z",
  // yap-1106-home-full-width-room added the History screen; a clock face.
  history: "M12 3.5a8.5 8.5 0 1 0 0 17 8.5 8.5 0 0 0 0-17zM12 7.5V12l3 2",
  permissions: "M12 3.5 5 6.5v5.2c0 4.2 3 7.8 7 8.8 4-1 7-4.6 7-8.8V6.5zM9 12l2 2 4-4",
  meetings: "M4.5 6h15v13.5h-15zM4.5 10h15M8.5 3.5v4M15.5 3.5v4",
  insights: "M5 20v-8M12 20V5M19 20v-5M3.5 20.5h17",
  dictionary: "M6 4h9.5A3.5 3.5 0 0 1 19 7.5V20H9.5A3.5 3.5 0 0 1 6 16.5zM6 16.5A3.5 3.5 0 0 1 9.5 13H19",
  scratchpad: "M6 3.5h9l3.5 3.5v13.5H6zM14.5 3.5V7.5h4M9 12.5h6.5M9 16h4.5",
  settings: "M4 7h9M17 7h3M4 17h3M11 17h9M15 5v4M9 15v4",
};

export default function NavIcon({ id }: { id: Nav }) {
  const d = PATHS[id] ?? "M5 5h14v14H5z";
  return (
    <svg
      className="nav-icon"
      viewBox="0 0 24 24"
      fill="none"
      stroke="currentColor"
      strokeWidth={1.6}
      strokeLinecap="round"
      strokeLinejoin="round"
      aria-hidden="true"
      focusable="false"
    >
      <path d={d} />
    </svg>
  );
}

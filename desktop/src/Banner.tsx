/**
 * Banner — the main window's full-width notice strip (Y5-J).
 *
 * The one way to draw `.banner`. It exists so a clickable banner cannot be a
 * `<div onClick>` again: give it `onActivate` and it renders a real
 * `<button>` (focusable, Enter/Space for free, announced as a button by
 * VoiceOver); leave it off and it renders an inert `<div>` with the live-region
 * role the caller asks for. Before this, App.tsx's "Setup incomplete" banner
 * was a mouse-only div — the one path from Home to Permissions that a keyboard
 * could not take.
 */
import type { ReactNode } from "react";

export type BannerTone = "warn" | "blocked" | "update";

interface BannerProps {
  tone: BannerTone;
  /** Makes the whole strip a button that does this. */
  onActivate?: () => void;
  /** For an inert banner: how a screen reader should treat it. */
  role?: "status" | "alert";
  children: ReactNode;
}

export function bannerClass(tone: BannerTone): string {
  return `banner ${tone}`;
}

export default function Banner({ tone, onActivate, role, children }: BannerProps) {
  if (onActivate) {
    return (
      <button type="button" className={bannerClass(tone)} onClick={onActivate}>
        {children}
      </button>
    );
  }
  return (
    <div className={bannerClass(tone)} role={role}>
      {children}
    </div>
  );
}

// YV43 + yap-1107-secure-input-own-field-quiet — the Secure Input surface of
// the main window, split out of App.tsx so its two states render in a test.
//
// * Another app (or an unknown holder) holds Secure Input: the CGEvent tap
//   behind fn / fn⌃ is blind, so the loud red banner names the holder and the
//   paths that still work — exactly as before.
// * Yap's OWN masked key field holds it (WebKit turns Secure Input on while a
//   password input has focus): the user did this by focusing the field, so it
//   is one quiet neutral line — never red, no detail paragraph — and it goes
//   away on the release edge the moment the field blurs.
import type { AppStatus } from "./appTypes";

/** Kept identical to `secure_input::OWN_FIELD_MESSAGE` (Rust). */
export const OWN_FIELD_MESSAGE = "Shortcut paused while the key field is focused";

type Props = {
  status: Pick<AppStatus, "secureInputBlocked" | "secureInputDetail" | "secureInputOwnField">;
};

export default function SecureInputBanner({ status }: Props) {
  if (status.secureInputBlocked) {
    return (
      <div className="banner blocked">
        <strong>
          Dictation paused — another app is blocking keyboard monitoring
          (Secure Input)
        </strong>
        <span className="banner-detail">{status.secureInputDetail}</span>
      </div>
    );
  }
  if (status.secureInputOwnField) {
    return (
      <p className="secure-input-quiet" role="status">
        {OWN_FIELD_MESSAGE}
      </p>
    );
  }
  return null;
}

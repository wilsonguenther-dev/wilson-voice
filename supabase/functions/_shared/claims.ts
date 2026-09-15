/**
 * LIC-A — the license wire format, and the only place it is written down.
 *
 * THE FORMAT IS FROZEN. Every shipped copy of Yap pins the issuer's public key
 * and verifies offline, in `desktop/src-tauri/src/license.rs`:
 *
 *     base64url(claimsJson) "." base64url(ed25519 signature)
 *
 * and the signature covers the **ASCII bytes of the first segment** — not the
 * JSON, not the decoded object, not a re-serialisation. Re-encoding the claims
 * on the verifying side would produce a different byte string for the same
 * object (key order, spacing) and every key in the field would stop verifying,
 * so the signer signs the segment it actually emits and nothing else.
 *
 * This module is deliberately Web Crypto only: no Deno APIs, no npm imports, no
 * Node builtins. That is what lets the Edge Function and the vitest suite run
 * the *same* code, so the tests are evidence about the deployed issuer rather
 * than about a parallel implementation that drifts.
 */

/** The claims Yap's verifier understands. Adding a field is safe (the Rust side
 *  ignores unknown fields); changing one of these is a fork of the format. */
export interface Claims {
  /** Claims version. `1` is what every shipped verifier accepts. */
  v: number;
  /** The only plan Yap sells. Entitlement requires it verbatim. */
  plan: string;
  seats: number;
  /** sha256(lowercased, trimmed purchase email), hex. No address is signed. */
  email_hash: string;
  /** ISO-8601, second precision. */
  issued_at: string;
  /** License id. The unit of revocation: one purchase, one kid. */
  kid: string;
  /** Signing-key id: sha256(SPKI DER)[..8] hex. Pins WHICH key signed. */
  skid: string;
}

export const CLAIMS_VERSION = 1;
export const SOLD_PLAN = "lifetime";

const encoder = new TextEncoder();

export function b64urlEncode(bytes: Uint8Array): string {
  let binary = "";
  for (const b of bytes) binary += String.fromCharCode(b);
  return btoa(binary).replace(/\+/g, "-").replace(/\//g, "_").replace(/=+$/, "");
}

export function b64urlDecode(segment: string): Uint8Array {
  const normalized = segment.replace(/-/g, "+").replace(/_/g, "/");
  const padded = normalized + "=".repeat((4 - (normalized.length % 4)) % 4);
  const binary = atob(padded);
  const out = new Uint8Array(binary.length);
  for (let i = 0; i < binary.length; i++) out[i] = binary.charCodeAt(i);
  return out;
}

export function hex(bytes: Uint8Array): string {
  return Array.from(bytes)
    .map((b) => b.toString(16).padStart(2, "0"))
    .join("");
}

export async function sha256Hex(input: string): Promise<string> {
  const digest = await crypto.subtle.digest("SHA-256", encoder.encode(input));
  return hex(new Uint8Array(digest));
}

/** The address is hashed before it is signed, and the hash is what travels in
 *  the key. A license key found on disk names no human. */
export function emailHash(email: string): Promise<string> {
  return sha256Hex(email.trim().toLowerCase());
}

/** Deterministic license id: the same Stripe session always yields the same
 *  kid, so a replayed webhook re-issues a byte-identical key instead of a
 *  second license the revocation list would have to chase. */
export async function kidForSession(sessionId: string): Promise<string> {
  return (await sha256Hex(`yap-license:${sessionId}`)).slice(0, 16);
}

function stripPem(pem: string): Uint8Array {
  const body = pem
    .replace(/-----BEGIN [^-]+-----/g, "")
    .replace(/-----END [^-]+-----/g, "")
    .replace(/\s+/g, "");
  return b64urlDecode(body.replace(/\+/g, "-").replace(/\//g, "_"));
}

/** Import the PKCS#8 signing key. The PEM is a secret and only ever arrives
 *  from the environment — never from this repository. */
export function importSigningKey(pkcs8Pem: string): Promise<CryptoKey> {
  return crypto.subtle.importKey(
    "pkcs8",
    stripPem(pkcs8Pem) as unknown as ArrayBuffer,
    { name: "Ed25519" },
    false,
    ["sign"],
  );
}

export function importVerifyingKeySpki(spkiDer: Uint8Array): Promise<CryptoKey> {
  return crypto.subtle.importKey(
    "spki",
    spkiDer as unknown as ArrayBuffer,
    { name: "Ed25519" },
    true,
    ["verify"],
  );
}

/** sha256(SPKI DER)[..8] hex — the same derivation `license.rs::skid_of_spki`
 *  performs on the pinned public key, which is how the app can refuse a blob
 *  signed by some other key that happens to be well-formed. */
export async function skidOfSpki(spkiDer: Uint8Array): Promise<string> {
  const digest = await crypto.subtle.digest("SHA-256", spkiDer as unknown as ArrayBuffer);
  return hex(new Uint8Array(digest)).slice(0, 16);
}

/**
 * Sign the claims into a license key.
 *
 * The one subtlety worth stating out loud: `segment` is built once and both
 * signed and emitted. There is no second serialisation anywhere in this
 * function, because two serialisations of one object is how a signer and a
 * verifier quietly disagree.
 */
export async function signLicenseKey(
  claims: Claims,
  signingKey: CryptoKey,
): Promise<string> {
  const segment = b64urlEncode(encoder.encode(JSON.stringify(claims)));
  const signature = await crypto.subtle.sign(
    { name: "Ed25519" },
    signingKey,
    encoder.encode(segment) as unknown as ArrayBuffer,
  );
  return `${segment}.${b64urlEncode(new Uint8Array(signature))}`;
}

/** Verify a key the way the app does — used by the tests, and by the resend
 *  path to refuse to mail a key that would not activate. */
export async function verifyLicenseKey(
  licenseKey: string,
  verifyingKey: CryptoKey,
): Promise<Claims | null> {
  const parts = licenseKey.trim().split(".");
  if (parts.length !== 2) return null;
  const [segment, sig] = parts;
  let ok = false;
  try {
    ok = await crypto.subtle.verify(
      { name: "Ed25519" },
      verifyingKey,
      b64urlDecode(sig) as unknown as ArrayBuffer,
      encoder.encode(segment) as unknown as ArrayBuffer,
    );
  } catch {
    return null;
  }
  if (!ok) return null;
  try {
    const claims = JSON.parse(new TextDecoder().decode(b64urlDecode(segment))) as Claims;
    return claims.v === CLAIMS_VERSION ? claims : null;
  } catch {
    return null;
  }
}

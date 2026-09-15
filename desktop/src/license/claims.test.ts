/**
 * LIC-A — the frozen wire format, tested from the SIGNING side.
 *
 * `license.rs` already proves the verifier. What had never been tested in this
 * repository is the half that makes the keys, because that half lived on
 * another machine in another repository. It lives here now, so it is tested
 * here: same module the Edge Function deploys, no mock of it.
 */
import { describe, expect, it } from "vitest";
import {
  CLAIMS_VERSION,
  SOLD_PLAN,
  b64urlDecode,
  b64urlEncode,
  emailHash,
  kidForSession,
  signLicenseKey,
  skidOfSpki,
  verifyLicenseKey,
} from "../../../supabase/functions/_shared/claims.ts";

async function keyPair() {
  const kp = (await crypto.subtle.generateKey({ name: "Ed25519" }, true, [
    "sign",
    "verify",
  ])) as CryptoKeyPair;
  const spki = new Uint8Array(await crypto.subtle.exportKey("spki", kp.publicKey));
  return { kp, spki };
}

async function claimsFor(spki: Uint8Array) {
  return {
    v: CLAIMS_VERSION,
    plan: SOLD_PLAN,
    seats: 1,
    email_hash: await emailHash("Buyer@Example.com"),
    issued_at: "2026-09-15T00:00:00Z",
    kid: await kidForSession("cs_test_1"),
    skid: await skidOfSpki(spki),
  };
}

describe("LIC-A · license claims", () => {
  it("the_signature_covers_the_ascii_bytes_of_the_claims_segment", async () => {
    const { kp, spki } = await keyPair();
    const claims = await claimsFor(spki);
    const key = await signLicenseKey(claims, kp.privateKey);
    const [segment, sig] = key.split(".");

    // The signature verifies against the SEGMENT's ASCII bytes — the exact
    // string that travels — and not against the JSON or a re-serialisation of
    // the decoded object. This is the property every shipped copy of Yap
    // depends on, and the one a "tidy the JSON" change would silently break.
    const verifyingKey = await crypto.subtle.importKey(
      "spki",
      spki as unknown as ArrayBuffer,
      { name: "Ed25519" },
      true,
      ["verify"],
    );
    await expect(
      crypto.subtle.verify(
        { name: "Ed25519" },
        verifyingKey,
        b64urlDecode(sig) as unknown as ArrayBuffer,
        new TextEncoder().encode(segment) as unknown as ArrayBuffer,
      ),
    ).resolves.toBe(true);

    // ...and NOT against the decoded JSON bytes, which is the plausible wrong
    // implementation. If this ever passes, the two sides have forked.
    await expect(
      crypto.subtle.verify(
        { name: "Ed25519" },
        verifyingKey,
        b64urlDecode(sig) as unknown as ArrayBuffer,
        b64urlDecode(segment) as unknown as ArrayBuffer,
      ),
    ).resolves.toBe(false);

    expect(await verifyLicenseKey(key, verifyingKey)).toEqual(claims);
  });

  it("a key signed by another key does not verify", async () => {
    const a = await keyPair();
    const b = await keyPair();
    const key = await signLicenseKey(await claimsFor(a.spki), a.kp.privateKey);
    expect(await verifyLicenseKey(key, b.kp.publicKey)).toBeNull();
  });

  it("the claims are exactly the seven fields the Rust verifier reads", async () => {
    const { spki } = await keyPair();
    expect(Object.keys(await claimsFor(spki)).sort()).toEqual([
      "email_hash",
      "issued_at",
      "kid",
      "plan",
      "seats",
      "skid",
      "v",
    ]);
  });

  it("no email address is ever signed into a key", async () => {
    const { kp, spki } = await keyPair();
    const key = await signLicenseKey(await claimsFor(spki), kp.privateKey);
    expect(new TextDecoder().decode(b64urlDecode(key.split(".")[0]))).not.toContain("@");
  });

  it("base64url round-trips without padding", () => {
    const bytes = new Uint8Array([0, 1, 250, 251, 252, 253, 254, 255]);
    const encoded = b64urlEncode(bytes);
    expect(encoded).not.toContain("=");
    expect(Array.from(b64urlDecode(encoded))).toEqual(Array.from(bytes));
  });
});

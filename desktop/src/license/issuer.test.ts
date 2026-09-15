/**
 * LIC-A — the fulfilment path, driven with a fixture Stripe event and a
 * throwaway key pair. No socket is opened: the store, the mailer and the clock
 * are injected, which is the whole reason the handler takes them.
 */
import { describe, expect, it } from "vitest";
import {
  handleIssuerRequest,
  type Issuance,
  type IssuerDeps,
  type IssuerStore,
} from "../../../supabase/functions/_shared/issuer.ts";
import { verifyLicenseKey } from "../../../supabase/functions/_shared/claims.ts";

const NOW = new Date("2026-09-15T12:00:00Z");

function memoryStore(seed: Issuance[] = [], revoked: string[] = []) {
  const rows = [...seed];
  const store: IssuerStore & { rows: Issuance[] } = {
    rows,
    findByEventId: async (id) => rows.find((r) => r.event_id === id) ?? null,
    findBySessionId: async (id) => rows.find((r) => r.session_id === id) ?? null,
    findByEmailHash: async (h) => rows.find((r) => r.email_hash === h) ?? null,
    insert: async (i) => {
      rows.push(i);
    },
    revokedKids: async () => revoked,
  };
  return store;
}

async function deps(overrides: Partial<IssuerDeps> = {}) {
  const kp = (await crypto.subtle.generateKey({ name: "Ed25519" }, true, [
    "sign",
    "verify",
  ])) as CryptoKeyPair;
  const spki = new Uint8Array(await crypto.subtle.exportKey("spki", kp.publicKey));
  const sent: string[] = [];
  const base: IssuerDeps = {
    store: memoryStore(),
    mailer: { send: async (to) => void sent.push(to) },
    signingKey: kp.privateKey,
    publicKeySpki: spki,
    webhookSecret: "whsec_test",
    now: () => NOW,
    ...overrides,
  };
  return { deps: base, sent, publicKey: kp.publicKey };
}

function eventBody(eventId: string, sessionId: string, email = "buyer@example.com") {
  return JSON.stringify({
    id: eventId,
    type: "checkout.session.completed",
    data: { object: { id: sessionId, customer_details: { email } } },
  });
}

async function signedRequest(body: string, secret: string, at = NOW) {
  const t = Math.floor(at.getTime() / 1000);
  const key = await crypto.subtle.importKey(
    "raw",
    new TextEncoder().encode(secret) as unknown as ArrayBuffer,
    { name: "HMAC", hash: "SHA-256" },
    false,
    ["sign"],
  );
  const mac = await crypto.subtle.sign(
    "HMAC",
    key,
    new TextEncoder().encode(`${t}.${body}`) as unknown as ArrayBuffer,
  );
  const v1 = Array.from(new Uint8Array(mac))
    .map((b) => b.toString(16).padStart(2, "0"))
    .join("");
  return new Request("https://issuer.test/functions/v1/yap-license/stripe-webhook", {
    method: "POST",
    headers: { "stripe-signature": `t=${t},v1=${v1}` },
    body,
  });
}

describe("LIC-A · the issuer", () => {
  it("the_webhook_is_idempotent_on_event_id_and_session_id", async () => {
    const { deps: d, sent } = await deps();
    const store = d.store as ReturnType<typeof memoryStore>;

    const first = await handleIssuerRequest(
      await signedRequest(eventBody("evt_1", "cs_1"), "whsec_test"),
      d,
    );
    expect(first.status).toBe(200);
    expect(await first.json()).toMatchObject({ reused: false, mailed: true });
    expect(store.rows).toHaveLength(1);

    // Same delivery again (Stripe retries anything it did not see a 2xx for).
    const replay = await handleIssuerRequest(
      await signedRequest(eventBody("evt_1", "cs_1"), "whsec_test"),
      d,
    );
    expect(await replay.json()).toMatchObject({ reused: true });

    // Same PURCHASE under a NEW event id — a replay from the Stripe dashboard.
    // The session id is the identity of the payment, so this must not issue a
    // second live license behind one charge.
    const newEventSameSession = await handleIssuerRequest(
      await signedRequest(eventBody("evt_2", "cs_1"), "whsec_test"),
      d,
    );
    expect(await newEventSameSession.json()).toMatchObject({ reused: true });

    expect(store.rows).toHaveLength(1);
    expect(sent).toEqual(["buyer@example.com"]);

    // A genuinely different purchase still issues.
    await handleIssuerRequest(await signedRequest(eventBody("evt_3", "cs_2"), "whsec_test"), d);
    expect(store.rows).toHaveLength(2);
  });

  it("issues a key the shipped verifier accepts", async () => {
    const { deps: d, publicKey } = await deps();
    await handleIssuerRequest(await signedRequest(eventBody("evt_1", "cs_1"), "whsec_test"), d);
    const row = (d.store as ReturnType<typeof memoryStore>).rows[0];
    const claims = await verifyLicenseKey(row.license_key, publicKey);
    expect(claims).toMatchObject({ v: 1, plan: "lifetime", seats: 1, kid: row.kid });
    expect(claims?.email_hash).not.toContain("@");
  });

  it("refuses an unsigned or wrongly signed webhook", async () => {
    const { deps: d } = await deps();
    const bad = await handleIssuerRequest(
      await signedRequest(eventBody("evt_1", "cs_1"), "whsec_wrong"),
      d,
    );
    expect(bad.status).toBe(400);
    const unsigned = await handleIssuerRequest(
      new Request("https://issuer.test/functions/v1/yap-license/stripe-webhook", {
        method: "POST",
        body: eventBody("evt_1", "cs_1"),
      }),
      d,
    );
    expect(unsigned.status).toBe(400);
    expect((d.store as ReturnType<typeof memoryStore>).rows).toHaveLength(0);
  });

  it("a mail failure never fails the webhook", async () => {
    // Deliberate, and ported verbatim from the Forge implementation: a non-2xx
    // makes Stripe retry an event whose license is already on record.
    const { deps: d } = await deps({
      mailer: {
        send: async () => {
          throw new Error("resend_503");
        },
      },
    });
    const res = await handleIssuerRequest(
      await signedRequest(eventBody("evt_1", "cs_1"), "whsec_test"),
      d,
    );
    expect(res.status).toBe(200);
    expect(await res.json()).toMatchObject({ mailed: false, reused: false });
    expect((d.store as ReturnType<typeof memoryStore>).rows).toHaveLength(1);
  });

  it("retrieval returns the key for the purchase email and one sentence otherwise", async () => {
    const { deps: d } = await deps();
    await handleIssuerRequest(await signedRequest(eventBody("evt_1", "cs_1"), "whsec_test"), d);

    const hit = await handleIssuerRequest(
      new Request("https://issuer.test/functions/v1/yap-license/license", {
        method: "POST",
        body: JSON.stringify({ email: "BUYER@example.com " }),
      }),
      d,
    );
    expect(hit.status).toBe(200);
    expect(((await hit.json()) as { key: string }).key).toContain(".");

    const miss = await handleIssuerRequest(
      new Request("https://issuer.test/functions/v1/yap-license/license", {
        method: "POST",
        body: JSON.stringify({ email: "nobody@example.com" }),
      }),
      d,
    );
    expect(miss.status).toBe(404);
    const body = (await miss.json()) as { message: string };
    expect(body.message).toMatch(/could not find/i);
    expect(body.message).toMatch(/reply to your receipt/i);
  });

  it("the revocation list is public, versioned and cached for five minutes", async () => {
    const { deps: d } = await deps({ store: memoryStore([], ["deadbeefdeadbeef"]) });
    const res = await handleIssuerRequest(
      new Request("https://issuer.test/functions/v1/yap-license/revoked.json"),
      d,
    );
    expect(res.headers.get("cache-control")).toBe("public, max-age=300");
    expect(await res.json()).toMatchObject({ version: 1, kids: ["deadbeefdeadbeef"] });
  });
});

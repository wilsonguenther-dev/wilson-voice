/**
 * LIC-A — the Yap license issuer, as a pure request handler.
 *
 * This is the whole fulfilment path Yap has always had, moved off the Forge box
 * (`drivia-forge server/src/routes/yap.ts`) and into a Supabase Edge Function
 * this repository owns. The logic is PORTED, not reinvented: same four routes,
 * same wire format, same idempotency keys, same deliberate decision that a mail
 * failure is not a webhook failure.
 *
 * Every dependency that touches the world — the clock, the database, the
 * mailer, the signing key — is injected, so the tests drive the real handler
 * with a fixture event and a throwaway key pair and never open a socket. That
 * is what keeps the gate offline while still being evidence about the deployed
 * code rather than about a mock of it.
 */
import {
  CLAIMS_VERSION,
  SOLD_PLAN,
  type Claims,
  emailHash,
  importVerifyingKeySpki,
  kidForSession,
  signLicenseKey,
  skidOfSpki,
  verifyLicenseKey,
} from "./claims.ts";

/** One issued license. `event_id` and `session_id` are BOTH idempotency keys. */
export interface Issuance {
  kid: string;
  email: string;
  email_hash: string;
  license_key: string;
  issued_at: string;
  event_id: string;
  session_id: string;
}

export interface IssuerStore {
  /** An existing issuance for either Stripe id, or null. */
  findByEventId(eventId: string): Promise<Issuance | null>;
  findBySessionId(sessionId: string): Promise<Issuance | null>;
  findByEmailHash(hash: string): Promise<Issuance | null>;
  insert(issuance: Issuance): Promise<void>;
  /** The public revocation list: license ids that were refunded or charged back. */
  revokedKids(): Promise<string[]>;
}

export interface Mailer {
  /** Resolves on delivery; rejects on any mail failure. The webhook route
   *  swallows the rejection on purpose — see `issueForSession`. */
  send(to: string, licenseKey: string): Promise<void>;
}

export interface IssuerDeps {
  store: IssuerStore;
  mailer: Mailer;
  /** Already-imported PKCS#8 Ed25519 private key. The PEM never reaches here. */
  signingKey: CryptoKey;
  /** SPKI DER of the public half, for `skid` and for self-checking a key. */
  publicKeySpki: Uint8Array;
  /** Stripe's endpoint signing secret (`whsec_…`). */
  webhookSecret: string;
  now: () => Date;
  /** Seconds of clock skew tolerated on a Stripe signature. */
  toleranceSeconds?: number;
  log?: (line: string) => void;
}

const JSON_HEADERS = { "content-type": "application/json" };

function json(body: unknown, status = 200, headers: Record<string, string> = {}) {
  return new Response(JSON.stringify(body), {
    status,
    headers: { ...JSON_HEADERS, ...headers },
  });
}

/**
 * Verify Stripe's `Stripe-Signature` header without the Stripe SDK.
 *
 * The scheme is documented and small: the header is `t=<unix>,v1=<hex>,…`, the
 * signed payload is `${t}.${rawBody}`, and the MAC is HMAC-SHA256 under the
 * endpoint secret. Doing it with Web Crypto keeps the function dependency-free
 * and — the part that matters here — testable without a network or an SDK.
 *
 * The comparison is constant-time and the timestamp is bounded, so a captured
 * webhook cannot be replayed at leisure.
 */
export async function verifyStripeSignature(
  rawBody: string,
  header: string | null,
  secret: string,
  now: Date,
  toleranceSeconds = 300,
): Promise<boolean> {
  if (!header) return false;
  const parts = new Map<string, string[]>();
  for (const pair of header.split(",")) {
    const idx = pair.indexOf("=");
    if (idx <= 0) continue;
    const k = pair.slice(0, idx).trim();
    const v = pair.slice(idx + 1).trim();
    parts.set(k, [...(parts.get(k) ?? []), v]);
  }
  const t = parts.get("t")?.[0];
  const signatures = parts.get("v1") ?? [];
  if (!t || signatures.length === 0) return false;
  const age = Math.abs(Math.floor(now.getTime() / 1000) - Number(t));
  if (!Number.isFinite(age) || age > toleranceSeconds) return false;

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
    new TextEncoder().encode(`${t}.${rawBody}`) as unknown as ArrayBuffer,
  );
  const expected = Array.from(new Uint8Array(mac))
    .map((b) => b.toString(16).padStart(2, "0"))
    .join("");
  return signatures.some((candidate) => constantTimeEqual(candidate, expected));
}

function constantTimeEqual(a: string, b: string): boolean {
  if (a.length !== b.length) return false;
  let diff = 0;
  for (let i = 0; i < a.length; i++) diff |= a.charCodeAt(i) ^ b.charCodeAt(i);
  return diff === 0;
}

/** The subset of a `checkout.session.completed` event this issuer reads. */
interface StripeEvent {
  id?: string;
  type?: string;
  data?: { object?: { id?: string; customer_details?: { email?: string | null }; customer_email?: string | null } };
}

/**
 * Issue (or re-use) the license for one paid checkout session.
 *
 * IDEMPOTENT ON BOTH IDS. Stripe retries a webhook it did not get a 2xx for,
 * and it will happily deliver the same event twice with different event ids in
 * a replay; the session id is the stable identity of the purchase, the event id
 * is the stable identity of the delivery, and either one already on record
 * means the customer has their key. Issuing twice would put two live licenses
 * behind one payment, and only one of them would ever be revocable by support.
 */
export async function issueForSession(
  deps: IssuerDeps,
  eventId: string,
  sessionId: string,
  email: string,
): Promise<{ issuance: Issuance; reused: boolean; mailed: boolean }> {
  const existing =
    (await deps.store.findByEventId(eventId)) ??
    (await deps.store.findBySessionId(sessionId));
  if (existing) return { issuance: existing, reused: true, mailed: false };

  const issued_at = deps.now().toISOString().replace(/\.\d{3}Z$/, "Z");
  const kid = await kidForSession(sessionId);
  const claims: Claims = {
    v: CLAIMS_VERSION,
    plan: SOLD_PLAN,
    seats: 1,
    email_hash: await emailHash(email),
    issued_at,
    kid,
    skid: await skidOfSpki(deps.publicKeySpki),
  };
  const license_key = await signLicenseKey(claims, deps.signingKey);
  const issuance: Issuance = {
    kid,
    email,
    email_hash: claims.email_hash,
    license_key,
    issued_at,
    event_id: eventId,
    session_id: sessionId,
  };
  await deps.store.insert(issuance);

  // A MAIL FAILURE IS NOT A WEBHOOK FAILURE. Resend being down must never turn
  // into a non-2xx, because Stripe would then retry an event whose license is
  // already issued and recorded. The customer's key exists and the retrieval
  // route below hands it to them in the app; losing the email is recoverable,
  // losing the issuance record is not.
  let mailed = false;
  try {
    await deps.mailer.send(email, license_key);
    mailed = true;
  } catch (e) {
    deps.log?.(`yap-license: mail failed for ${kid}: ${String(e)}`);
  }
  return { issuance, reused: false, mailed };
}

/**
 * The whole issuer: four routes, matched on the LAST path segment so the
 * function works at `/functions/v1/yap-license/...` and at a custom domain
 * without the routing depending on where it is mounted.
 */
export async function handleIssuerRequest(req: Request, deps: IssuerDeps): Promise<Response> {
  const url = new URL(req.url);
  const route = url.pathname.replace(/\/+$/, "").split("/").pop() ?? "";

  if (route === "revoked.json") {
    // PUBLIC and cacheable. This is the only URL a running Yap ever fetches.
    const kids = await deps.store.revokedKids();
    return json(
      { version: 1, updatedAt: deps.now().toISOString(), kids },
      200,
      { "cache-control": "public, max-age=300" },
    );
  }

  if (route === "stripe-webhook") {
    if (req.method !== "POST") return json({ error: "method_not_allowed" }, 405);
    const raw = await req.text();
    const ok = await verifyStripeSignature(
      raw,
      req.headers.get("stripe-signature"),
      deps.webhookSecret,
      deps.now(),
      deps.toleranceSeconds ?? 300,
    );
    if (!ok) return json({ error: "bad_signature" }, 400);

    let event: StripeEvent;
    try {
      event = JSON.parse(raw) as StripeEvent;
    } catch {
      return json({ error: "malformed_event" }, 400);
    }
    if (event.type !== "checkout.session.completed") {
      return json({ ignored: event.type ?? "unknown" }, 200);
    }
    const session = event.data?.object;
    const email = session?.customer_details?.email ?? session?.customer_email ?? "";
    if (!event.id || !session?.id || !email) {
      return json({ error: "incomplete_event" }, 400);
    }
    const { issuance, reused, mailed } = await issueForSession(deps, event.id, session.id, email);
    return json({ kid: issuance.kid, reused, mailed }, 200);
  }

  if (route === "license" || route === "resend") {
    // RETRIEVAL. `license` answers with the key (the app activates it in
    // place); `resend` mails it again and answers with nothing. Both take the
    // purchase email, and both answer the SAME shape whether or not a purchase
    // exists, so this endpoint cannot be walked as an oracle for who bought Yap.
    if (req.method !== "POST") return json({ error: "method_not_allowed" }, 405);
    let email = "";
    try {
      email = String(((await req.json()) as { email?: unknown }).email ?? "");
    } catch {
      return json({ error: "malformed_request" }, 400);
    }
    if (!email.includes("@")) return json({ error: "malformed_request" }, 400);

    const found = await deps.store.findByEmailHash(await emailHash(email));
    if (!found) {
      // 404 with one sentence, not a stack trace and not a lie.
      return json(
        {
          error: "not_found",
          message:
            "We could not find a Yap license for that email address. Use the address you paid with, or reply to your receipt and we will sort it out.",
        },
        404,
      );
    }
    if (route === "resend") {
      try {
        await deps.mailer.send(found.email, found.license_key);
      } catch (e) {
        deps.log?.(`yap-license: resend failed for ${found.kid}: ${String(e)}`);
        return json({ error: "mail_failed", message: "We could not send that email just now. Try again in a minute." }, 502);
      }
      return json({ sent: true }, 200);
    }
    return json({ key: found.license_key, kid: found.kid }, 200);
  }

  return json({ error: "not_found" }, 404);
}

/** Re-exported so a caller (and the tests) can check a key end to end without
 *  importing two modules. */
export { importVerifyingKeySpki, verifyLicenseKey };

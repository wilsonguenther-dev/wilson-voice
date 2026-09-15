/**
 * LIC-A — the Yap license issuer, deployed.
 *
 *     supabase functions deploy yap-license --no-verify-jwt --project-ref <ref>
 *
 * This file is the ONLY Deno-specific part of the issuer: it reads the
 * owner-provisioned secrets out of the environment, builds the three real
 * dependencies (Postgres, Resend, the signing key) and hands them to
 * `handleIssuerRequest`, which holds all of the logic and all of the tests.
 * Nothing here is worth a unit test, and nothing testable is here.
 *
 * `--no-verify-jwt` is deliberate and not an oversight: Stripe cannot present a
 * Supabase JWT, and the revocation list is public by design (every copy of Yap
 * fetches it anonymously). The webhook is authenticated by its Stripe
 * signature, which is stronger than a shared bearer token because it covers the
 * body.
 *
 * RUNTIME DEPENDENCIES — all owner-provisioned, none committed. See
 * docs/YAP-LICENSING.md for the table and what breaks when one is missing:
 *   YAP_SIGNING_KEY_PEM       Ed25519 PKCS#8 private key (the SAME key the
 *                             shipped app pins the public half of)
 *   YAP_PUBLIC_KEY_SPKI_B64   public half, base64 DER — used for `skid`
 *   STRIPE_WEBHOOK_SECRET     `whsec_…` from the Stripe endpoint
 *   RESEND_API_KEY            delivery
 *   SUPABASE_URL / SUPABASE_SERVICE_ROLE_KEY  injected by the platform
 */
import { createClient } from "jsr:@supabase/supabase-js@2";
import { b64urlDecode, importSigningKey } from "../_shared/claims.ts";
import {
  handleIssuerRequest,
  type Issuance,
  type IssuerStore,
  type Mailer,
} from "../_shared/issuer.ts";

declare const Deno: { env: { get(k: string): string | undefined }; serve(h: (r: Request) => Promise<Response>): void };

function required(name: string): string {
  const value = Deno.env.get(name);
  if (!value) {
    // Fail loudly at cold start rather than issuing an unsigned nothing.
    throw new Error(`yap-license: missing secret ${name}`);
  }
  return value;
}

const db = createClient(required("SUPABASE_URL"), required("SUPABASE_SERVICE_ROLE_KEY"), {
  auth: { persistSession: false },
});

const store: IssuerStore = {
  async findByEventId(eventId) {
    const { data } = await db.from("yap_licenses").select("*").eq("event_id", eventId).maybeSingle();
    return (data as Issuance | null) ?? null;
  },
  async findBySessionId(sessionId) {
    const { data } = await db.from("yap_licenses").select("*").eq("session_id", sessionId).maybeSingle();
    return (data as Issuance | null) ?? null;
  },
  async findByEmailHash(hash) {
    const { data } = await db
      .from("yap_licenses")
      .select("*")
      .eq("email_hash", hash)
      .order("issued_at", { ascending: false })
      .limit(1)
      .maybeSingle();
    return (data as Issuance | null) ?? null;
  },
  async insert(issuance) {
    const { error } = await db.from("yap_licenses").insert(issuance);
    if (error) throw new Error(error.message);
  },
  async revokedKids() {
    const { data } = await db.from("yap_revocations").select("kid");
    return ((data as { kid: string }[] | null) ?? []).map((r) => r.kid);
  },
};

const mailer: Mailer = {
  async send(to, licenseKey) {
    const res = await fetch("https://api.resend.com/emails", {
      method: "POST",
      headers: {
        authorization: `Bearer ${required("RESEND_API_KEY")}`,
        "content-type": "application/json",
      },
      body: JSON.stringify({
        from: "Yap <wilson@drivia.consulting>",
        to: [to],
        subject: "Your Yap license key",
        html:
          `<p>Thank you for buying Yap.</p>` +
          `<p>Paste this key into Yap → Settings → License:</p>` +
          `<p style="font-family:ui-monospace,monospace;word-break:break-all">${licenseKey}</p>` +
          `<p>It is a lifetime license for this Mac, and it works offline forever. ` +
          `If you ever lose it, open Yap and choose “I already paid — retrieve my license”.</p>`,
      }),
    });
    if (!res.ok) throw new Error(`resend_${res.status}`);
  },
};

const signingKey = await importSigningKey(required("YAP_SIGNING_KEY_PEM"));
const publicKeySpki = b64urlDecode(
  required("YAP_PUBLIC_KEY_SPKI_B64").replace(/\+/g, "-").replace(/\//g, "_"),
);

Deno.serve((req: Request) =>
  handleIssuerRequest(req, {
    store,
    mailer,
    signingKey,
    publicKeySpki,
    webhookSecret: required("STRIPE_WEBHOOK_SECRET"),
    now: () => new Date(),
    log: (line) => console.log(line),
  }),
);

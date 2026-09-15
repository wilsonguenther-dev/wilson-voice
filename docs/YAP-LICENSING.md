# Yap licensing — payment to working dictation

*Owner: Wilson. Status: the issuer is code in this repository (`supabase/functions/yap-license`) and the Stripe payment link is still `active: false` until the first live walk. Nothing below is a plan; everything below is either in the tree or in the release runbook.*

## The flow, end to end

```
  Buy Yap  ──▶  Stripe Payment Link (hosted checkout, one compile-time URL)
                      │  checkout.session.completed  (signed webhook)
                      ▼
            supabase/functions/yap-license  ── Ed25519 sign ──▶  license key
                      │                                              │
                      │ Resend                                       │
                      ▼                                              ▼
              the customer's inbox  ──── paste ────▶  Yap  ── offline verify ──▶ dictation
                      ▲                                    (pinned public key)
                      └──── "I already paid — retrieve my license" ────┘
```

Two properties hold the whole design up, and every change here has to keep them:

1. **Verification is offline and local.** Yap checks a key against a public key compiled into the binary. It does not phone home to activate, it has no account, no session, no "licensed" boolean on disk — the signed key *is* the state, re-verified on every read. A Mac that never touches the internet again keeps working forever.
2. **The issuer is a convenience, not a dependency.** It is contacted for exactly two things: the public revocation list (best effort, on a background task, and every failure mode leaves local state untouched) and retrieval, which only ever happens because a person pressed a button. Neither is on the dictation path.

### What moved in LIC-A

Fulfilment used to be a Fastify service on the Forge box (`drivia-forge`, `server/src/routes/yap.ts`), invisible to this repository and untestable in its gate, and `REVOCATION_URL` was pinned to a hostname with that box's **IP address** in it — so re-IPing the box would have silently killed revocation for every shipped copy of Yap, with no release able to fix it.

The issuer is now a Supabase Edge Function this repository owns, with the routing logic under test in `npm test`. The Forge service is deliberately **not deleted** — two live issuers signing with one key is fine; one dead customer path is not. Decommissioning it is a release step, after the first live walk.

### The wire format is frozen

```
base64url(claimsJson) "." base64url(ed25519 signature)
```

The signature covers the **ASCII bytes of the first segment** — not the JSON, not a re-serialisation of the decoded object. Claims are `{ v, plan, seats, email_hash, issued_at, kid, skid }`. Every shipped copy of Yap pins the public key and this shape, so changing the format forks the verifier silently; changing the *key* would be worse. `supabase/functions/_shared/claims.ts` is the only place the format is written down on the signing side, `desktop/src-tauri/src/license.rs` on the verifying side, and `desktop/src/license/claims.test.ts` is the standing guard that they agree.

No email address is ever signed into a key: `email_hash` is `sha256(lowercased, trimmed address)`. A license key found on a disk names nobody.

### The four routes

| Route | Method | What it does |
|---|---|---|
| `…/stripe-webhook` | POST | Verifies Stripe's signature (HMAC-SHA256 over `t.body`, 300 s tolerance), issues on `checkout.session.completed`, mails the key. **Idempotent on both `event.id` and `session.id`** — a retry or a dashboard replay must never put two live licenses behind one charge. A *mail* failure deliberately still answers 2xx: a non-2xx makes Stripe retry an event whose license is already on record. |
| `…/license` | POST `{email}` | Retrieval. Answers `{key}` for the address that paid; the app activates it in place. |
| `…/revoked.json` | GET | The public revocation list, `{version, updatedAt, kids[]}`, `cache-control: max-age=300`. The only URL a running Yap ever fetches. |
| `…/resend` | POST `{email}` | Mails the existing key again. |

### What a failure says

| Failure | Code | What the person sees |
|---|---|---|
| Bad / truncated paste | `malformed`, `bad_signature` | A sentence naming the next action ("Paste the whole key from your email…", "…reply to that email and we will re-send it"). |
| Key from another issuer | `wrong_signing_key` | "That license key was not issued by Yap. Use the key from your purchase email." |
| Signed, wrong product | `wrong_plan` | Says so, and names what to do next. |
| Refunded / charged back | `revoked` | Says so, and invites a reply if it is a mistake. |
| Retrieval, issuer unreachable | `retrieval_failed` | Says the emailed key still works **with no internet at all**, and to try again in a minute. Never blocks anything. |
| Retrieval, no such purchase | `retrieval_not_found` | Asks for the address that paid, and offers the receipt reply. |

`desktop/src-tauri/tests/activation_e2e.rs` asserts every refusal is a written sentence with an action in it, and that a refused key is never written to disk.

## Runtime Dependencies

Same convention as `ARCHITECTURE.md`. **Nothing here is "assumed present on the machine"**, and none of these values is committed to this repository.

| Value | What it is | Who provisions it | Where it lives | What breaks without it |
|---|---|---|---|---|
| Supabase project (dedicated to Yap) | The project the issuer function is deployed into. Explicitly **not** the Drivia project, which is over its free-tier limits — a licensing outage caused by an unrelated product's usage is the worst possible coupling. | Wilson | Supabase dashboard; ref used by `supabase functions deploy --project-ref` | No issuer at all: no fulfilment, no retrieval, no revocation list. |
| `YAP_ISSUER_BASE_URL` | The deployed function's base URL, e.g. `https://<ref>.functions.supabase.co/yap-license`. Compiled into the app at **build** time (`option_env!`). | Wilson, at release | Build environment only | The binary falls back to an RFC 2606 `.invalid` host that can never resolve: revocation refresh and retrieval both fail harmlessly, offline verification is untouched. The release checklist's liveness check catches this. |
| `YAP_SIGNING_KEY_PEM` | Ed25519 **private** key, PKCS#8 PEM. The same key whose public half every shipped Yap pins — re-keying invalidates nothing today (no customer exists yet) but changing it later orphans every issued license. | Wilson | `supabase secrets set` only. Never in this repo, never in an `.env` anything else reads. Existing copy: Forge box `/etc/forge/yap/license-signing-ed25519.pem`, `root:root 0400`. | The function throws at cold start. No keys are issued. Nothing is issued unsigned. |
| `YAP_PUBLIC_KEY_SPKI_B64` | The public half, base64 DER. Used only to derive `skid`. | Wilson | Supabase secrets | Cold-start failure. (The value itself is public — it is compiled into every copy of Yap as `ISSUER_PUBLIC_KEY_SPKI_B64`.) |
| `STRIPE_WEBHOOK_SECRET` | `whsec_…` for the registered endpoint. | Wilson, in Stripe | Supabase secrets | Every webhook is refused with 400 and no license is ever issued. Stripe's dashboard shows the failures. |
| `RESEND_API_KEY` | Delivery of the key email. | Wilson | Supabase secrets | Issuance still succeeds and is recorded; the email does not arrive, and the customer recovers with "I already paid — retrieve my license". |
| `SUPABASE_URL`, `SUPABASE_SERVICE_ROLE_KEY` | Platform-injected; the function's own database access. | Supabase | Injected into the function runtime | Cold-start failure. |
| Tables `yap_licenses`, `yap_revocations` | One row per issuance (`kid`, `email`, `email_hash`, `license_key`, `issued_at`, `event_id`, `session_id`); the revocation list. | Wilson, once | The Yap Supabase project | Issuance fails; idempotency has nowhere to record itself. |
| Stripe Payment Link | The one checkout URL, `license.rs::PAYMENT_LINK_URL`. Currently `active: false` on purpose. | Wilson, in Stripe | Compiled into the app | The Buy button opens a page saying the link is not open yet — the honest state of the world, not a bug in Yap. |

## Testing without any of it

The gate is **offline**. `handleIssuerRequest` takes its store, its mailer, its clock and its signing key as injected dependencies, so `desktop/src/license/issuer.test.ts` drives the real handler with a fixture Stripe event and a throwaway key pair and never opens a socket. `supabase` does not have to be installed for `npm test` to pass, and it is not a gate conjunct.

The local-stack walk is a **documented manual step**:

```bash
supabase start                       # local Postgres + Edge runtime
supabase functions serve yap-license --no-verify-jwt --env-file supabase/.env.local
# .env.local is git-ignored and holds a STAGING key pair, never the production one
YAP_ISSUER_BASE_URL=http://127.0.0.1:54321/functions/v1/yap-license \
  npm run desktop:dev                # then: Buy sheet → "I already paid" → retrieve
```

The proof that matters is the one in `docs/RELEASE.md` § ISSUANCE: a key issued from a real deploy, activated in a clean `YAP_DATA_DIR`, with a dictation completing afterwards.

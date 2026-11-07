# Yappy Social: threat model (2026-11)

Written 2026-11-07T02:30Z for loop item `yap-1106-fb-19-social-threat-model`.
Gate: `yap-1106-social-relay-db2` stays a DRAFT PR until this document has had
one Senior Panel pass and every BLOCKING finding below (and from the panel) is
fixed. The panel pass is run by the orchestrator, not by a builder.

Sources read for this model: the approved design page
`Projects/Yap/design-2026-11-06/social.html` (sections 1 to 6 and section 10,
"Adding friends, your ID, and what the relay can see"), the relay item
`yap-1106-social-relay-db2`, `desktop/src-tauri/src/license_client.rs`
(activation client, `LICENSING_BASE`, device id), and
`desktop/src-tauri/src/license_v2.rs` (`TokenClaims`, `EMBEDDED_KEYS`,
`GRACE_S`, `PLAN`).

Words used here, defined once:

* **Relay**: the server half of Social. Tables in schema `social` on DB #2
  (the consulting Supabase project), edge functions in front of them, and that
  project's realtime presence channels. It stores and forwards; it is not
  trusted with content.
* **License token**: the EdDSA-signed JWS the Mac got from
  `https://drivia.consulting/api/v1/licensing/activate`, header kid
  `dcl-2026-09-24`, claims `sub` (license id), `act` (activation id), `prd`,
  `plan`, `ent`, `dev` (the install UUID), `iat`, `exp`.
* **Device key**: two key pairs the Mac makes when it joins Social. An Ed25519
  signing pair (proves "this request came from that Mac") and an X25519 box
  pair (lets friends lock things that only that Mac can open). Private halves
  live only in the macOS Keychain.
* **Row rules**: Postgres row level security policies. They decide which rows a
  signed-in caller can see, whatever query it sends.
* **Session token**: a short-lived JWT the relay mints for a device after it
  proves it holds its device key; the Mac uses it for reads and for realtime.

## 1. What leaves the Mac, and what never does

| Data | Leaves the Mac? | Who can read it |
|---|---|---|
| Audio, transcripts, dictionary, scratchpad | Never | Nobody else |
| App names, window titles, meeting attendees, meeting topics | Never | Nobody else |
| License key (`DCL-...`) | Never to the relay | Licensing only (existing flow) |
| License token | Once, at join, and on device add | Relay edge function, verified then dropped (not stored) |
| Handle, chosen character | Yes | Relay (needed for the invite card), friends, invite page visitors |
| Bio | Yes, locked for friends | Friends only (inside ciphertext) |
| Post line, session kind, length | Only after a tap on Post | Friends only (ciphertext; relay holds no key) |
| Presence state (live, idle, meeting, asleep) | If presence is on | The friends allowed to see it (ciphertext) |
| Friend graph, pending requests, timestamps, sizes | Yes | Relay (metadata it needs to route) |
| Invite code | As a fingerprint only | Relay stores HMAC, never the code |

The relay can therefore learn who is friends with whom, when posts and
presence changes happen, and when a Mac is connected. It cannot learn what a
post says or which presence state someone is in. The privacy page and the
in-app "Leaves this Mac" labels must say exactly this, by category, with no
vendor or model names (rules.product).

## 2. Networking and ports

Wilson asked about ports first, so they come first.

* **The Mac opens no listening port.** No TCP or UDP listener, no LAN
  discovery (no Bonjour/mDNS), no WebRTC, no peer to peer, no NAT traversal.
  Every Social byte is an outbound HTTPS request or one outbound secure
  WebSocket, both on port 443.
* **One relay host, pinned in code.** A `SOCIAL_RELAY_BASE` constant, built the
  same way as `LICENSING_BASE` in `license_client.rs`: no runtime override in
  release builds, no host taken from a server reply, a deep link or a setting.
  Only `#[cfg(test)]` code may point a client at a stub server.
* **TLS** uses the system trust store through the existing HTTP stack. No
  plain HTTP fallback; any redirect to another host is refused.
* **Not on the dictation path.** Social code must stay out of the dictation and
  meeting capture path; the existing `tests/no_outbound_on_the_dictation_path.rs`
  is extended to cover the Social module.
* **Off means no sockets.** With Social not joined, paused, or killed by the
  operator switch (section 8), the app makes no Social connection at all.
* **The invite page** (`yapvoice.app/add/:code`) is a server route on the Yap
  site, not on the Mac. It needs read-only access to one view (handle +
  character for a valid code) and nothing else.

Test the relay builder adds: `social_no_listener` (Rust) asserts that the
Social module contains no `TcpListener`, `UdpSocket::bind`, `bind(` or mDNS
use, and that every Social URL is built from `SOCIAL_RELAY_BASE`.

## 3. Who we defend against

1. **A stranger on the internet** who saw an invite link on Instagram or X.
2. **A spammer or scraper** with many free Macs or scripts and no paid license.
3. **A former friend** who was removed and wants to keep watching.
4. **A friend** who wants to learn more than they were shown (other friends,
   app names, meeting details).
5. **Someone holding a copied license token** (from a backup, a shared Mac, a
   leaked log).
6. **The relay operator or a DB #2 breach**: someone who can read every table.
   The design promise is that this person still cannot read posts or presence.
7. **A buggy release** of Yap or of the relay that must be stopped fast.

Out of scope: an attacker who already controls the user's Mac (they can read
the screen; Social adds nothing they lack).

## 4. Relay auth: license token plus device key

### 4.1 Join (once per Mac)

1. Social is offered only when the local license status is entitled and the
   plan is the paid plan (`license_v2::PLAN`). Trials never see Join.
2. The Mac makes the Ed25519 signing pair and the X25519 box pair and stores
   the private halves in the Keychain through `keychain.rs`'s `SecretStore`,
   under a new `.social` service suffix next to the existing `.license` one
   (rules.irreversible: the bundle identifier is not touched).
3. The Mac sends `social-join` one body: the license token, handle, character,
   both public keys, its install UUID, a timestamp and a random nonce. The
   whole body is signed with the new Ed25519 private key. That signature is
   proof of possession: the key that will sign later requests made this one.
4. The edge function, in order, and refusing at the first failure:
   * parses the JWS header: `alg` must be `EdDSA`, `kid` must be
     `dcl-2026-09-24` (or a later kid the operator adds to the relay's own
     pinned list), no `crit`, token at most `license_v2::MAX_TOKEN_LEN` bytes;
   * verifies the signature with the pinned public key from
     `https://drivia.consulting/.well-known/drivia-licensing-keys.json`, copied
     into the function at build time. The function never fetches keys at
     request time and never trusts a `jku`/`x5u` header. This is the
     drivia.consulting key, NOT the old Forge key in `license.rs`;
   * checks `prd == "yap"`, `plan == "lifetime"`, `ent` grants Yap, and
     `now <= exp + GRACE_S`;
   * checks `dev` equals the install UUID in the body;
   * looks the activation `act` up in the licensing tables on DB #2 (same
     database, read through a security-definer function): it must exist, be
     active, and not be revoked, refunded, disputed or deactivated;
   * verifies the body signature with the submitted signing public key and
     checks the timestamp is within 120 seconds and the nonce is unseen.
5. On success: one profile per license. The profile id is a new random UUID.
   The license is remembered only as `license_ref = HMAC(pepper, sub)` and
   `act`, so the social tables do not hold the license id or anything that
   names the buyer. The token itself is not stored.
6. A second Mac on the same license joins the existing profile only if its
   `act` is a different active activation of the same license, up to the seat
   limit. Every existing device gets a "A new Mac joined your Social" notice
   with Remove.

### 4.2 Every request after join

* Writes go through edge functions only. Each carries `X-Yap-Device` (device
  row id), `X-Yap-Ts`, `X-Yap-Nonce` and `X-Yap-Sig`, an Ed25519 signature over
  `method \n path \n sha256(body) \n ts \n nonce`. Clock skew allowed: 120 s.
  Nonces are kept 10 minutes and a repeat is refused (`replayed`).
* Reads and realtime use a session token from `social-session`: the device
  signs a server challenge, the function mints a JWT valid 15 minutes with
  claims `role=authenticated`, `profile_id`, `device_id`. The signing secret
  lives only in edge-function secrets. The relay builder must confirm against
  the current Supabase docs how custom claims are minted for this project's
  JWT signing setup before writing the function (research-first rule).
* The license token is never sent again after join. License health is
  re-checked server-side once a day from the licensing tables by `act`.

### 4.3 Threats and answers

| Threat | Answer | Blocking? |
|---|---|---|
| Free or trial user joins | Join needs a verified paid token and an active activation | BLOCKING |
| Forged token | Pinned EdDSA key, alg and kid allowlist, no remote key fetch | BLOCKING |
| Revoked or refunded license keeps posting | Daily `act` re-check; revoked or refunded = profile deleted within 24 h; disputed = profile hidden until it clears | BLOCKING |
| Copied token used on another Mac | `dev` must match and `act` must be active; a different `act` needs a free seat; existing devices are told and can remove it; deactivating that seat at drivia.consulting removes the device | BLOCKING |
| Replayed request | Signed timestamp + nonce, 120 s window, nonce store | BLOCKING |
| Stolen session token | 15-minute life, bound to device; device removal revokes at next mint and row rules check the device is still listed | ADVISORY |
| Key confusion (the design page calls the license id `kid`) | The relay never uses the JWS `kid` as an identity; identity is `profile_id`, the license is `license_ref` | BLOCKING |

## 5. Row rules: a profile reads only accepted friends' rows

Principles:

* The `authenticated` role gets SELECT only. INSERT, UPDATE and DELETE on every
  `social` table are revoked from `anon` and `authenticated`; edge functions
  write with the service role after the signature checks in section 4. `anon`
  gets nothing at all (and never TRUNCATE or TRIGGER).
* RLS is enabled and forced on every table. The caller is
  `social.current_profile()`, read from `auth.jwt() ->> 'profile_id'`.
* One helper, `social.are_friends(a, b)`, security definer, true only when a
  friendship row between the two has `state = 'accepted'` and neither side has
  blocked the other.

| Table | Who may SELECT |
|---|---|
| `profiles` | self, and accepted friends (handle, character, public keys only) |
| `devices` | self, and accepted friends (public keys, so they can lock posts for you) |
| `friendships` | only rows where the caller is one of the two sides |
| `posts` | self, and accepted friends of the author |
| `post_keys` | only rows whose recipient device belongs to the caller |
| `presence_keys` | only rows whose recipient device belongs to the caller |
| `invites` | nobody through the API (service role only) |
| `blocks`, `reports`, `rate_events`, `nonces`, `settings` | nobody through the API |

Friends never see each other's friend lists: a friendship row is visible only
to its two sides, and presence channels are per owner (section 7).

### 5.1 The two fake users test (BLOCKING)

The relay builder ships this as an executable test in the relay's SQL test
folder (`social_rls_two_users.sql`), run inside one transaction that ends in
ROLLBACK, against a local or branch database, never against DB #2 production.
Column names follow the relay migration; the shape is binding:

```sql
begin;
-- two fake users
insert into social.profiles (id, handle, character) values
  ('00000000-0000-4000-8000-0000000000a1', 'test_alice', 'sal'),
  ('00000000-0000-4000-8000-0000000000b2', 'test_bob',   'mochi');
insert into social.posts (id, profile_id, ciphertext, nonce) values
  ('00000000-0000-4000-8000-0000000000c1', '00000000-0000-4000-8000-0000000000a1', '\x01', '\x02');
insert into social.friendships (requester, addressee, state) values
  ('00000000-0000-4000-8000-0000000000b2', '00000000-0000-4000-8000-0000000000a1', 'pending');

-- phase 1: pending. Bob sees nothing of Alice.
set local role authenticated;
select set_config('request.jwt.claims',
  '{"role":"authenticated","profile_id":"00000000-0000-4000-8000-0000000000b2"}', true);
do $$ begin
  if (select count(*) from social.posts) <> 0 then raise exception 'FAIL pending friend read a post'; end if;
  if (select count(*) from social.profiles where handle = 'test_alice') <> 0 then raise exception 'FAIL pending friend read a profile'; end if;
  begin
    insert into social.posts (profile_id, ciphertext, nonce)
      values ('00000000-0000-4000-8000-0000000000a1', '\x00', '\x00');
    raise exception 'FAIL bob inserted a post as alice';
  exception when insufficient_privilege then null; end;
  begin
    update social.friendships set state = 'accepted';
    if found then raise exception 'FAIL bob accepted his own request'; end if;
  exception when insufficient_privilege then null; end;
end $$;

-- phase 2: Alice accepts (the edge function's service-role write). Bob reads the post.
reset role;
update social.friendships set state = 'accepted'
  where requester = '00000000-0000-4000-8000-0000000000b2';
set local role authenticated;
select set_config('request.jwt.claims',
  '{"role":"authenticated","profile_id":"00000000-0000-4000-8000-0000000000b2"}', true);
do $$ begin
  if (select count(*) from social.posts) <> 1 then raise exception 'FAIL accepted friend cannot read the post'; end if;
end $$;

-- phase 3: Alice removes Bob. Bob reads nothing again.
reset role;
delete from social.friendships where requester = '00000000-0000-4000-8000-0000000000b2';
set local role authenticated;
select set_config('request.jwt.claims',
  '{"role":"authenticated","profile_id":"00000000-0000-4000-8000-0000000000b2"}', true);
do $$ begin
  if (select count(*) from social.posts) <> 0 then raise exception 'FAIL removed friend still reads posts'; end if;
end $$;

-- phase 4: no session at all. anon reads nothing anywhere.
reset role;
set local role anon;
select set_config('request.jwt.claims', '{"role":"anon"}', true);
do $$ begin
  if (select count(*) from social.profiles) + (select count(*) from social.posts)
     + (select count(*) from social.friendships) <> 0 then raise exception 'FAIL anon read social rows'; end if;
exception when insufficient_privilege then null; end $$;
rollback;
```

The same two users drive the realtime test in section 7.3.

## 6. Rate limits

Edge functions are stateless, so limits are counted in Postgres
(`social.rate_events`, keyed by profile, device, or a salted hash of the IP for
the public page), checked before any work. Over the limit = HTTP 429 with
`Retry-After`; the Mac honours it the way `license_client.rs` already does and
shows one sentence, never a retry storm.

| Action | Limit | Why |
|---|---|---|
| `social-join` | 3 per license per day, 10 per IP per day | Stops seat churn and profile farming |
| `social-session` (token mint) | 60 per device per hour | Normal use is 4 per hour |
| `social-post` | 30 per profile per day (relay acceptance) | Matches the design |
| `social-invite` create / rotate | 5 per profile per day | Design section 10 |
| `social-request` (use a code) | 20 per device per hour, 50 open outgoing requests | Design section 10, plus a cap |
| Incoming pending requests | 200 per profile; the oldest drop silently | A viral link cannot fill the inbox forever |
| Invite page and code lookups | 30 per IP hash per minute; 1,000 failed lookups per hour site-wide raises an alert | Code guessing |
| `social-report` | 10 per profile per day | Abuse of the report path |
| Presence updates | 1 per 10 s per device; the realtime client never sends faster | Cost and timing leakage |
| Feed reads | Mac never polls faster than every 30 s (standing rule) | Cost |

Ciphertext size caps: a post at most 2 KB, a wrapped key at most 128 bytes,
at most 3 devices per profile (the seat limit), so one post fans out to at
most a bounded number of `post_keys` rows. Oversized bodies are refused before
parsing (the same idea as `MAX_REPLY_BYTES` in `license_client.rs`).

## 7. Presence: only the state, never the app, title or attendees

### 7.1 What a presence message may contain

Exactly one of four states (`live`, `idle`, `meeting`, `asleep`) and the
minute it began. Nothing else. No app name, bundle id, window title, URL,
meeting platform, attendee, topic, duration counter or transcript fragment.
The "Working on: ..." line on the design page is typed by the user and goes
out only through the normal tap-to-post path, never through presence.

`asleep` is forced from 11 PM to 7 AM local time whatever the Mac is doing,
which also hides late-night work patterns.

### 7.2 Locked presence (BLOCKING design correction)

The design page promises the relay cannot read which state you are in. Plain
realtime presence would put the state on the server in clear text, so:

* each profile has a random presence key; the Mac locks the presence message
  with it (XChaCha20-Poly1305), padded to a fixed size so every state is the
  same length;
* the presence key is wrapped with each allowed friend's device box key and
  stored in `presence_keys`, readable only by that recipient (section 5);
* when the audience shrinks (unfriend, switch to "chosen friends", remove one
  friend from the chosen list) the Mac makes a new presence key and wraps it
  only for the remaining audience. A removed friend keeps the old key but
  never sees a message locked with the new one.

What the relay still learns: that a Mac is connected, and when a presence
change happened. The privacy page says so.

### 7.3 Channels

* One private channel per owner: `presence:<owner profile_id>`. Only the owner
  tracks in it. Friends only listen; they never track, so subscribers never
  show up to each other and a friend cannot list your other friends. Shared
  "friend group" channels from the design's comparison table are NOT used,
  because they would show every member to every other member.
* Realtime authorization through row rules on `realtime.messages`: insert
  (track) only when the topic is the caller's own channel; select (listen)
  only when `social.are_friends(caller, owner)` and the owner's presence mode
  allows this viewer (`all`, or `chosen` and the viewer is chosen; `off`
  allows nobody).
* A session token expiry closes the socket; the Mac re-mints and re-joins.
* Default is ON for all friends (Wilson-approved). The Join screen says it in
  one plain sentence and shows the Off / All friends / Chosen friends switch
  BEFORE the first presence message is sent.
* Presence never runs while Social is paused, during the 11 PM to 7 AM sleep
  window it only ever says `asleep`, and Pause removes presence within 2 s.

### 7.4 Presence tests (BLOCKING)

* `social_presence_payload` (Rust): the serialized presence message, before
  locking, has exactly the fields `state` and `since_minute`; the locked bytes
  are the same length for all four states; a fake session with an app name,
  a window title and attendees in Yap's state produces no trace of any of
  them in the bytes handed to the network layer.
* `social_presence_channel` (relay): with the two fake users of section 5.1,
  Bob cannot listen to Alice's channel while pending, can after accept, cannot
  after removal or when Alice's mode is `off`, and can never track on Alice's
  channel.

## 8. Invite-link abuse

The invite is `yapvoice.app/add/<code>`: 8 characters from a 31-character
alphabet (A to Z and 2 to 9, minus 0, O, 1, I and L), about 8.5 x 10^11
codes.

| Abuse | Answer |
|---|---|
| Guessing codes | Large space, 30 lookups per minute per IP hash, a site-wide failed-lookup alarm, codes expire after 7 days by default |
| A post goes viral | Max 25 uses by default, counted when a request is sent (not on page view); "Turn off" and "New code" act within a minute (the page caches at most 60 s) |
| Leaked database | Codes are stored only as `HMAC(pepper, normalized code)`; the pepper lives in edge-function secrets |
| Request spam through a valid code | Every request still needs the owner's Accept; Ignore is silent; per-device and inbox caps (section 6); Block stops that profile from requesting again through any code |
| Finding people by handle | No search, no handle lookup, no public profile; a handle alone adds nobody |
| Impersonation (`@wi1son`) | Handles are lowercase ASCII `a-z`, `0-9`, `_`, 3 to 20 characters, unique after case folding, with a reserved list (yap, yappy, drivia, support, admin, staff, official and similar); the request card shows the character and "from your invite K7QM" so the owner sees which link it came through |
| The invite page as an attack surface | It shows handle + character image only, from a fixed set of characters (no uploaded images); everything HTML-escaped; no redirect parameter; no other profile field is reachable from that route |
| Deep link `yap://add/<code>` injection | The app checks the code against the exact alphabet and length before any network call and never sends a request without the "Send a friend request to @x?" confirmation |
| A code in logs | Codes, handles and tokens are never written to app telemetry or edge-function logs |

## 9. Kill switches

Four levels, from the user to the operator:

1. **Pause Social** (menu bar and Settings): stops presence within 2 s, closes
   the socket, holds drafts in the outbox, makes no Social request until
   resumed. Local only, works offline.
2. **Leave Social**: a signed `social-leave` deletes the profile, devices,
   friendships, posts, wrapped keys, presence keys and invites (one cascade).
   The Mac shows "Done" only after the call returns and a follow-up read
   proves the profile is gone, within 60 seconds; then it deletes its local
   Social cache and Keychain entries. If offline, it says it will finish when
   back online and keeps retrying; it never claims done early.
3. **Remove a Mac / a friend / block**: device removal stops that device's
   mints and drops its wrapped keys; unfriending rotates the presence key and
   stops wrapping new posts for that friend.
4. **Operator switch**: one row in `social.settings` (`enabled`, `read_only`).
   Every edge function checks it first and answers `503 social_paused`; every
   row rule and realtime policy also requires `social.enabled()`, so reads and
   presence stop at once even for live sessions. The Mac treats
   `social_paused` like Pause: no presence, drafts held, one sentence on the
   Friends tab. Flipping the row back restores everything with no app update.
   License revocation (section 4.3) is the per-person version of this switch.

Kill switch tests (relay builder): with `enabled = false`, every function
returns 503 and the two fake users read zero rows and receive no presence;
Leave Social leaves zero rows for that profile in every `social` table.

## 10. Other risks the panel should see

* **Moderation without reading.** Posts are locked, so the relay cannot scan
  them. A report sends the post as the reporter's Mac unlocked it, plus the
  author device's signature from inside the ciphertext, so a report cannot be
  forged against someone. Reports go to a table only the operator can read.
* **Local copies.** The unlocked feed is cached in Yap's existing local
  database for offline reading. Leave Social clears it. Removing a friend does
  not erase what they already saw on their Mac (said plainly in the UI).
* **Metadata on the relay.** The friend graph, timestamps, sizes and request
  IP addresses at the edge are visible to the operator of DB #2. The privacy
  page names these categories. Raw IPs are not stored in `social` tables; only
  salted hashes for rate limiting, kept 24 hours.
* **Telemetry.** App lines follow the rules (`social <event> key=value`):
  counts, durations and error codes only, never handles, codes, post text,
  presence state, keys or tokens.
* **Drafts.** The draft is made on the Mac; the transcript is never sent to the
  relay. Sessions in password fields or during screen sharing are never
  offered. A draft expires after 10 minutes and expiry posts nothing.
* **Same database as licensing.** Social lives in its own `social` schema on
  DB #2; its functions get only a narrow security-definer read of activation
  status, never the customer tables.

## 11. Corrections the relay builder must adopt

1. **Which license key.** The design sketch says the relay checks the license
   "with the same public key Yap ships, license.rs" and asks where licenses
   are issued now that Forge is down. Answer: drivia.consulting issues them;
   verify the EdDSA JWS with kid `dcl-2026-09-24` (`license_v2.rs`
   `EMBEDDED_KEYS`), not the old Forge key in `license.rs`.
2. **`kid` is not an identity.** The design's tables key everything on `kid`
   (meaning the license id). In the token, `kid` is the signing key's name.
   Use `profile_id` (random UUID) as the key everywhere and keep the license
   only as `license_ref`.
3. **Presence must be locked** (section 7.2), or the "relay cannot read your
   presence state" promise on the design page and privacy page is false.
4. **Per-owner presence channels**, not shared friend-group channels (7.3).
5. **More functions than the relay acceptance names.** Besides `social-join`
   and `social-post`: `social-session`, `social-invite`, `social-request`,
   `social-accept` / ignore / remove / block, `social-leave`,
   `social-device-remove`, `social-report`. They may be one function with
   routes; all of them share the section 4.2 signature check and the section
   9 operator switch.
6. **No migration is applied to DB #2 from the loop.** Migrations and functions
   are written as files; the SQL tests run against a local or branch database.

## 12. Release gate for `yap-1106-social-relay-db2`

The PR leaves DRAFT only when all of these hold:

- [ ] Senior Panel pass on this document and the Social design page, every
      BLOCKING finding fixed (orchestrator).
- [ ] `social_rls_two_users.sql` passes (section 5.1).
- [ ] License verification tests in the join function: wrong alg, unknown kid,
      `plan` not lifetime, `prd` not yap, expired past grace, `dev` mismatch,
      revoked or disputed activation, each refused with its own code.
- [ ] Signature tests: bad signature, old timestamp, replayed nonce refused.
- [ ] Rate limit tests for post (31st post in a day gets 429), invite, request.
- [ ] `social_presence_payload` and `social_presence_channel` pass (7.4).
- [ ] `social_no_listener` passes (section 2).
- [ ] Kill switch and Leave Social tests pass (section 9).
- [ ] Privacy page and in-app labels updated, categories only, no vendor names.

## 13. Open questions for the panel

1. Should a second Mac on the same license need approval from an existing Mac
   (stronger against a copied token) instead of a notice with Remove (simpler)?
2. Is 7 days / 25 uses the right invite default, or should a public-post code
   default to 24 hours?
3. Should the Mac pin the relay's TLS certificate, given the host is a
   platform-managed domain whose certificate rotates?
4. Is keeping the friend graph visible to the relay acceptable for launch, or
   does the panel want it hidden too (much harder; not in the approved design)?

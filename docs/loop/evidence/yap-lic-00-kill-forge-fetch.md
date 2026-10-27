ROOT-CAUSE-PROBE yap-lic-00-kill-forge-fetch

# yap-lic-00-kill-forge-fetch — evidence (stamped 2026-10-26T21:35:00Z from date -u)

## Root cause (probe on unmodified origin/main 821910c)

Pre-flight chain on origin/main exited 1. On that commit `license.rs` defined the issuer host and the
revocation URL (a wildcard-DNS name for the decommissioned Forge box IP), `fetch_revocations` built a
reqwest client and GET it, and `lib.rs` called `spawn_revocation_refresh` from two places: inside
`activate_license` and in the `setup` hook once per launch. The installed 0.9.3-dev logs the result on
every launch ("license: revocation refresh skipped (error sending request for url ...)", audit
2026-10-26 16:35:12Z, docs/audits/meeting-notetaker-audit-2026-10-26.md issue #15). The response, if any
host ever answered, was an UNSIGNED kid list the gate trusted.

## Fix

- Deleted both constants, the fetch, `RevocationDocument`, `apply_fetch_result` and `spawn_revocation_refresh` plus both call sites. No replacement endpoint.
- Launch init is now `license::launch_init(dir, store, &license::NoNetwork)`; the transport is injected so a recording fake can prove zero requests.
- A cached `revoked_kids` list on disk is still read and still honoured (`decide_entitlement` unchanged). `apply_revocations` is now `#[cfg(test)]` (seeds that cache in tests); its named future production consumer is the yap-lic-0x client.
- PRIVACY.md lists two network calls and states licensing makes none.

## RED first: mutation run (test kind (1))

Injected `let _ = transport.get("https://mutation.example/revoked.json"); // reqwest mutation` into `launch_init`, ran
`cargo test --features custom-protocol --lib license`, exit 101, then restored the file byte-for-byte (cmp ok):

```
test license::tests::launch_license_init_makes_no_network_call ... FAILED
test license::tests::license_rs_has_no_http_client ... FAILED
thread 'license::tests::launch_license_init_makes_no_network_call' (3304595) panicked at src-tauri/src/license.rs:1898:9:
thread 'license::tests::license_rs_has_no_http_client' (3304611) panicked at src-tauri/src/license.rs:1933:13:
test result: FAILED. 38 passed; 2 failed; 0 ignored; 0 measured; 603 filtered out; finished in 0.02s
```

(On unmodified main neither test exists, so the acceptance grep for them fails there too.)

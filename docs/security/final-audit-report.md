## Final Audit Report

**Date:** 2026-03-25
**Target:** stophammer v0.1.0

---

### Verification Status

- `cargo build`: PASS
- `cargo test`: PASS across the full suite, including the new `sync/register` hardening regression test
- `cargo clippy -- -D warnings`: PASS
- `cargo fmt -- --check`: PASS

---

### Security Audit Summary

Manual review and automated coverage both indicate the current tree has no open
critical or high-severity issues in the primary auth, proof, sync, and
replication paths.

The only code-level issue found in this audit pass was a primary-side
`POST /sync/register` ownership-check gap: `node_url` was validated first, but
the follow-up same-origin `GET /node/info` fetch could still re-resolve DNS or
follow a redirect. That created a DNS-rebinding / redirect window on the
verification fetch. The fix now:

- disables redirects for the `node/info` ownership check
- resolves and validates the exact `node/info` URL inside `spawn_blocking`
- pins the validated addresses into the verification client before the fetch
- keeps the existing test-only `skip_ssrf_validation` escape hatch for localhost
  mocks without weakening production behavior

Regression coverage was added in
[`tests/sync_register_ssrf_tests.rs`](/home/citizen/build/stophammer/tests/sync_register_ssrf_tests.rs)
to ensure redirected ownership checks fail closed.

---

### Current Controls

The audited code currently enforces the following materially important controls:

- Sync endpoints require a dedicated `X-Sync-Token`; admin tokens are not
  accepted on the sync surface.
- Admin token comparison is constant-time via SHA-256 + `subtle::ConstantTimeEq`.
- Proof assertion re-fetches RSS and verifies exact `podcast:txt` matches before
  issuing bearer tokens.
- RSS proof fetches use SSRF validation, redirect re-validation, DNS pinning,
  and a bounded response body.
- Community push registration now validates URL shape, signature freshness,
  same-origin ownership, SSRF safety, and the no-redirect DNS-pinned
  `node/info` fetch described above.
- Challenge resolution remains single-use via SQL `WHERE state = 'pending'`
  semantics, closing duplicate-issuance races.
- SSE fan-out is bounded by per-connection artist caps, a global registry cap,
  and a global concurrent connection cap.

---

### Residual Risk

No immediate code changes are required for CI or security correctness after the
fix above. The remaining meaningful risks are operational rather than
implementation bugs:

- `CORS_ALLOW_ORIGIN` defaults to `*`; operators should narrow this in browser
  deployments.
- `ADMIN_TOKEN` and `SYNC_TOKEN` remain bearer secrets and should be treated
  like infrastructure credentials.
- `ALLOW_INSECURE_PUBKEY_DISCOVERY=true` is still development-only and should
  not be enabled on public deployments.

---

### Documentation State

The API, operations, and security docs were updated in this pass to match the
current codebase, including:

- sync endpoint auth wording (`X-Sync-Token`, not `X-Admin-Token`)
- proof challenge semantics (replacement of prior pending challenges plus the
  global pending cap)
- proof assert response shape (`proof_level`) and status codes
- TLS environment variable coverage (`TLS_ACME_DIRECTORY_URL`)
- current security posture of `sync/register`, RSS proof fetches, and SSE limits

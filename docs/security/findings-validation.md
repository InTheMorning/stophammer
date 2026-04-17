# Security Findings Validation Report

**Updated:** 2026-03-25

This document supersedes the earlier 2026-03-13 point-in-time validation notes.
Several findings that were accurate at that time are no longer accurate against
the current tree.

Use these as the current sources of truth instead:

- [final-audit-report.md](final-audit-report.md)
- [auth-blackbox-report-v2.md](auth-blackbox-report-v2.md)
- [crypto-blackbox-report-v2.md](crypto-blackbox-report-v2.md)
- [availability-blackbox-report-v2.md](availability-blackbox-report-v2.md)

## Current Status of the Earlier Findings

1. Artist-merge alias transfer: closed by current merge-path coverage and
   regression tests.
2. Mutation/event atomicity gaps: closed by transactional mutation paths and
   current CI coverage.
3. Community onboarding requiring admin credentials: superseded. Sync endpoints
   now use a dedicated `SYNC_TOKEN` via `X-Sync-Token`.
4. Single global DB bottleneck: superseded. The current code uses a WAL-aware
   `DbPool` with a writer plus reader pool.
5. `POST /sync/reconcile` pagination robustness: superseded. The current API
   returns `has_more` and `next_seq` and the implementation is covered by
   pagination tests.
6. Proof-of-possession weaker than documented: partially historical. The current
   implementation is explicitly documented as RSS-only proof via `proof_level =
   rss_only`; Phase 2/3 audio and relocation proofs remain intentionally
   unimplemented.
7. Verifier-chain unknown-name fail-open: superseded. Unknown verifier names now
   fail closed rather than silently skipping.

If a future audit needs the original historical text, recover it from git
history instead of treating the old statuses as current.

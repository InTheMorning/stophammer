# ADR 0026: Signed Peer Registration

## Status
Accepted

Date: 2026-03-18

## Context
Community nodes register their push endpoint with the primary via
`POST /sync/register`.

The existing hardening already requires a dedicated `SYNC_TOKEN` (or legacy
`ADMIN_TOKEN`) and applies SSRF validation to the submitted `node_url`.
That closes anonymous registration and obvious SSRF attacks, but it still
leaves one integrity gap:

- the request body carries `node_pubkey` as an unauthenticated string
- any caller holding the sync token can claim any pubkey
- the primary will overwrite the stored `peer_nodes.node_url` for that pubkey

This is primarily a resilience problem. It lets a misconfigured or compromised
community node hijack push delivery for another peer and poison the primary's
peer table until retries evict the real node.

## Decision
`POST /sync/register` is extended with an optional signed payload:

- `signed_at`
- `signature`

The signature covers:

- `node_pubkey`
- `node_url`
- `signed_at`

and is verified against `node_pubkey` using the same Ed25519 key material the
community node already uses for event signing.

Rollout is additive:

1. new community nodes sign registration requests
2. the primary verifies the signature when present
3. legacy unsigned registration remains accepted temporarily, but is logged as
   deprecated
4. once all managed nodes have upgraded, unsigned registration can be removed

The signed registration payload is independent of sync-event signing. It does
not enter the replicated event log; it only authenticates the registration
request itself.

## Consequences
- A node can no longer impersonate another node's pubkey unless it controls the
  corresponding signing key.
- Push-route hijacking risk is materially reduced without changing the existing
  sync-token model.
- Registration remains idempotent and replay-safe enough for current purposes,
  because the operation is an upsert; exact anti-replay nonce tracking is not
  required for the initial hardening step.
- The protocol gains a temporary compatibility branch that must eventually be
  removed once all deployed nodes sign registration requests.

# ADR 27: Authenticate sync read endpoints

## Status
Accepted

Date: 2026-03-18

## Context
The sync replication surface currently exposes two unauthenticated read
endpoints on both primary and community nodes:

- `GET /sync/events`
- `GET /sync/peers`

That makes replication easy to bootstrap, but it also leaks the signed event log
and the current peer topology to any caller that can reach a node. After
separate sync credentials and signed peer registration were added, this is the
largest remaining avoidable sync-surface exposure.

## Decision
The sync read endpoints now use the same authentication rule as the sync write
endpoints:

- when `SYNC_TOKEN` is configured, callers must send `X-Sync-Token`
- otherwise the node falls back to `X-Admin-Token`

This applies to:

- `GET /sync/events`
- `GET /sync/peers`
- `POST /sync/register`
- `POST /sync/reconcile`

Community nodes use the same sync credentials for fallback polling that they
already use for registration.

## Consequences
- The signed event log and peer list are no longer publicly enumerable by
  default.
- Community nodes now require sync credentials not only for registration and
  reconcile, but also for fallback polling.
- Operators must include sync auth headers in debugging and monitoring calls to
  `/sync/events` and `/sync/peers`.
- Primary-as-tracker bootstrapping now requires the primary URL plus sync
  credentials, not just the primary URL alone.

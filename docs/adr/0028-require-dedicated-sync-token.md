# ADR 0028: Require Dedicated Sync Token

## Status
Accepted

Date: 2026-03-18

## Context
ADR 0027 authenticated sync read endpoints, but it still allowed sync
replication calls to fall back to `X-Admin-Token` when `SYNC_TOKEN` was unset.

That preserved backward compatibility, but it kept an avoidable least-privilege
gap:

- a replica credential could still become full admin authority on nodes without
  `SYNC_TOKEN`
- community operators could keep depending on `ADMIN_TOKEN` for replication
- a leaked admin token could still be used to register peers or pull the sync
  log

At this point the network already has dedicated sync authentication, signed
peer registration, and authenticated sync reads. Keeping the fallback is now
more risky than useful.

## Decision
All sync endpoints require `X-Sync-Token` and never accept `X-Admin-Token`.

This applies to:

- `GET /sync/events`
- `GET /sync/peers`
- `POST /sync/register`
- `POST /sync/reconcile`

If `SYNC_TOKEN` is unset on a node, those endpoints return 403.

Community nodes use only `SYNC_TOKEN` for:

- startup registration
- fallback polling
- reconcile

This ADR narrows ADR 0027 by removing the admin-token compatibility branch.

## Consequences
- Sync replication now has a dedicated credential boundary.
- Leaked admin credentials no longer grant sync replication access.
- Operators must set `SYNC_TOKEN` on both primary and community nodes for
  replication to work.
- Old deployments that relied on `ADMIN_TOKEN` for sync must update their
  configuration before upgrading.

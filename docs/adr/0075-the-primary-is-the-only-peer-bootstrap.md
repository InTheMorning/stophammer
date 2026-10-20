# ADR 0075: The Primary Is The Only Peer Bootstrap

## Status
Accepted on 2026-10-20. Supersedes
[ADR 0008](archive/0008-cloudflare-tracker-implementation.md). It also
supersedes the sections "Cloudflare tracker demoted to optional bootstrap" and
"Long-term" of [ADR 0016](0016-push-gossip-tracker-elimination.md), and the
tracker registration of [ADR 0009](0009-community-node-mode.md).

## Date
2026-10-20

## Context

ADR 0008 decides how a Cloudflare Workers tracker is built in
`stophammer-tracker/`. ADR 0016 made the primary the tracker through
`POST /sync/register` and `GET /sync/peers`, and it kept the Cloudflare
tracker as an optional bootstrap. ADR 0027 later put `GET /sync/peers` behind
the sync token.

On 2026-10-20, these facts were true:

- No `stophammer-tracker/` directory is in this repository or in its git
  history.
- The default `TRACKER_URL`, `https://stophammer-tracker.workers.dev`, has no
  DNS record.
- At each start, a community node sent `POST {TRACKER_URL}/nodes/register`
  (`register_with_tracker` in `src/community.rs`). The request failed, and the
  node logged a warning and continued.
- `docker-compose.e2e.yml` set `TRACKER_URL` to the primary. The primary has
  no `/nodes/register` route, so that request also failed.

Thus each community node sent a request that could not succeed, and two
documents described a component that does not exist.

## Decision

1. The primary is the only peer bootstrap. A community node registers its
   push URL with the primary through `POST /sync/register` (ADR 0016, ADR
   0026). A node finds its peers through `GET /sync/peers` on the primary (ADR
   0016, ADR 0027).
2. A community node sends no request to a tracker. `CommunityConfig` has no
   `tracker_url`, and the node does not read `TRACKER_URL`.
3. A node discovery service outside the primary needs a new ADR.

## Consequences

- A community node needs `PRIMARY_URL` and `NODE_ADDRESS`, as before. It needs
  no other address to start.
- An operator who sets `TRACKER_URL` sees no change in behavior. The release
  notes say that the variable is removed.
- A client that cannot reach the primary cannot find a community node. This
  was also true with the tracker, because the tracker did not run.

## Invariants

- A community node sends a registration only to the primary.

## Guards

No test. The rule has not broken since the removal. The compiler rejects a
`CommunityConfig` with a `tracker_url` field.

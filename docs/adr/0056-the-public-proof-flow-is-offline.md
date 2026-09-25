# ADR 0056: The Public Proof Flow Is Offline

## Status
Accepted

Supersedes [ADR 0018](0018-proof-of-possession-mutations.md).

## Date
2026-09-25

## Context
ADR 0018 lets a publisher prove control of a feed. The publisher asks for a
challenge, puts it in a channel-level `podcast:txt`, and asserts it. The node
then issues a bearer token that permits `DELETE` and `PATCH` on that feed and
its tracks. [The history research](../reviews/adr-0018-proof-flow-history-research.md)
traces the flow from 2026-03-14 to now. It found these facts:

1. The extractor accepts a `podcast:txt` in an item, or at any depth. Both
   versions of the extractor did this, and no test covers it. ADR 0018 says
   "at channel level".
2. Each stored design for challenge admission lets a stranger with no
   credential stop a publisher. A limit for each feed lets the stranger fill
   the slots. The replacement rule of commit `73dffce` lets the stranger cancel
   the challenge of the publisher. The global limit of 5,000 lets the stranger
   block every feed on the node for 24 hours.
3. The node checks the challenge expiry before the RSS fetch, not when it
   issues the token.
4. A change of the feed URL from A to B and back to A escapes the URL check at
   token issue.
5. The replacement rule never reached ADR 0018. Two reports in
   `docs/security/` describe code that no longer exists.
6. The production backup of 2026-09-24 20:01 UTC holds no challenge, no token
   and no `feed_retired` event. No evidence shows a publisher who uses the
   flow.

The flow is also the only path where a public request makes the primary fetch
a URL.

The research compares six options. Only three stop both the cancellation and
the exhaustion: signed challenges with no stored state, a publisher key in the
RSS, and this decision.

## Decision

### 1. The public routes are removed

`POST /v1/proofs/challenge` and `POST /v1/proofs/assert` are removed from the
router and from the OpenAPI document. A request to either one answers `404`.

### 2. The write routes take the admin token only

`DELETE /v1/feeds/{guid}`, `PATCH /v1/feeds/{guid}`,
`DELETE /v1/feeds/{guid}/tracks/{track_guid}` and `PATCH /v1/tracks/{guid}`
need `X-Admin-Token`. A request with only `Authorization: Bearer` answers `403`.
The routes stop sending `WWW-Authenticate`.

### 3. The code is deleted, and the tables stay one release

The challenge and token functions of `src/proof.rs`, the pruner, and the bearer
path of `check_admin_or_bearer_with_conn` are deleted. No code reads or writes
the tables `proof_challenges` and `proof_tokens` after this change.

The tables stay for one release. The binary before this change deletes rows
from them when it retires a feed. If the tables were gone, a rollback would
break that delete. A later migration drops the tables after the deploy is
stable. The production tables are empty.
The trigger `trg_feeds_cleanup_before_delete` still deletes the rows of a
deleted feed from the two tables. That migration also changes the trigger.

Each test of the deleted behavior is deleted in the same change. A test of the
admin path stays.

### 4. The fetch guard stays and gets its own module

The SSRF guard in `src/proof.rs` serves sync registration, and ADR 0054 needs
it. It moves to a module whose name says what it does: `src/fetch_guard.rs`.
Its functions and their tests do not change.

### 5. A publisher asks the operator

A publisher who needs a feed removed, moved or corrected, and cannot do it
through the feed, asks the operator. The operator examines the evidence and
records it in the reason of the change.

Through the feed itself, a publisher can still change metadata and payment
routes, add and remove tracks, and send a podping. ADR 0052 adds moves and GUID
changes through the feed.

### 6. The other decisions change with this one

- ADR 0051 section 1: the source URL changes by an operator relocation or by a
  move under ADR 0052.
- ADR 0052: the publisher relocation trigger and the publisher proof for a GUID
  change are removed. A move no longer revokes proof tokens.
- ADR 0053 section 1: a publisher does not retire a feed.
- ADR 0054 and ADR 0055: the proof fetch no longer exists.

## Alternatives Considered

The research record gives each one. In short:

### Keep the replacement rule
It keeps cancellation and the global exhaustion. Rejected.

### A limit for each feed with a short expiry, or replacement for the same requester only
Each one keeps one of the two attacks. Rejected.

### Signed challenges with no stored state
It stops both attacks. It needs a revision number on each feed and a new
design, for a flow that no evidence shows in use. Rejected for now.

### A publisher key in the RSS
It stops both attacks and gives a continuing identity for recovery. It needs a
new ADR, a key rule and publisher tools. It stays open as a future ADR.

## Consequences

- No public request can stop a publisher, because no public flow exists.
- No public request makes the primary fetch a URL.
- A publisher loses self-service removal and relocation. The operator does
  that work. A later ADR can return self-service removal through
  `podcast:block`, as the
  [removal research](../reviews/feed-removal-and-wavlake-forms-research.md)
  describes.
- The four defects of the context stop mattering, because the code is gone.
- Two empty tables stay for one release. A later migration drops them.

## Invariants

- No route issues a bearer token.
- Each write route needs `X-Admin-Token`.

## Non-Goals

- A replacement for publisher self-service. Separate ADRs own `podcast:block`
  and a publisher key.
- A change to the SSRF guard.

## Guards

The cancellation and the item-level proof were reproduced on 2026-09-24.

- A test sends `POST /v1/proofs/challenge` and `POST /v1/proofs/assert`, and
  each answers `404`.
- A test sends each write route with only a bearer header, and each answers
  `403`.

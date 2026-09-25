# ADR 0053: A Correction Stays Applied

## Status
Proposed

## Date
2026-09-24

## Context
An operator corrects the index in three ways today. None of them lasts.

**Removal.** `DELETE /v1/feeds/{guid}` deletes the rows of a feed and emits a
signed `FeedRetired` event. Community nodes apply it (`src/apply.rs:185`).
The node records no block. The next podping, publisher link or import adds
the feed again.

**Block.** `FeedBlocklistVerifier` reads `BLOCKED_FEED_GUIDS` and
`BLOCKED_FEED_URLS` from the environment
(`src/verifiers/feed_blocklist.rs`). A change needs a restart of the primary.
The list is not replicated and makes no signed record. A community node does
not know why a feed is gone.

**Stale content.** `upsert_feed` replaces each field with no ordering
condition (`src/db.rs:1320`). An old copy of a feed replaces a newer copy.
The old copy can come from a CDN cache, from a host that still serves the
feed after a move, or from an NDJSON replay. The newer tracks and payment
routes stay lost until the next fresh crawl.

A fourth gap concerns payment routes. The signed event log holds each
change to the routes. No API shows a client or an operator when the recipients
of a feed changed. A change of recipients is the largest effect of an error or
an attack.

ADR 0043 defines `lastBuildDate` as the time the feed file was generated, not
a release date. That is the correct clock for the order of two copies of one
feed.

## Decision

### 1. A block is a signed, replicated fact

The primary keeps a `feed_blocks` table. A row blocks one GUID or one exact
URL, with a reason and a time. Two new signed events change the table:
`FeedBlocked` and `FeedUnblocked`. Community nodes apply them.

The primary checks the table after authentication and before the verifier
chain. A block rejects a submission when the declared GUID, `source_url`,
`canonical_url` or a redirect hop (ADR 0052) matches a row. The reason is
`blocked`. The crawler does not retry it.

Admin routes create and delete a block:

- `POST /v1/blocks` with the admin token.
- `DELETE /v1/blocks/{id}` with the admin token.

`DELETE /v1/feeds/{guid}` blocks the GUID and the source URL in the same
transaction as the retirement. The query `?block=false` retires with no
block. That serves an operator who wants the next crawl to admit the feed
again.

A publisher with ADR 0018 proof can retire its own feed. That retirement
also blocks. Only the operator removes a block.

### 2. The environment blocklist becomes a seed

At startup, the primary reads `BLOCKED_FEED_GUIDS` and `BLOCKED_FEED_URLS`.
It adds a block for each value that the table does not hold, and it emits
`FeedBlocked` for each one. The primary never removes a block because a value
left the environment. `feed_blocklist` leaves the configurable chain.

### 3. An older copy does not replace a newer copy

The node rejects the content of a submission for a held record when both of
these are true:

- The submission and the record both have `last_build_date`.
- The submitted `last_build_date` is earlier than the stored value.

The reason is `stale_submission`. `force_reingest` does not skip this rule.
An equal date passes, so a corrective pass that sends the same body again
still works.

A feed without `lastBuildDate` gets no protection from this rule. The node
does not use `pubDate` or the newest item date. A publisher can remove an
item or change a date for a correct reason.

A publisher whose tool clock went back gets a rejection until its date passes
the stored date. The operator can clear the stored date with a retire and
`?block=false`.

### 4. A change of payment recipients is visible

The primary records each change of the recipient set of a feed or a track. A
recipient set is the list of pairs of address and split, in order. A change
of name or of `fee` only is not a change of recipients.

- `GET /v1/feeds/{guid}/route-history` returns the changes for the feed and
  its tracks: the time, the old set, the new set and the event ID. Each entry
  comes from the signed events, so a community node gives the same answer.
- The primary logs each change at `warn` level with the feed GUID.
- The node does not hold or delay a change. A hold needs rules for size and
  time that no evidence supports yet.

## Alternatives Considered

### Keep the environment blocklist
It needs a restart, it does not replicate, and it leaves no signed record.
Rejected.

### Order copies by the newest item date
A publisher can remove its newest item for a correct reason. The feed then
looks older. Rejected.

### Hold a large change of payment recipients for review
This needs an operator for each hold, and a rule for "large" with no data
behind it. Rejected for now. The history of section 4 gives the data for a
later decision.

## Consequences

- A removed feed stays removed on the primary and on each community node.
- An operator can block with no restart.
- A stale CDN copy or an old host no longer replaces a newer copy, when the
  feed has `lastBuildDate`.
- A client can show when the recipients of a feed changed, and can compare
  them with the previous set.
- Two new event types need the community-node rollout that ADR 0052 section 6
  describes.

## Invariants

- A block removes nothing by itself. A retirement removes rows.
- Only an operator removes a block.
- A submission never replaces content that has a later `last_build_date`.

## Guards

Each guard follows an observed gap: the removed feed that the next crawl adds
again, and the stale body that replaces a newer body.

- A test retires a feed and then submits it again. The response is `blocked`,
  and no row returns.
- A test replays `FeedBlocked` on a community node, and then sends a
  submission there through the primary. The result is the same on both nodes.
- A test submits an older `last_build_date` with `force_reingest`. The
  response is `stale_submission`, and the routes do not change.

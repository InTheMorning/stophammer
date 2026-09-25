# ADR 0057: A Feed Can Block This Index

## Status
Accepted on 2026-09-25

## Date
2026-09-25

## Context
ADR 0056 removed the public proof flow. A publisher can still change the
content of a feed through the feed, but no publisher can remove a feed from
the index. The operator must do it. The node keeps a record when its source
URL answers `404`, and ADR 0053 keeps a removed feed out of the index.

The Podcasting 2.0 namespace defines `podcast:block` for this purpose. The
specification says:

> Platforms should not ingest a feed for public display/use if their slug
> exists in the `id` of a `yes` block tag, or if an unbounded `yes` block tag
> exists.

The tag is in the channel, and a feed can hold more than one. The value is
`yes` or `no`. The optional `id` holds one slug from the service slug list. A
platform examines the tags in this sequence:

1. Its own slug with `no`: the platform can ingest the feed.
2. Its own slug with `yes`: the platform does not ingest the feed.
3. An unbounded `yes`: the platform does not ingest the feed.

The slug list has `podcastindex`. It has no slug for this index.

Podcast Index has no self-service removal. Its maintainers remove feeds by
hand, and a discussion of January 2026 says that this work is too large for
them. Thus no tested design exists to copy, and the namespace tag is the one
signal that the source gives.

In the April snapshot of 7,538 feeds, no feed had a `yes` block, 154 feeds had
an unbounded `no`, and no feed had `itunes:block`. No crate reads either tag
as a typed field. The evidence and the sources are in
[the removal research](../reviews/feed-removal-and-wavlake-forms-research.md).

## Decision

### 1. The slug of this index

This index answers to the slug `musicindex`. The operator asks the namespace
repository to add it to the service slug list. Until the list has it, the
index still honors the slug, because a tag with that `id` can only mean this
index.

The index does not answer to `podcastindex`. A publisher who blocks Podcast
Index has not blocked this index.

### 2. The rule

For each submission, the node evaluates the channel-level `podcast:block` tags
in the sequence of the specification:

1. A tag with `id="musicindex"` and the value `no`: the feed is not blocked.
   The next steps do not run.
2. A tag with `id="musicindex"` and the value `yes`: the feed is blocked.
3. A tag with no `id` and the value `yes`: the feed is blocked.
4. Otherwise the feed is not blocked.

A tag with a different slug has no effect. An unbounded `no` has no effect.
The node compares the value after trim, with no case sensitivity. A value
other than `yes` or `no` has no effect. A tag inside an item has no effect,
because the specification puts the tag in the channel.

`itunes:block` has no effect. It asks Apple not to list a show, and a
publisher can set it to leave Apple only.

### 3. What a blocked feed does

The node applies the rule after the classification of ADR 0051, so only the
source URL decides:

| Case of ADR 0051 | Blocked | Result |
|---|---|---|
| Update | Yes | Retire the record with `FeedRetired`, reason `podcast_block`. Write no row in `feed_blocks` |
| New feed | Yes | Write nothing. Reason `source_blocked` |
| Mirror, record conflict, GUID change | Any | The result of ADR 0051, unchanged |
| Update or new feed | No | The normal ingest |

The retirement uses the transaction and the signed event of ADR 0053 task 003,
with no block. Community nodes apply the retirement.

The response of a retirement is `accepted: false` with the reason
`source_blocked`, and the ID of the `FeedRetired` event in `events_emitted`.
The crawler reads it as a final rejection. It does not follow the links of a
rejected feed (`stophammer-crawler/src/modes/batch.rs:191`).

### 4. The publisher can reverse it

The node writes no durable block. When the publisher removes the tag, or sets
`id="musicindex"` to `no`, the next crawl admits the feed as a new feed. The
tracks get the same feed-scoped identities as before, because ADR 0040 makes
them from the feed GUID and the item GUID.

An operator block of ADR 0053 is different. It stays until the operator
removes it, and the node checks it before this rule.

### 5. The contract

- The parser reads each channel-level `podcast:block` into a typed list of
  `{ id, value }`, with the raw values. The field is additive.
- The ingest request carries that list. The field is optional, so an older
  crawler sends no list, and the node applies no block rule for it.
- `docs/API.md` and the OpenAPI document give the reason `source_blocked`.

## Alternatives Considered

### Hide the feed and keep the record
The API would filter a blocked feed from each read. Each query route would
need the filter, and a missed route would show the feed. The retirement
already exists, replicates, and removes the feed from each route. Rejected.

### Write a durable block on the retirement
The publisher could not reverse the removal through the feed. Only the
operator could. Rejected.

### Honor `itunes:block`
A publisher can set it to leave Apple only. Rejected.

### Honor the `podcastindex` slug
This index is not Podcast Index. A publisher who blocks Podcast Index gives no
statement about this index. Rejected.

### Wait for two fetches before a retirement
A tag is an explicit statement of the source, not a tool error such as a
changed GUID. A mistaken tag costs one retirement, and the publisher reverses
it by removing the tag. Rejected.

## Consequences

- A publisher can remove a feed without the operator, and can bring it back.
- A retirement deletes the tracks, the routes and the source facts of the
  record. A return recreates them from the feed.
- A community node shows the removal and the return through the signed events.
- The parser, the ingest contract and the node change. Each repository gets
  one commit that names this ADR.

## Invariants

- Only the body of the source URL can retire a record through this rule.
- This rule writes no row in `feed_blocks`.
- An operator block of ADR 0053 is checked before this rule.

## Non-Goals

- A block for one track.
- A durable publisher block.
- The registration of the slug. That is an operator step outside the code.

## Guards

No feed in the index uses the tag today, so no incident exists. The guards
protect the direction of each rule, because a wrong direction removes a feed or
keeps one that asked to leave:

- An unbounded `yes` from the source URL retires the record.
- `id="musicindex"` with `no` beside an unbounded `yes` admits the feed.
- `id="podcastindex"` with `yes` admits the feed.
- A `yes` tag in a mirror body changes nothing.
- A feed that removes its tag is admitted on the next crawl.

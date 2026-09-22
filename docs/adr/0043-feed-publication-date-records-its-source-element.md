# ADR 0043: A Feed Publication Date Records Its Source Element

## Status
Accepted

Amended 2026-09-22: the API also returns `last_build_date` on the feed.
The operator decided this when the work started. An added field stays inside
`v1` by ADR 0044. The amendment adds a field and reverses no decision.

## Date
2026-09-22

## Context
The v4vmm desktop client reported that a feed build time can become a release
date. Stophammer checked the statement and found it correct. The effect is much
larger than the client estimated. The evidence is in
[the verification record](../reviews/v4vmm-musicindex-api-change-request-verification.md).

`stophammer-parser/src/profile.rs:184` reads `lastBuildDate` into
`FeedField::PubDate`. The rule engine at `src/engine.rs:98` keeps the first
value, so `lastBuildDate` applies only when `pubDate` is missing or unparseable.

`src/api.rs:2092` computes `feed_data.pub_date.or(oldest_item_at)`. Line 2101
labels the resulting `release_date` claim `feed.pub_date` when `pub_date`
holds a value. The label does not record that `lastBuildDate` supplied it.

`lastBuildDate` is the time the feed file was generated. It is not a
publication date. A generator can write it on each fetch.

The v4vmm client recorded the number of affected feeds as unknown. A
measurement over the raw XML of 7,538 feeds gives that number:

| Channel elements | Feeds |
|---|---:|
| `pubDate` in the channel | 228 |
| `lastBuildDate` in the channel and no `pubDate` | 7,095 |
| No `pubDate` and no `lastBuildDate` | 215 |

For 7,095 feeds, which is 94 percent of the snapshot, `lastBuildDate` supplies
the feed publication date. The median difference between `lastBuildDate` and
the newest item `pubDate` is 459 days. 4,183 feeds show a difference of more
than 365 days.

The live service shows the same behavior. For 47 of 50 recent feeds,
`release_date` is after `newest_item_at`.

The `feeds.release_date` value is thus a feed build time for most of the
index, and the claim label names an element the parser did not read.

## Decision
A claim extraction path names the element that the parser read. A feed build
time is not a release date.

1. The parser stops writing `lastBuildDate` into `FeedField::PubDate`. It
   writes the value into a new field `FeedField::LastBuildDate`.
2. The ingest layer computes `release_date` from the feed `pubDate` first, and
   from the oldest item `pubDate` second. It does not use `lastBuildDate`.
3. The path labels `feed.pub_date` and `oldest_item.pub_date` then name the
   element that supplied the value.
4. The ingest layer records a second claim with the path
   `feed.last_build_date` when the feed holds a `lastBuildDate` element.
   Stophammer keeps that evidence and does not use it as a release date.
5. `FeedResponse` returns `last_build_date` so a client can show feed
   freshness. The field is the feed build time. A client must not present it
   as a release date.

Reingest is incremental. A parser change corrects a feed when the crawler or
the podping listener reads that feed again. `ContentHashVerifier` stops an
unchanged feed, so a feed with unchanged content needs `force_reingest`.
That forced pass can run as a background trickle. No client needs one
synchronized pass over all feeds.

## Alternatives Considered

### Keep the fallback and correct the label

This is the first change that the v4vmm document gives. It labels the claim
`feed.last_build_date` and keeps the value in `release_date`. The provenance
becomes correct and the release date stays incorrect for 94 percent of feeds.
Rejected.

### Drop the fallback and record no publication claim

This is the second change that the v4vmm document gives. It discards a
recorded source value. The Provenance First mandate in `AGENTS.md` keeps the
source layer. Rejected.

### Keep the current behavior and document it

A client cannot repair the value, because the response does not say which
element supplied it. Rejected.

## Consequences

- About 7,095 feeds change their `release_date` from a feed build time to the
  date of their oldest item. That value is a publication date.
- The `feed.pub_date` claim count falls to about 228. The
  `oldest_item.pub_date` claim count rises by about 7,095.
- A new claim path `feed.last_build_date` appears. It holds evidence that
  Stophammer did not keep before.
- `FeedResponse` gains `last_build_date`. This is an added field, so it needs
  no new path version.
- A client that sorted by `release_date` sees a large change in sequence. The
  new sequence shows publication. The sequence before this change showed feed
  generation.
- The parser gains one field. This is an additive change to
  `stophammer-parser`.

## Invariants

- A claim extraction path names the element that the parser read.
- A feed build time is not a release date.
- Stophammer keeps `lastBuildDate` as source evidence.

## Guards

This rule broke in service and the defect reached 94 percent of the index. It
earns a test.

- A feed with `lastBuildDate` and no `pubDate` produces a `release_date` from
  its oldest item, and a claim with the path `oldest_item.pub_date`.
- The same feed produces a claim with the path `feed.last_build_date`.
- A feed with `pubDate` and `lastBuildDate` produces a claim with the path
  `feed.pub_date`, and the value comes from `pubDate`.

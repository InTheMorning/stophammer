# ADR 0074: A Feed Without A GUID Is Invalid

## Status
Proposed on 2026-10-20. The operator decided the rule of §1 on 2026-10-20. §4
waits for the count of the affected feeds. It supersedes the section
"`podcast:guid` absent → empty string" of ADR 0011.

## Date
2026-10-20

## Context

ADR 0011 says that the crawler sets the feed GUID to an empty string when a
feed has no `podcast:guid`. The node then rejects the feed. The code
does not do this.

On stophammer-crawler `b142c79` and stophammer-parser `9021262`:

- The parser gives an error for a feed with no `podcast:guid`, unless the caller
  gives a fallback GUID. `profile::stophammer_with_fallback` and
  `FeedParserBuilder::fallback_guid` supply the fallback. The parser CLI
  `stophammer-parse` accepts one too.
- `parse_feed_xml` in `src/crawl.rs` turns that error into an ingest with
  `feed_data: null`. A body with no `<channel>` or no `<title>` takes the same
  path. Only malformed XML is a parse error in the crawler.
- The `feed`, `refresh` and `gossip` modes give no fallback.
- The `import` mode gives the `podcastGuid` value of the PodcastIndex snapshot
  as the fallback (`src/modes/import.rs`).
- The `ndjson` mode gives the `feed_guid` of the export row as the fallback
  (`src/modes/ndjson.rs`).

Thus a feed with no `podcast:guid` enters the index through `import` or
`ndjson`, with a GUID that its source does not state. A `refresh` pass then
cannot change that record, because the feed has no GUID without the fallback.

### Why the crawler does this

Both behaviors come from commit `c02bbae` of 2026-03-12, which rewrote the
crawler in Rust. No ADR states a reason for them.

- The comment on the null ingest says that it lets "the server record the
  crawl attempt". The node does not do this. A test with the production
  `VERIFIER_CHAIN` on 2026-10-20 gave this answer to a `200` ingest with
  `feed_data: null`:

  ```text
  {"accepted":false,"no_change":false,"reason":"[medium_music] podcast:medium absent — must be 'music', 'publisher', or 'musicL'"}
  ```

  The node writes no row for a rejected ingest. It writes `feed_crawl_cache`
  only after an accepted ingest.
- The fallback of the `import` mode makes a seed corpus from the PodcastIndex
  snapshot. ADR 0013, now archived, describes the snapshot as "a seed set of
  feed URLs and PodcastIndex GUIDs".

### What the crawler records from these answers

Two stores decide which feeds a mode skips. The shared skip list
`feed_skip.db` is read by `gossip` and `import`. The import memory
`import_feed_memory` of ADR 0030 is read by `import`. Each store keeps
`raw_medium` only from a body that parsed.

The false node answer is a medium-gate rejection. Both stores skip a feed with
a medium-gate rejection with no time limit. Thus today:

| Body | `feed`, `refresh`, `gossip` | `import`, with the fallback |
|---|---|---|
| No `podcast:guid`, medium `podcast` | False reason "medium absent". No `raw_medium`. Skipped with no time limit | Real reason "medium is 'podcast'". `raw_medium` kept. Skipped with no time limit |
| No `podcast:guid`, no medium | Reason "medium absent". Skipped with no time limit | The same |
| No `podcast:guid`, medium `music` | False reason "medium absent". Skipped with no time limit, also after the feed adds a GUID | Accepted with the PodcastIndex GUID |

A removal of the fallback with no other change gives each mode the left column.
The import then loses `raw_medium` for each feed with no GUID, and records a
music feed as a feed with no medium.

`FeedGuidVerifier` on the node rejects a GUID that is not a UUID. It cannot
tell a GUID from the source from a fallback GUID.

The number of indexed feeds whose source has no `podcast:guid` is not measured.
The fetch cache of the crawler holds the last body of each fetched feed (ADR
0050 §1), so a count from it is possible.

## Decision

### 1. A feed is valid only with a `podcast:guid`

A feed body is valid only when its channel has a non-empty `podcast:guid`. The
index does not accept a feed body without it. No other value replaces it: not
the PodcastIndex GUID, not a stored GUID, and not a GUID made from the URL.

### 2. The parser has no fallback GUID

The parser removes `fallback_guid`, `profile::stophammer_with_fallback` and the
fallback argument of `stophammer-parse`. A feed with no `podcast:guid` is a
parse error with a reason that names the missing element.

### 3. Each crawler mode applies the same rule

`crawl_feed_report`, `parse_feed_xml` and the `ndjson` ingest lose their
fallback argument. The `import` mode uses the PodcastIndex GUID only to select
and record the rows of the snapshot. It does not send that GUID as feed data.

### 3a. A body that does not parse is a parse error, with its medium

A body with no `<channel>`, no `<title>` or no `podcast:guid` is a crawler
parse error, as malformed XML is. The crawler sends no ingest for it. The
reason names the missing element.

The parser error for a missing `<title>` or `podcast:guid` gives the
`raw_medium` of the channel. The crawl report carries that `raw_medium`, so
each store records it.

### 3b. Each store keeps the skip rules of today

After the change, each mode records this:

| Body | Recorded outcome | Skip |
|---|---|---|
| No `podcast:guid` or no `<title>`, medium not `music`, `publisher` or `musicL` | Parse error, with `raw_medium` | No time limit: known non-music medium |
| No `podcast:guid` or no `<title>`, no medium | Parse error, with no `raw_medium` | No time limit: no medium |
| No `podcast:guid` or no `<title>`, medium `music`, `publisher` or `musicL` | Parse error, with `raw_medium` | 7 days, as each parse error (ADR 0030) |
| A `podcast:guid` | As today | As today |

A feed that adds its GUID is fetched again after at most 7 days. The skip of
a known accepted feed (`--skip-known-success`) does not change. The import
audit does not change, because it records only accepted rows.

The rule that skips a feed with no medium belongs to the medium gate of the
node. The crawler applies it only to choose a skip. The node keeps its
verifier chain, and `FeedGuidVerifier` stays.

### 4. A record that has no source GUID

Not decided. This waits for the count. The two options are:

- Keep the record. It does not change, because no crawl can update it.
- Retire the record on its next crawl, with a reason such as `no_podcast_guid`.
  ADR 0057 and ADR 0067 retire a feed with the same procedure.

## Alternatives Considered

### Keep the fallback for `import` and `ndjson`

This keeps each feed that PodcastIndex knows. But the index then holds a GUID
that the source does not state, and only two of the five modes can change the
record. That does not agree with ADR 0051: only content from the source URL
changes a record.

### Make a GUID from the feed URL

The specification of `podcast:guid` gives a UUIDv5 of the feed URL as the
value that a publisher must write. A GUID that the index computes is not a fact
of the source, and it changes when the feed moves.

## Consequences

- Each mode accepts the same feed bodies.
- The `import` mode no longer adds a feed whose source has no `podcast:guid`.
- Each GUID in a new record is a fact of the source.

## Invariants

- No mode sends feed data with a GUID that the source body does not state.
- A body that does not parse sends no ingest.
- Each store records the `raw_medium` of a body with a `<channel>`.

## Guards

- A parser test: a feed with no `podcast:guid` is a parse error, and the error
  gives `raw_medium`. The test of the fallback in
  `stophammer-parser/tests/basic.rs` is deleted.
- A crawler test for each row of the §3b table: the outcome, the
  `raw_medium` in the report, and the skip of `feed_skip.db` and of
  `import_feed_memory`.
- A crawler test: an `import` row with a PodcastIndex GUID and a body with no
  `podcast:guid` sends no ingest.
- A crawler test: the same for an `ndjson` row.

# Review: Requests Received After Release 0.13.2

## Status

Review of 2026-10-23. This document is advisory. It binds nothing. Each
proposal below needs a decision from the operator, and each accepted proposal
needs its own ADR.

The review covers the commits from `v0.13.2` to `9ce7e16`. It examined the
request documents, the source at `9ce7e16`, and the ADRs that each request
names. It did not run Stophammer, and it did not send a request to the live
API.

## Summary

Six requests arrived after release 0.13.2 of 2026-10-21.

| Request | Source | State |
|---|---|---|
| musicindex request 19 | [musicindex requests](../plans/musicindex-open-requests.md) | Answered. Refilled on 2026-10-21. ADR 0077 records the rule |
| musicindex request 20 | [musicindex requests](../plans/musicindex-open-requests.md) | Answered. Copies recorded on 2026-10-22 |
| musicindex request 17 | [musicindex requests](../plans/musicindex-open-requests.md) | Open. No answer |
| musicindex request 18 | [musicindex requests](../plans/musicindex-open-requests.md) | Open. No answer |
| v4vmm requests 10 and 11 | [v4vmm requests](../plans/v4vmm-open-requests.md) | Drafts. The v4vmm operator reviews them first |
| Research request of 2026-10-23 | [search index request](../plans/research-search-index-request.md) | Received. No decision |

The research addendum of 2026-10-22, in
[the compression and split request](../plans/research-crawler-compression-and-value-time-split-request.md),
asks for no change.

## Answered Requests

### musicindex Request 19: The Splits Of An Unchanged Feed

The parser of ADR 0073 did not read a feed body that did not change. The feed
server answered `304`, or the `content_hash` verifier gave `NO_CHANGE`. Two
forced `feed` passes read four feeds again. The live API gives each of the 38
splits. ADR 0077 tells each future ADR of this type to state its refill pass.

One item stays open. The search for `valueTimeSplit` covered only the crawler
fetch cache. A feed that only an `ndjson` replay gave to the index is not in
that cache. A forced `refresh` pass of all feeds reads such a feed, but that
pass has not run. ADR 0077 "Consequences" records this. The "Open Items" section of
`AGENTS.md` does not name it.

### musicindex Request 20: No Copy Of Two Feeds

A `feed` pass of the three album URLs on 2026-10-22 recorded each one as a
copy. No crawler reads these album files again. Thus a subsequent change to them
makes no event. This is a limit of ADR 0058. A change to it needs a new
decision.

## Open Requests Of musicindex.org

### Request 17: The Image Of The Indexed Feed On Each Copies Row And Live Row

The request adds `feed_image_url` to each row of `GET /v1/copies`. It adds
`feed_title` and `feed_image_url` to each row of `GET /v1/live-items`. Without
these fields, the search site sends 20 more requests for a page of 20 rows. The
request follows the rule of ADR 0059 §1.

### Request 18: Summary Fields On Each Remote Value Time Split

The request adds the summary fields of ADR 0059 §1 and §5 to each remote split
row. The names are the names that a `remote_items` entry uses. Without them, a
track page sends at most 12 more requests.

## Open Requests Of v4vmm

Both requests are drafts. The v4vmm operator reviews them before Stophammer
answers.

### Request 10: Search Accepts Words, Phrases And Field Filters

The request of 2026-10-23 replaces the draft of 2026-10-21. It asks for these
items:

1. The text of `q` is words and phrases only, so that each text gives `200`.
2. Filter parameters: `artist`, `title`, `release`, `kind`, `after`, `before`,
   `min_duration` and `max_duration`.
3. An exclusion for the words, the phrases and each text filter.
4. A `400` with a stable error code for a filter that a type cannot apply.
5. A search with filters and no `q`.
6. The filters in the contract.

Item 1 corrects a defect. Items 2 to 6 add a large API surface.

### Request 11: A Feed Search Hit Gives Its Release Date And Its Named Feeds

The request adds `newest_item_at` to each `type=feed` hit. It adds
`include=named_feeds` to `GET /v1/search`. v4vmm states that only its saved
searches need these facts.

## Research Request Of 2026-10-23: The Search Index

### The Four Defects

This review examined each defect against the source at `9ce7e16`.

| # | Defect | Result of this review |
|---|---|---|
| S1 | The words from before a change of text stay in the index | Confirmed. `populate_search_index` sends the new text in the FTS5 `'delete'` command (`src/search.rs:139-145`). The index is contentless (`content=''` in `src/schema.sql`). A delete needs the text that the index holds, and no table keeps that text (`src/db.rs:832-835`) |
| S2 | A retired publisher feed stays in the index with a null title | Not examined. The research gives it as an inference |
| S3 | The internal columns match words | Confirmed. `entity_type` and `entity_id` are not `UNINDEXED`, and `tags` holds `raw_medium` (`src/schema.sql`, `src/db.rs:1395-1404`) |
| S4 | Each publisher hit has a quality score of 0 | Probable. `src/db.rs:1419` stores the score with the type `feed`. The search joins `entity_quality` on the type of the hit (`src/search.rs:337-338`), and that type can be `publisher` |

The research examined S2 and S4 again after this review. The
[addendum](#addendum-research-checks-of-s2-and-s4) gives the results.

The research proposes the FTS5 option `contentless_delete=1` for S1. The
option needs SQLite 3.43 or a subsequent version. `Cargo.lock` gives
`libsqlite3-sys` 0.28.0, and the research gives its bundled SQLite as 3.45.0.

The correction needs a new index and one rebuild. The rebuild of 2026-09-26
took 3.7 s. The same rebuild can also correct S2 and S3. ADR 0022 owns the
index design.

### The Other Changes

The request gives these changes with their priority:

- Priority 1: each word of `q` as an FTS5 phrase. This is v4vmm request 10
  item 1.
- Priority 1: `newest_item_at` on a feed hit. This is v4vmm request 11 item 1.
- Priority 2: the `effective_rank` in the response, or a contract statement
  that the row order is the rank order.
- Priority 2: a prefix index and a prefix match on the last word of `q`.
- Priority 3: one request for all types, an `ETag` and a short
  `Cache-Control`, one summary query for each page, and an index on
  `tracks.publisher`.
- Priority 4: a search time test on a copy of the production database, and a
  log of the query length in place of the query text.

### Shared Client Code

The request proposes a small crate with the response types and no HTTP code.
This needs decisions in Stophammer, v4vmm and musicindex-studio. v4vmm has no
license, and Stophammer is AGPL-3.0-only. The operator selects the license
first.

## The Research Addendum Of 2026-10-22

The addendum asks for no change. It gives evidence that ADR 0072 §5 holds on
24 hosts. It also gives evidence that the parser keeps each split that ADR 0073
§1 permits.

It asks one question. Is a show with `podcast:medium` `podcast` a source of
facts about indexed tracks? One such show names 1,738 indexed tracks in its
splits. `MediumMusicVerifier` (ADR 0005) rejects that show.

## Proposals

These proposals are advisory. The operator decides each one.

### P1. One ADR For The Search Index

Write one ADR that corrects S1 to S4 and treats each word of `q` as an FTS5
phrase. S1 and S3 are confirmed defects that give incorrect results. The
correction needs one rebuild. The phrase rule corrects the `400` of an
apostrophe before the v4vmm operator reviews request 10. The ADR supersedes or
amends ADR 0022.

### P2. Accept musicindex Requests 17 And 18

Both requests apply the rule of ADR 0059 §1 to more rows. Neither request
needs a new rule. An answer in the musicindex requests can cite ADR 0059. Each
new field follows the steps for an API change in `AGENTS.md`.

### P3. Wait For The v4vmm Review Of Requests 10 And 11

The v4vmm operator reviews the two drafts first. P1 covers request 10 item 1.
The filters of request 10 need their own ADR after the review. Request 11 item
1 is also priority 1 in the research request, so an answer can give it with
P1 or after it.

### P4. Decide On The Forced Refresh Pass Of ADR 0077

Select one of these:

- Run a forced `refresh` pass of all feeds, and check the include
  `value_time_splits` of the feeds that it changes.
- Name the pass in the "Open Items" section of `AGENTS.md` until it runs.
- Record in ADR 0077 why the pass is not necessary.

### P5. Lower Priority Items

- The other search changes of priorities 2 to 4 can wait for P1 and for the
  search time test of priority 4.
- The shared client crate waits for the license decision.
- The question of the addendum about shows with `medium=podcast` needs a
  decision only if the operator wants such facts. No client asks for them.
- The limit of ADR 0058 in request 20 needs a decision only if a client asks
  for events from copies that no crawler reads.

## Addendum: Research Checks Of S2 And S4

Added on 2026-10-23 by the research repository at `~/build/musicindex-research`.
This addendum binds nothing. It gives two checks that this review did not do.
The research examined the source at `9ce7e16` and sent read requests to the
live API.

| # | Result of the research check | Data source |
|---|---|---|
| S2 | Confirmed in the source. The two retire paths delete only the index entry of the type `feed`, and then they delete the feed row (`src/api.rs:4802-4810`, `src/apply.rs:307-315`). A publisher feed has an index entry of the type `publisher` (`src/db.rs:1394`), and no retire path deletes it. These delete calls also send `""` as `name`, but the index holds the artist in `name`. Thus the artist words of a retired feed and of its tracks stay in the index too. The research found no retired publisher feed in the live index, so the effect is not observed. | Source at `9ce7e16` |
| S4 | Confirmed on the live API. Three searches with `type=publisher`, for `label`, `music` and `records`, gave 85 publisher hits. Each hit had `quality_score` 0. | `sources/captures/2026-10-23-search/search-q-*-type-publisher.json` in the research repository, with hashes in `SHA256SUMS` |

With these checks, P1 covers four defects that the source confirms. S1, S3 and
S4 are also observed on the live API or in an SQLite test. The research report
is `reports/2026-10-23-search-code-sharing-and-stophammer.md` in the research
repository.

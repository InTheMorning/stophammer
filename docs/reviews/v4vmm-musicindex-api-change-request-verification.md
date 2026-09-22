# Verification Of The v4vmm MusicIndex API Change Request

## Purpose

This document records the checks that Stophammer made on the v4vmm document at
`docs/plans/v4vmm-musicindex-api-change-request.md`.

It holds evidence. It states no rule. ADR 0042, ADR 0043 and ADR 0044 hold the
decisions that came from these checks.

## Method And Date

The checks ran on 2026-09-22. They used three sources:

- The live deployment at `https://api.musicindex.org`.
- The local checkout at commit `a220f44`.
- The local files `stophammer.db`, dated 2026-04-19, and
  `stophammer-crawler/analysis/data/feed_audit.ndjson`, dated 2026-04-03.

The snapshot file holds the raw XML of 7,538 feeds. The crawler fetched them in
March 2026.

## Result Summary

| # | Statement in the v4vmm document | Result |
|---|---|---|
| 1 | Search results hold no title | Correct |
| 2 | Track artwork hides its owner | Correct, and the defect is larger |
| 3 | A build timestamp can become a release date | Correct, and the effect is much larger |
| 4 | A renamed field disappears silently | Correct, and the cause is different |

Each code line number and each measured count in the v4vmm document is correct.

## The Deployed Revision

The v4vmm document asks which revision is deployed.

The live `/openapi.json` document and the document that commit `a220f44`
makes are the same, byte for byte. The command was `cargo run --bin gen_openapi`.

The live node public key is
`fae4b4c8de468abcb430b05efa1d8cfd8b85a3e90d62ca5a5c2c7ed4a56167bd`.

Limit of this evidence: the document comes from a static literal in
`src/openapi.rs`. A build with an unchanged literal gives the same document.
That match does not prove the deployed binary revision.

Two behavior checks give more evidence. The live search fields match
`SearchResponseItem` at `a220f44`. The live artwork behavior matches the query
code at `a220f44`. No check found a difference between the live service and the
local commit.

## Statement 1: Search Results Carry No Title

Confirmed in the code and on the live service.

`SearchResponseItem` at `src/query.rs:306` declares six fields: `entity_type`,
`entity_id`, `rank`, `quality_score`, `feed_guid` and `href`.

A live call to `GET /v1/search?q=love&limit=3` returned items with those six
fields and no other field.

The live contract shows the same six fields in its example for `/v1/search`.

## Statement 2: Track Artwork Hides Its Owner

Confirmed. The five cited lines are correct.
`COALESCE(t.image_url, f.image_url)` is at `src/query.rs:530`, `:549`,
`:1967`, `:2044` and `:2075`.

The two columns are different, as the v4vmm document states. The migrations
`0025_source_first_feed_track_fields.sql` and
`0032_feed_scoped_track_identity.sql` create them.

The measured count reproduces. 1,781 of 23,960 tracks hold their own
`image_url`. The other 22,179 tracks hold none.

### An Additional Defect

`build_feed_response` at `src/query.rs:661` selects `image_url` at
`src/query.rs:700` with no `COALESCE`. That route returns the track column
unchanged.

The field `image_url` thus has two different meanings. The meaning depends
on the route.

A live test shows the two meanings for one track. The track is
`64a25afa-a545-5a93-afd0-ecc118549913`, "Stereon - Night's Call". The feed is
`145287d8-9a96-5a03-a7e6-22b1aa5a75a0`. That track holds no artwork of its own.

| Live route | `image_url` |
|---|---|
| `GET /v1/feeds/{feed}?include=tracks` | empty |
| `GET /v1/feeds/{feed}/tracks/{track}` | the feed artwork URL |
| `GET /v1/tracks/{track}` | the feed artwork URL |

A client cannot see this difference from the field name. The v4vmm document did
not report it.

## Statement 3: A Build Timestamp Can Become A Release Date

Confirmed. Each cited line is correct.

- `stophammer-parser/src/profile.rs:180` holds the comment
  "RSS2: lastBuildDate as pubDate fallback".
- Line 184 reads `lastBuildDate` into `FeedField::PubDate`.
- `src/api.rs:2092` computes `feed_data.pub_date.or(oldest_item_at)`.
- Lines 2101 and 2103 supply the path labels `feed.pub_date` and
  `oldest_item.pub_date`.

### The Rule Order Is First-Wins

`src/engine.rs:98` holds `if feed.is_set(field) { continue; }`.

The `lastBuildDate` rule thus applies only when `pubDate` is missing, or
when the date transform fails on `pubDate`. It does not replace a good
`pubDate`. The v4vmm document describes this correctly.

### The Size Of The Affected Subset

The v4vmm document records this subset as unknown. It is measurable from the
raw XML snapshot. These counts come from the channel elements of 7,538 feeds.

| Channel elements | Feeds |
|---|---:|
| `pubDate` in the channel | 228 |
| `pubDate` in the channel and unparseable | 0 |
| `lastBuildDate` in the channel | 7,323 |
| `lastBuildDate` in the channel and no `pubDate` | 7,095 |
| No `pubDate` and no `lastBuildDate` | 215 |

For 7,095 feeds, which is 94 percent of the snapshot, `lastBuildDate` supplies
the feed publication date. Those claims hold the path label `feed.pub_date`.
That label names an element the parser did not read.

The stored claim counts agree. The local database holds 7,414 feed
`release_date` claims with the path `feed.pub_date`, and 214 with the path
`oldest_item.pub_date`. The 215 feeds with no date element in the snapshot
agree with the 214 stored `oldest_item.pub_date` claims.

### The Size Of The Error

The measurement compared `lastBuildDate` with the newest item `pubDate` in the
same feed. 7,071 feeds supplied the two values.

| Measure of the difference | Days |
|---|---:|
| Minimum | 0 |
| Median | 459 |
| 90th percentile | 963 |
| Maximum | 1,269 |

6,894 feeds show a difference of more than 30 days. 4,183 feeds show a
difference of more than 365 days.

### The Live Service Shows The Same Behavior

A live call to `GET /v1/feeds/recent?limit=50` returned 50 feeds. For 47 of
them, `release_date` is after `newest_item_at`. One feed reported a
`release_date` of 2026-09-22, which is the date of this check.

A release date that follows the newest track is not a release date. It is the
time of the feed build.

## Statement 4: A Renamed Field Disappears Silently

Confirmed. The cause is not the one the v4vmm document assumes.

The v4vmm document states that the contract marks fields optional. The live
contract marks no field at all.

| Measure of the live contract | Value |
|---|---:|
| JSON responses declared | 54 |
| Responses with a bare `{"type":"object"}` schema | 54 |
| Responses that declare properties | 0 |
| Entries in `components.schemas` | 0 |

The document thus records no field name. A rename changes no part of the
document, and no client can find the change.

### The Cause

`src/openapi.rs` builds the document from a hand-written `serde_json::json!`
literal in `spec_value`. The source tree holds zero `#[utoipa::path]`
attributes and zero `ToSchema` derives.

Nothing connects a Rust response type to the published document. A rename in a
response struct changes the response and leaves the document unchanged.

### Two Incorrect Statements In AGENTS.md

The checks found drift in this repository's own guidelines.

- Step 3 of "New API Endpoint" tells an author to add a `utoipa` attribute to
  a handler. No handler does this, and the attribute would change nothing.
- Step 4 tells an author to regenerate `api.html` with
  `cargo run --bin gen_openapi`. That command prints the OpenAPI JSON document
  to standard output. `api.html` is a hand-written page that reads
  `/openapi.json` at run time.

The "Commit Hygiene" section also names `api.html` as a generated artefact. It
is not generated.

## Statements Not Checked

The v4vmm document states client-side costs, such as 21 requests for a page of
20 results. Those statements describe the v4vmm client. Stophammer made no
measurement of them. They do not change the decisions.

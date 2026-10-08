# Verification Of v4vmm Request 5, One Search Hit For Each Entity

## Purpose

This document records the checks that Stophammer made on request 5 in
[the v4vmm open requests](../plans/v4vmm-open-requests.md).

It holds the facts that the checks found. It states no rule.

## Method And Date

The checks ran on 2026-10-07 and 2026-10-08. They used three sources:

- The live node at `https://api.musicindex.org`, revision `9dabeec`.
- The local checkout of Stophammer and of v4vmm.
- The production backup `stophammer-20260926T131633Z.db`, opened read-only.

## Result Summary

| # | Statement in request 5 | Result |
|---|---|---|
| 1 | `q=arbiter` gives track `f8b048de-df0c-40b1-9bf9-359307d10301` two times, with the same `entity_id`, `feed_guid` and `href`, and two ranks | Correct |
| 2 | A search for "How Bout You" gives one track two times | Correct |
| 3 | v4vmm drops a second hit of the same entity | Correct |
| 4 | The client fix does not correct the result count or the pagination | Correct |

## The Live Responses

On 2026-10-07, `GET /v1/search?q=arbiter` gave two rows for track
`f8b048de-df0c-40b1-9bf9-359307d10301` in feed
`7b7949ca-5019-5814-aa53-d4b14bd15a6d`. The ranks were `-12.566378885662488`
and `-11.103123308333087`. These are the values in the request.

`GET /v1/search?q=How%20Bout%20You` gave two rows for track
`b38479ed-e823-418a-8cb1-b8e5c21ca546` in feed
`a2d2e313-9cbd-5169-b89c-ab07b33ecc33`. The ranks were `-18.833191448459537`
and `-16.640215040807796`. The check cannot make sure of the date 2026-10-04 of
the first observation.

## The Cause

The two rows of one track are two different rows of the search index:

- One row has the feed-scoped key `["<feed_guid>","<track_guid>"]`, from
  `db::canonical_track_entity_id`. Each write uses this key since commit
  `dc61716` of 2026-04-23, which made the track identity feed-scoped.
- The other row has the bare track GUID. A write before that commit made it.
  No migration removed it. No subsequent write replaces it, because each
  subsequent write uses the other key.

The search handler resolved a bare-GUID row with `db::get_track_by_guid`. That
gave the same `feed_guid` and `href` as the feed-scoped row.

The check excluded two other causes:

- `search_entities` has `rowid` as its primary key, and `entity_quality` has
  `(entity_type, entity_id)`. Thus a join cannot make a second row.
- An `fts5vocab` count found no duplicate instance in the FTS5 index.

## The Size In The Backup

In the backup of 2026-09-26:

| Item | Count |
|---|---|
| Track rows in `search_entities` with the feed-scoped key | 26,700 |
| Track rows in `search_entities` with a bare GUID | 23,963 |
| Bare-GUID rows that have a feed-scoped twin | 23,763 |
| Bare-GUID rows for a track that is gone | 18 |
| Bare-GUID rows with a GUID that is in more than one feed | 7 |
| Track rows in `entity_quality` with a bare GUID | 23,940 |

A track gives two hits only when the query matches each of its two rows. A
bare-GUID row holds the text of the track from before 2026-04-23. Thus a track
with a new title can also match its initial title.

A bare-GUID row for a track that is gone gave a hit with `href` null. Some
bare-GUID rows have a GUID that is in more than one feed. Such a row gave the
`href` of one of those feeds, which can be an incorrect feed.

## The Requested Rank

The request asks for each entity with its best rank. For `arbiter`, the better
rank, `-12.57`, came from the bare-GUID row. The correct fix removes that row,
so the rank comes from the current text of the track. That rank is not always
the better of the two.

## The v4vmm Key

The v4vmm commit `59c27e4` keyed `unique_hits` by `(entity_type, entity_id)`.
For a track, `entity_id` is the track GUID, which is unique only in its feed.
Two tracks with one GUID in two feeds are two entities, and that key dropped
the second one. The key must hold `feed_guid`.

## The Fix

Stophammer:

- `db::try_open_db` calls `ensure_search_index_track_identity` after the
  migrations. It operates only when `search_entities` holds a bare-GUID track
  row. It clears the FTS5 index with `delete-all` and empties
  `search_entities`. It removes the bare-GUID track rows of `entity_quality`.
  Then it fills the index again for each feed.
- The index is contentless, so the repair cannot remove one row. FTS5 removes
  a row only with its initial text, and no table keeps that text.
- The search handler no longer resolves a bare-GUID row.
- `tests/search_one_hit_per_entity_tests.rs` makes the condition from before
  2026-04-23. It checks that one open of the database gives the track one time. The test fails when
  the repair call is removed.

v4vmm:

- `unique_hits` keys each hit by `(entity_type, feed_guid, entity_id)`.

## Measurement Of The Repair

On a copy of the backup of 2026-09-26, the first open took 3.7 seconds. That
time includes the migrations from the schema of that date. After the open:

- `arbiter` and "How Bout You" each gave one hit.
- `search_entities` held 0 bare-GUID rows, 26,882 track rows and 8,641 feed
  rows.
- A second open took 9 milliseconds, so the repair did not operate again.

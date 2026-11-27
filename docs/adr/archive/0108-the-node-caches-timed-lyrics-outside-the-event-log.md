# ADR 0108: The Node Caches Timed Lyrics Outside The Event Log

## Status
Superseded on 2026-11-27 by musicindex ADR 0104 R-LYRIC-6 (musicindex commit
`30367c2`). The operator selected a separate lyrics index on 2026-11-27. That
index fetches, normalizes, keeps and serves all lyrics. stophammer keeps only
its transcript references, and a community node serves no lyrics text. No code
held this record. Read it for history only.

Accepted on 2026-11-26. musicindex ADR 0103 R-LYRIC-4 item 3 gives the table,
the fetch and the API field to a stophammer ADR. The orchestrator session sent
the request on 2026-11-26. This session selected the shape below. No code
holds it, and the code waits for the parser of R-LYRIC-3 (§2).

- Bar: crosses repositories, and it is difficult to change. The node takes a
  parser from musicindex-lib. HitIt and Rainbrook read the API field.
  Without this record, an agent can put the lyrics text in an event. Each
  community node then keeps it in its log with no removal.
- Uses:
  - musicindex ADR 0103 R-LYRIC-2 to R-LYRIC-4, musicindex commit `868d834`.
  - [ADR 0003](../0003-sqlite-wal-primary-store.md) for the event log.
  - [ADR 0054](0054-a-fetch-reaches-only-public-feed-hosts.md) §1 and
    §4 for the address test and the web URL rule.
  - [ADR 0066](../0066-a-version-number-tells-what-to-upgrade.md) for the
    version.
  - [ADR 0077](../0077-a-new-body-value-states-its-refill-pass.md) for the
    refill pass.

## Date
2026-11-26

## Terms

- **Timed lyrics** are the words of a song with a start time for each line or
  each word (musicindex ADR 0103). This is an established term.
- A **transcript reference** is one row of `source_item_transcripts`: the URL,
  type, language and `rel` of one `podcast:transcript`. This is a project term.
- The **lyrics cache** is the table of §3. This is a project term.
- **Revalidation** is a conditional request with `If-None-Match` or
  `If-Modified-Since`. A server that has no change answers HTTP 304. This is an
  established HTTP term.

## Context

The node keeps each `podcast:transcript` of an item in
`source_item_transcripts` (migration 0028). The event
`SourceItemTranscriptsReplaced` carries the rows. `include=source_transcripts`
gives them on the two track routes (`src/query.rs`, `TRACK_INCLUDES`). The node
keeps only the URL and the type. It does not fetch the file.

The research note `research_notes/music-feeds/timed-lyrics.md`
(musicindex-research commit `118b316`) gives these facts:

- 202 references on 120 tracks in 34 feeds. They have 199 different URLs. 198
  URLs gave HTTP 200 and 1 URL gave HTTP 405.
- The 198 files have 450,639 bytes. The median is 2,292 bytes and the largest
  file is 13,167 bytes.
- 135 timed files give 5,177 cues. As namespace JSON, this is 2,984 bytes for
  each song.
- For all 27,646 tracks, the JSON is approximately 165 MB, or 11% of the
  database, on each node that keeps it.

The node fetches no file today. `src/fetch_guard.rs` holds the address test
(`resolve_and_validate_url`) and a fetch with a pinned address for each
redirect (`fetch_with_pinned_redirects`). That fetch has no caller. It sends no
request header, and it gives only the body. The crawler has the conditional
request of ADR 0050, in its own crate.

ADR 0003 makes the event log append-only. No code deletes an event. No ADR
gives a removal of an event.

## Decision

### 1. A worker in the node fetches each track transcript

A background task of the node, the lyrics worker, fetches the files. The ingest
does not fetch. An ingest is one database transaction, and a community node
gets its rows from events, not from an ingest.

- The worker runs on the primary and on a community node (§4).
- It reads the transcript references with `entity_type = 'track'`. A live item
  is out of scope (§6).
- It fetches each different URL one time. Two references with the same URL
  share one cache row.
- It fetches only an `http` or `https` URL (ADR 0054 §4). Each other URL gets
  the status `unsupported` with no request.
- It starts a pass every 10 minutes. A pass takes the URLs that have no cache
  row, then the rows with `next_check_at` in the past, oldest first. A pass
  takes at most 200 URLs.
- It sends at most one request at a time, and waits a minimum of 1 second between
  two requests.

The fetch extends `fetch_with_pinned_redirects`. It does not make a copy. The
extension adds the request headers and gives the HTTP status, `ETag` and
`Last-Modified` with the body.

- The address test of ADR 0054 §1 applies to the first URL and to each
  redirect target.
- At most 3 redirects, and at most 30 seconds for the chain
  (`MAX_SSRF_REDIRECTS` and `REDIRECT_CHAIN_TIMEOUT_SECS`).
- The size limit is 262,144 bytes (256 KiB). This is 19 times the largest file
  of the research note. A larger body stops the read and gets the status
  `too_large`.
- The user agent is `USER_AGENT` of `src/lib.rs`.
- When a cache row has `etag`, the request sends `If-None-Match`. When it has
  `last_modified`, the request sends `If-Modified-Since`.

The result of a request sets the time of the subsequent check:

| Result | Cache change | `next_check_at` |
|---|---|---|
| HTTP 200 | A new body, parsed again (§2) | now + 7 days |
| HTTP 304 | `checked_at` only | now + 7 days |
| HTTP 404 or 410 | `fetch_failed` | now + 7 days |
| Other failure | `fetch_failed`, `failure_count` + 1 | now + 1 hour × 4^(`failure_count` − 1), at most 7 days |
| Body too large | `too_large` | now + 7 days |

A failure does not delete a stored result. A row with `timed` that then fails
keeps its lyrics and its status, and only `failure_count`, `http_status` and
`checked_at` change. After 4 failures in sequence, the row gets
`fetch_failed` and loses its lyrics.

### 2. The parser of musicindex-lib normalizes the file

The worker gives the body and the type of the reference to the public parser of
musicindex ADR 0103 R-LYRIC-3. The node has no parser of its own, and no copy.

- The parser does not exist on 2026-11-26. The node code waits for it.
- The node takes the parser as a git dependency at a fixed revision, as it does
  `musicindex-build-info`. Both repositories have the license
  `AGPL-3.0-only`.
- The parser is in the tag crate `musicindex-tags` now, and that crate brings
  `id3` and `lofty`. The node needs the parser in a crate or a Cargo feature
  that does not bring the two audio dependencies. This is a request to
  musicindex-lib through the orchestrator session.
- Two references with the same URL and a different type give one parse, with
  the type of the first reference by `feed_guid`, `entity_id` and `position`.
- The cache row keeps the revision of the parser in `parser_rev`. The node keeps
  no copy of the original body. Thus, when the dependency moves to a new
  revision, the worker marks each row with a different `parser_rev` as due.
  It then fetches that row with no conditional header.

The parser result sets the status:

| Parser result | Status | `lyrics_json` |
|---|---|---|
| Cues, each with a start time | `timed` | The namespace JSON of R-LYRIC-2 |
| Text with no time, for example plain text | `untimed` | null |
| A type or a format that the parser does not read, for example `text/html` | `unsupported` | null |
| An error, or no cue | `parse_failed` | null |

The node gives no text for `untimed`. musicindex ADR 0103 open point 1 asks if
a client shows a text with no time. When the owner of that point decides, a
subsequent ADR can add the text.

### 3. A local cache table keeps the result

A migration adds the table `transcript_lyrics_cache`. It is `STRICT`:

| Column | Type |
|---|---|
| `url` | `TEXT PRIMARY KEY` |
| `mime_type` | `TEXT`, the type that the parser got |
| `status` | `TEXT NOT NULL`, a `CHECK` on the values of §2 and `fetch_failed`, `too_large` |
| `lyrics_json` | `TEXT`, only for `timed` |
| `etag` | `TEXT` |
| `last_modified` | `TEXT` |
| `http_status` | `INTEGER`, the last status, or null when no request occurred |
| `byte_length` | `INTEGER`, the length of the last body with HTTP 200 |
| `parser_rev` | `TEXT` |
| `fetched_at` | `INTEGER`, the last HTTP 200 |
| `checked_at` | `INTEGER NOT NULL`, the last request or the last decision with no request |
| `next_check_at` | `INTEGER NOT NULL`, with an index |
| `failure_count` | `INTEGER NOT NULL DEFAULT 0` |

The table is local to each node, as `source_gone_answers` (ADR 0067) and
`feed_crawl_cache` are.

- No event carries a cache row. The signed event log keeps only the transcript
  references, as now (musicindex ADR 0103 R-LYRIC-4 item 2).
- No sync route sends the table.
- The table has no `feed_guid`, so a GUID change (ADR 0106) does not change
  it.
- At the end of each pass, the worker deletes each row whose URL no track
  reference holds. A retire, a GUID change or a new body without the tag thus
  removes the cached text at the subsequent pass.

### 4. Each node fetches and normalizes for itself

A community node runs the same worker against its own references. It does not
get the cache from the primary.

- The API gives the same field on each node. A client can use any node.
- The load on a file host is one request for each node each 7 days, for each
  URL. A file has a median size of 2,292 bytes.
- The environment variable `LYRICS_FETCH=off` stops the worker on a node. The
  node then makes no request to a file host. Each reference with no cache row
  gives the status `disabled` (§5). Each other value, or no value, starts the
  worker.

### 5. The track routes give the lyrics with `include=lyrics`

`TRACK_INCLUDES` gets the value `lyrics`. `GET /v1/tracks/{guid}` and
`GET /v1/feeds/{guid}/tracks/{track_guid}` then give the field `lyrics`. The
field is a list with one entry for each transcript reference of the track, in
the sequence of `position`. With no request for it, the response has no field.
`include=source_transcripts` does not change.

| Field | Value |
|---|---|
| `position` | The position of the reference |
| `url` | The URL, or null when it is not `http` or `https` (ADR 0054 §4) |
| `mime_type` | The type of the reference |
| `language` | The language of the reference |
| `rel` | The `rel` of the reference |
| `status` | One value of the list below |
| `checked_at` | `checked_at` of the cache row, or null |
| `transcript` | The namespace JSON object `{version, segments}` for `timed`, else null |

The values of `status`:

| Value | Meaning for a client |
|---|---|
| `timed` | `transcript` has the cues with their times. |
| `untimed` | The file has text with no time. The node gives no text. |
| `unsupported` | The node does not read this URL or this format. |
| `parse_failed` | The node read the file, and the parser gave an error or no cue. |
| `fetch_failed` | The last request failed after its retries. |
| `too_large` | The file is larger than 262,144 bytes. |
| `pending` | The worker has not fetched the URL yet. |
| `disabled` | This node does not fetch files (§4). |

A client takes the entry with `timed`, in the sequence of the list, and its
own language preference. A client does not fetch the file itself. When no
entry has `timed`, the client shows no timed lyrics.

The OpenAPI document and `docs/API.md` get the include value, the entry schema
and the status list.

### 6. Out of scope

- The live item routes. musicindex ADR 0103 R-LYRIC-5 item 3 gets the lyrics
  of the song from its track, through `feedGuid` and `itemGuid`.
- The text of an `untimed` file (§2).
- The anchors of R-LYRIC-5. They are a client rule.
- The meaning of `rel`. musicindex ADR 0103 open point 4 owns it. The node
  fetches each reference with each `rel`.

### 7. The node needs no event removal

The research note left this question open: can the node remove a text from
its signed event log? This ADR does not need an answer. No event carries the
lyrics text (§3). When a source removes or changes a transcript, the
subsequent ingest replaces the references. The worker then deletes or
replaces the cached text on each node.

The old events keep the old transcript URL, as each old event does now (ADR
0003). This ADR does not change that, and gives no removal of an event.

## Alternatives Considered

### The text goes in the event log

A field of `SourceItemTranscriptsReplaced` with the JSON makes each community
node keep the same text with no fetch. It adds approximately 165 MB to each
node for all tracks, and the append-only log keeps each old version. A
correction or a removal by the artist then stays in each log. The operator
choice of 2026-11-26 rejects this (musicindex ADR 0103 R-LYRIC-4 item 2).
Rejected.

### A community node gets the cache from the primary

A sync route for the cache gives the same text with no request to a file host.
It needs a new route, a new client in `community.rs` and its own revalidation.
A file host sees at most a few nodes, and each file is small. Rejected.

### A community node has no lyrics

The worker on the primary only is less code. A client then gets `lyrics` from
the primary only, and the field has two meanings on two node roles. The
variable `LYRICS_FETCH=off` gives this to an operator who wants it. Rejected as
the default.

### The ingest fetches the file

The ingest of the primary can fetch each new URL. A slow host then holds the
ingest transaction, and a community node gets no fetch. Rejected.

### The crawler fetches the file

The crawler has the conditional request of ADR 0050. But the crawler sends
feed bodies. A new path for transcript bodies needs a new ingest route, and
also a new event or a sync of the cache. Rejected.

### `musicindex-fetch` makes the request

`musicindex-fetch` in musicindex-lib has a blocking client for feed bodies and
enclosures, with no address test. The node uses an async client with the ADR
0054 guard. Rejected.

### `include=source_transcripts` gives the lyrics

A field in each entry of `source_transcripts` needs no new include value. The
JSON is approximately 3 KB for each song, and that include is a list of source
facts. A separate include lets a client ask for the facts with no lyrics.
Rejected.

## Consequences

- **The order of the work.** musicindex-lib makes the public parser (R-LYRIC-3)
  in a crate or a feature with no audio dependency. Then a task file of this
  ADR gives the node work. The orchestrator session sends the dependency
  request to musicindex-lib.
- **The refill pass (ADR 0077 §1).** The node keeps no new value from a feed
  body. The worker fills the cache from the existing references, so no crawler
  pass is necessary.
- **The refill check (ADR 0077 §1).** One day after the deploy, a backup gives
  two numbers. The first is the number of different `http` and `https` URLs
  of the track references. The second is the number of cache rows with a
  status that is not `pending`. The two numbers are the same. One known track with an SRT file gives `timed` and its
  cues in `GET /v1/tracks/{guid}?include=lyrics`.
- **The open check.** `AGENTS.md` names the refill check in "Open Checks"
  until it passes (ADR 0077 §2).
- **The version.** A new include value, a new response field and a new
  migration raise MINOR (ADR 0066 §4). The event types and the payloads do not
  change, so the community nodes need no upgrade sequence.
- **The guards of the contract.** `tests/fixtures/v1-schema-fields.json` gets
  `lyrics` and the entry schema. The migration gets its line in
  `tests/fixtures/migration_sha256.txt`.
- **The size.** The cache keeps only the timed JSON, approximately 165 MB for
  all tracks if each track had a file. With 135 timed files today, it is
  approximately 400 KB.
- **HitIt and Rainbrook** read `include=lyrics` for a lyrics view. They make
  no request to the file host.

## Verification

Mechanical. Each criterion is a test in `stophammer`. The HTTP tests use a
local server and `skip_ssrf_validation`.

- The cache table: a pass with two track references to one URL makes one
  cache row. A pass after the removal of the references deletes the row.
- The fetch: the first request has no conditional header. A row with `etag`
  and `last_modified` sends `If-None-Match` and `If-Modified-Since`. HTTP 304
  changes only `checked_at` and `next_check_at`.
- The fetch: a body of 262,145 bytes gives `too_large`. A failure after a
  `timed` result keeps the lyrics, and the fourth failure in sequence gives
  `fetch_failed`.
- The fetch: a reference with a non-web URL gives `unsupported` with no
  request. With `LYRICS_FETCH=off`, the worker makes no request and the API
  gives `disabled`.
- The fetch: the address test rejects a redirect to a private address. This
  test uses the guard, not `skip_ssrf_validation`.
- The API field: `include=lyrics` gives one entry for each reference, in the
  sequence of `position`. A `timed` entry has `transcript` with `version` and
  `segments`, and each other status has `transcript` null. A reference with
  no cache row gives `pending`.
- The event log: after a pass with a `timed` result, no event payload has the
  text of a cue.
- The parser revision: a row with a different `parser_rev` becomes due, and
  its request has no conditional header.

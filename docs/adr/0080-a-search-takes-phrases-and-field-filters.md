# ADR 0080: A Search Takes Phrases And Field Filters

## Status
Proposed on 2026-10-24, for items 1 to 7 of v4vmm request 10. The v4vmm
operator answered items 2 to 7 on 2026-10-24, and decided on the same day that
Stophammer drafts this ADR now.

When it is accepted, it amends ADR 0078 §5: the text of `q` can hold phrases.
The rest of ADR 0078 stays.

## Date
2026-10-24

## Context

[v4vmm request 10](../plans/v4vmm-open-requests.md#10-search-accepts-words-phrases-and-field-filters)
asks for seven items. These are phrases in `q`, filter parameters, exclusions
and a stable error code. They are also a search with no `q`, a contract for
the filters, and a filter by host. v4vmm
ADR 0087 §5 sends each field of its query text as a parameter, never as syntax
in `q`. Until Stophammer applies a field, v4vmm shows no Index rows for a
query with that field. musicindex.org will send the same parameters.

These facts come from the source at `5c8b1b2`:

- `fts5_query_from_words` (`src/search.rs:199`) puts double quotes around each
  word of `q` (ADR 0078 §5). Thus `"night drive"` matches the two words in each
  order and at each distance.
- `SearchQuery` (`src/query.rs:137`) reads `q`, `type`, `limit`, `cursor` and
  `include`. The handler ignores each other parameter.
- A `q` with no word gives `400` with the message `q has no word` and no code
  (`src/query.rs:5089`).
- The index row of a feed and of a publisher holds `release_artist` in `name`.
  The row of a track holds `track_artist` in `name`. Each row holds its title
  in `title` (`src/db.rs:1486-1512`). The row of a track does not hold the
  title of its feed.
- The ingest of a feed sets `release_kind` to `unknown` (`src/api.rs:3291`).
  No other code writes a release kind.
- No column holds the host of `feed_url`. Three statements write `feed_url`:
  the two feed upserts (`src/db.rs:1351`, `src/db.rs:6707`) and the operator
  relocation (`src/api.rs:5314`).
- The `409` body of an ambiguous track GUID has `error` and `code`
  (`AmbiguousTrackGuidBody`, `src/api.rs:96`). No other error body has a code.

The backup of 2026-10-24 gives these counts:

| Fact | Count |
|---|---|
| Feeds | 10,583 |
| Feeds with the release kind `unknown` | 10,583 |
| Feeds at the host `wavlake.com` | 9,291 |
| Feeds with no `newest_item_at` | 1,814, of which 1,773 are publisher feeds |
| Tracks | 27,613 |
| Tracks with no `pub_date` | 29 |
| Tracks with no `duration_secs` | 83 |
| Tracks with no `track_artist` | 0 |
| Index rows of the types `feed`, `publisher` and `track` | 8,798, 1,774 and 27,612 |

## Decision

### 1. A phrase in `q` is text in double quotes

The node reads `q` from left to right. It first removes each control
character, as ADR 0078 §5 states.

- Text between two double quotes is one phrase. The node gives it to FTS5 as
  one phrase, so it matches its words in sequence.
- A double quote with no second double quote after it is part of its word.
- Each other text divides into words at white space. Each word is one FTS5
  phrase, as ADR 0078 §5 states.
- A phrase with no word, for example `""`, adds nothing.
- The words and the phrases join with the implicit AND of FTS5.

No character of `q` is an FTS5 operator, a column filter or a prefix. Each `q`
that has a word or a phrase gives `200`, possibly with no rows.

This amends ADR 0078 §5, which adds no phrase syntax.

### 2. The filters

`GET /v1/search` takes these filter parameters. Each filter applies together
with `q` and with each other filter. No filter gives an OR between two fields.

| Parameter | Value | `type=feed` | `type=track` | `type=publisher` |
|---|---|---|---|---|
| `artist` | text | `release_artist` | `track_artist` | the publisher title |
| `title` | text | the feed title | the track title | the publisher title |
| `release` | text | the feed title | the title of the feed of the track | unsupported |
| `kind` | `album`, `ep`, `single`, `unknown`, with commas | `release_kind` | the `release_kind` of the feed of the track | unsupported |
| `tracks`, `min_tracks`, `max_tracks` | a count | `episode_count` | the `episode_count` of the feed of the track | unsupported |
| `after`, `before` | a date, `YYYY-MM-DD` | `newest_item_at` | `pub_date` | unsupported |
| `min_duration`, `max_duration` | seconds | unsupported | `duration_secs` | unsupported |
| `host` | a host name | the host of `feed_url` (§5) | the host of the feed of the track | the host of `feed_url` |

- **Text.** The value of `artist`, `title` and `release` has words and phrases,
  with the rules of §1. Each word and each phrase must be in the named field.
  A filter matches whole words, as `q` does. `artist=jsmo` does not match
  `jSmovie`.
- **Release kind.** `kind` matches the stated `release_kind` only. The node
  infers no kind. `kind=unknown` matches `unknown` and a feed with no release
  kind. The commas give an OR of the listed kinds.
- **Counts.** `tracks` matches one count. `min_tracks` and `max_tracks` include
  their limit. A count is an integer from 0.
- **Dates.** A date is a day in UTC. `after` matches a value on that day or
  later. `before` matches a value on that day or earlier. A row with no date
  does not match a date filter. An `after` later than a `before` gives `200`
  with no rows.
- **Durations.** A duration is an integer number of seconds from 0. The two
  limits are included. A track with no `duration_secs` does not match a
  duration filter.
- **Request with no `type`.** The node merges feeds and tracks, as now. Such a
  request accepts a filter only when `type=feed` and `type=track` each
  support it.
  `min_duration` and `max_duration` give the error of §4.

The node reads each filter from the stored rows. A filter selects rows. It does
not change the rank of a row.

The client keeps the kind rule of musicindex ADR 0020 R-WORD-1. For example,
for `kind:single` it sends one request with `kind=single`, and one with
`kind=unknown&tracks=1`.

### 3. Exclusions

`not_q`, `not_artist`, `not_title`, `not_release` and `not_host` each remove
the rows that match their value. A `not_` parameter applies to the same field
as its positive parameter. `not_q` applies to `name`, `title` and
`description`, as `q` does.

- Each `not_` parameter can repeat. Each value is one exclusion.
- A value with two words removes only a row that has the two words. Thus one
  exclusion is one term of the v4vmm query text.
- A request has at most 10 exclusions. The 11th gives `400` with the code
  `invalid_filter`.
- A request with exclusions and no positive term gives `400` with the code
  `missing_positive_term`. A positive term is a word or a phrase of `q`, or a
  filter of §2.

A positive filter does not repeat. A second value gives `400` with the code
`invalid_filter`.

### 4. Error codes

A `400` of `GET /v1/search` for a filter has this body:

```json
{"error": "min_duration is not supported for type=feed", "code": "unsupported_filter", "filter": "min_duration"}
```

| Code | Cause |
|---|---|
| `unsupported_filter` | The type of the request cannot apply the filter, or the node does not know the parameter |
| `invalid_filter` | The value is not correct, a positive filter repeats, or the request has more than 10 exclusions |
| `missing_positive_term` | The request has no word, no phrase and no filter of §2 |

- `filter` gives the name of the parameter. A body with the code
  `missing_positive_term` has no `filter`.
- A parameter that the node does not know gives `unsupported_filter`. Thus a
  client that sends a newer filter to an older node gets an error. It does not
  get rows that the filter did not reduce.
- A request with no `q` and no filter now gives `missing_positive_term`. The
  status stays `400`.

### 5. The node stores the host of each feed

`feeds` gets the column `feed_host`. It holds the host of `feed_url`, as the
`url` crate parses it, in lower case. A URL that does not parse gives no host.

- Each statement that writes `feed_url` also writes `feed_host`, in the same
  statement.
- A migration adds the column. A repair of ADR 0046 in `open_db` fills each row
  with no host from its `feed_url`. A second open fills nothing.
- A community node calculates the host from its own `feed_url`. No event
  payload changes.
- A `host` value matches its host and each subdomain. `wavlake.com` matches
  `wavlake.com` and `feeds.wavlake.com`. It does not match `notwavlake.com`.
- The node reads the value with the host rules of the `url` crate. A value with
  a scheme, a path, a port or user data gives `invalid_filter`.
- A feed with no host does not match `host`, and `not_host` does not remove it.

The column has no index. A host filter reads each feed. The backup of
2026-10-24 has 10,583 feeds.

The host comes from `feed_url`, not from a feed body. Thus ADR 0077 §1 states
no refill pass for it.

### 6. A search with no `q`

A request with one or more filters and no word or phrase in `q` runs on
`GET /v1/search`. It reads the same rows that the index holds, so the medium
rules of ADR 0071 §5 apply.

- Its rows are newest first: `newest_item_at` for a feed and a publisher, and
  `pub_date` for a track. A request with no `type` merges feeds and tracks by
  that date.
- A row with no date comes after each row with a date. Rows with the same date,
  or with no date, come in rowid order.
- Its cursor holds the date and the rowid. A cursor of a search with `q` gives
  `400` on a search with no `q`, and a cursor of a search with no `q` gives
  `400` on a search with `q`.
- `rank` is 0 on each of its rows.

A search with `q` keeps its order and its cursor (ADR 0022 and ADR 0078).

A cursor is valid only with the parameters of the request that gave it.

### 7. The contract states the filters

- `/openapi.json` declares each parameter of §2 and §3 on `GET /v1/search`.
  Each description names the types that apply it.
- `/openapi.json` declares the error body of §4 as a schema.
- `GET /v1/node/capabilities` gives `search_filters`, a map from each type to
  the names of its filters. The handler and the capabilities read one table,
  as the include lists do (v4vmm request 3).
- `tests/fixtures/v1-schema-fields.json` gets each new property and each new
  enum value (ADR 0044).

## Alternatives Considered

### Stophammer parses field syntax in `q`

v4vmm ADR 0087 rejects this. v4vmm then cannot tell the curator which field is
not correct, and the two clients must agree on one syntax. ADR 0078 removed
each column filter from `q` for the same reason.

### A fourth index column with the title of the feed of a track

This lets `release` match a track in one `MATCH`. It needs a rebuild of the
index, and each `q` then matches a track by the title of its feed. §2 reads
the `title` of the index row of the feed instead.

### The filters go into the FTS5 expression of `q`

One `MATCH` then gives each text filter. bm25 then also scores the filter
words, so a filter changes the rank. §2 keeps the rank a function of `q` only.

### The host comes from `feed_url` in SQL

`substr` and `instr` give an incorrect host for a URL with a port, user data
or upper case. The `url` crate already parses each feed URL at ingest.

### The node ignores a parameter that it does not know

This is the present behavior. A client that sends a newer filter to an older
node then gets rows that the filter did not reduce. v4vmm ADR 0087 forbids
such rows.

## Consequences

- `kind=album`, `kind=ep` and `kind=single` give no rows now. The ingest sets
  `unknown` for each feed. A later decision that stores a stated kind changes
  this, with no change to the filter.
- A filter matches whole words. The Library search of v4vmm matches parts of
  words, so the two searches can give different rows for the same text.
- A request with a parameter that the node does not know gives `400`. Before
  this decision, the node ignored it. A client that sends an extra parameter,
  for example a cache key, must remove it.
- A request with no `q` and no filter gives a different body: the code
  `missing_positive_term`, with the same status.
- `feeds` gets one column, so the change needs a migration, a line in
  `tests/fixtures/migration_sha256.txt`, a change to `src/schema.sql`, and a
  repair of ADR 0046.
- A primary and a community node can upgrade in each sequence. The change is
  an addition, so it is a MINOR release (ADR 0066).
- `docs/API.md`, `src/openapi.rs` and `api.html` change in the same commit as
  the code.
- v4vmm and musicindex.org replace their stored contract when they decode the
  new error body.
- Request 11 stays separate. A date filter is more useful when a feed hit also
  gives its date.

## Open Questions

1. **`host` on `type=publisher`.** The operator answer for item 2 lets a
   publisher search take only `artist` and `title`. Item 7 came after it. This
   ADR proposes that `host` and `not_host` also apply to a publisher, because a
   publisher feed has a `feed_url`.
2. **The order of a publisher search with no `q`.** 1,773 of 1,774 publisher
   feeds have no `newest_item_at`. With §6, such a search gives them in rowid
   order. An order by title, as `GET /v1/feeds/by-name` gives, is the other
   possibility.
3. **The limit of 10 exclusions.** The limit keeps the cost of one request
   small. The operator can give a different limit.

## Checks

Each criterion is mechanical. A test in `tests/` proves it.

1. §1: `q="night drive"` matches a title with `night drive`, and not a title
   with `drive night`. `q=night drive` matches the two.
2. §1: `q` values with an unpaired double quote, `""`, `"`, an apostrophe and a
   colon each give `200`.
3. §2: for each cell of the table that is not "unsupported", a test gives one
   row that matches and one row that does not match.
4. §2: `kind=unknown` matches a feed with `unknown` and a feed with no kind.
   `kind=album,single` matches an album and a single.
5. §2: `after` and `before` include their day in UTC. A row with no date does
   not match.
6. §2: the `rank` of a hit is the same with and without a filter.
7. §3: each `not_` parameter removes the rows that match, and two values of
   one parameter remove the rows of each value.
8. §3: 11 exclusions give `invalid_filter`. Exclusions with no positive term
   give `missing_positive_term`.
9. §4: each "unsupported" cell of §2 gives `unsupported_filter` with the name
   of the filter. A request with no `type` and `min_duration` gives it too. An
   unknown parameter gives it.
10. §4: a date, a count, a duration, a kind and a host value that is not correct
    each give `invalid_filter`.
11. §5: a feed write, a feed upsert and the operator relocation each set
    `feed_host` from the new `feed_url`. A database with no host values opens
    with a host in each row, and a second open writes nothing.
12. §5: `host=wavlake.com` matches `wavlake.com` and `feeds.wavlake.com`, and
    does not match `notwavlake.com`.
13. §6: a search with filters and no `q` gives its rows newest first. A row
    with no date comes last. Its cursor gives the next page with no row two
    times.
14. §6: a cursor of a search with `q` gives `400` on a search with no `q`, and
    the reverse.
15. §7: each filter name of the handler table is in `/openapi.json` and in
    `search_filters` of `GET /v1/node/capabilities`.

No criterion is visual.

# ADR 0049: Publisher Relationships Are RSS Facts

## Status
Accepted

## Date
2026-09-23

## Context
The v4vmm client asks Stophammer to report publisher relationships as the facts
that RSS states. The client document is
[the publisher relationship request](../plans/v4vmm-publisher-relationship-request.md).
The client evidence is
[the publisher feed research](../reviews/v4vmm-publisher-feed-artist-research.md).
The operator gave four decisions on 2026-09-23. The request records them.

An album names its publisher in `<podcast:publisher><podcast:remoteItem
medium="publisher">`. A publisher feed lists its albums with `<podcast:remoteItem
medium="music">`. On Wavlake, the album-to-publisher link is correct. The
publisher-to-album link is not. Each listed `feedGuid` is the Wavlake URL
identifier, not the `podcast:guid` of the album.

Stophammer checked the request against the code at commit `2436e2a` and the
local database dated 2026-04-19. Most statements are correct. These statements
are not correct or not complete:

1. The request says that no Stophammer ADR records the Wavlake rule. ADR 0035
   records it. It sets `feeds.publisher` to `"Wavlake"` and lets linked
   publisher metadata supply the artist text. ADR 0038 repeats the rule in its
   "Wavlake caveat" section.
2. There is a third host rule. `wavlake_artist_name_from_links` in `src/api.rs`
   makes an artist name from the path of a Wavlake website link. This is a
   guess from the URL layout.
3. The rule also changes feeds that are not on Wavlake. For those feeds,
   `publisher` is the publisher feed title when the back-link matches, and
   otherwise the `itunes:owner` name.
4. The request says "Stophammer fetches". The node never fetches a feed. ADR
   0006 makes the crawler the only component that fetches.
5. The crawler has host pacing (`HOST_DELAY_MS`, 1500 ms by default) and a retry
   on HTTP `429`. It does not send a conditional GET for a feed. Only the
   snapshot download of the `import` mode sends `If-Modified-Since`.
6. The parser does not read `rel` on `podcast:remoteItem`.
   `IngestRemoteFeedRef` has no `rel` field.
7. The node does not store the URL that it received a feed from.
   `IngestFeedRequest.source_url` goes to the log only.
8. `load_publisher` in `src/query.rs` also matches a back-link by `feedGuid`
   only. `has_reciprocal_music_remote_item` is not the only such match.

These measurements come from the local database, dated 2026-04-19:

| Measured item | Count |
|---|---:|
| Music feeds | 7,583 |
| Publisher feeds that are indexed | 49 |
| Distinct publisher feeds that albums name | 1,642 |
| Named publisher feeds that are indexed | 44 |
| Music feeds with `publisher = 'Wavlake'` | 6,652 |

On 2026-09-23 Stophammer read the DETOX Wavlake artist feed. It lists 30 albums.
Each listed `feedUrl` has the form `https://wavlake.com/feed/music/<id>`.

| Listed `feedUrl` compared with the stored `feed_url` | Albums |
|---|---:|
| Equal | 6 |
| Stored as `https://wavlake.com/feed/<id>` | 22 |
| Not indexed | 2 |

Both URL forms return HTTP `200` with no redirect. Both serve the same
`podcast:guid`. Thus a comparison with the stored URL finds 6 of 30 albums. A
fetch of the listed URL finds each album that is available.

## Decision
A publisher relationship is a set of RSS facts. Each derived value is a
separate field, and its name or a companion field gives its derivation. No rule
uses the host or the URL layout of a feed.

### 1. The node records which URL gave which GUID
The node stores one URL observation for each accepted ingest. An observation
holds the URL, the `podcast:guid` in the feed body, and the time. The node
records `canonical_url` and `source_url` when they are different.

An observation goes in the signed ingest event. A community node thus derives
the same resolution as the primary node.

A later accepted ingest of the same URL replaces its observation. A failed fetch
does not delete an observation.

The stored `feed_url` of a feed does not change when the node receives the same
`podcast:guid` through a different URL. That ingest adds only a URL
observation. Wavlake serves one album at two URL forms, so without this rule
the stored URL, and a signed event, would change on each pass.

### 2. The crawler follows publisher links
The crawler submits the feeds that a publisher link names. It uses the ordinary
ingest route, and ADR 0006 is unchanged.

1. When a music feed names a publisher `feedUrl`, the crawler fetches and submits
   that publisher feed.
2. When a publisher feed lists a `feedUrl` with `medium="music"`, the crawler
   fetches and submits that feed.
3. The crawler does not follow a link from a feed that it fetched only because
   of step 2. The walk stops at one level.
4. The crawler uses the existing host pacing and the HTTP `429` retry.

### 3. The node resolves a back-link and never guesses
For each album that a publisher feed lists, the node resolves the link at read
time, in this order:

1. The listed `feedGuid` is the GUID of an indexed feed. The resolution is
   `guid`.
2. A URL observation for the listed `feedUrl` gives the GUID of an indexed feed.
   The resolution is `feed_url`.
3. Otherwise the resolution is `unresolved`.

The node stores the declared values unchanged. It does not store a resolution.
Thus a resolution cannot become stale when an album feed changes. When an album
feed is deleted or retired, the next read gives `unresolved`.
`publisher_link_observed_at` is the time of the URL observation that the read
uses, so it also comes from the read, not from a stored resolution.

### 4. The API reports each relationship fact
The `publisher` view on a feed read adds these fields to each row:

| Field | Meaning |
|---|---|
| `music_names_publisher` | The album names this publisher feed |
| `publisher_lists_music` | The publisher feed lists this album, by any resolution |
| `publisher_link_resolution` | `guid`, `feed_url` or `unresolved` |
| `publisher_link_observed_at` | The time of the URL observation. Null unless the resolution is `feed_url` |

`reciprocal_declared` and `two_way_validated` keep their names and their
meaning: the other side lists this feed. A `feed_url` resolution now counts.

A publisher feed can list an album that names a different publisher. The view
reports that row with `music_names_publisher` false. It is a "listed by"
relationship, not ownership.

### 5. Each text field names its source

| Field | Source |
|---|---|
| `release_artist` | The album `itunes:author`. The fallback follows this table |
| `release_artist_source` | `itunes_author`, `itunes_owner` or `placeholder` |
| `publisher_text` | The album `itunes:owner` name |
| `publisher_feed_title` | Derived. The title of the indexed feed that the album names as its publisher |

When an album has no `itunes:author`, `release_artist` is the `itunes:owner`
name, unless that name is a platform name such as "Wavlake". When neither value
is available, `release_artist` is "Unknown Artist". The feed-scoped artist
credit needs a name, so the field is never empty. `release_artist_source` tells
the client which of the three values it received.

A track inherits `publisher_text` from its feed, as ADR 0035 states.

These code paths are deleted: the `is_wavlake_url` branches in ingest,
`publisher_repair_text`, `publisher_repair_release_artist`, and
`wavlake_artist_name_from_links`. A Wavlake album still reports
`publisher_text` "Wavlake", because its `itunes:owner` name is "Wavlake".

### 6. A role names its source
The parser reads `rel` on each `podcast:remoteItem`. The node stores the raw
value. The Podcast Namespace does not define this attribute, so the response
marks it as non-standard. Each `publisher` row reports:

| Field | Value |
|---|---|
| `publisher_rel` | The raw `rel` on the publisher feed item that lists the album |
| `music_rel` | The raw `rel` on the album item that names the publisher |
| `role` | The stated value, or `artist` when neither side states one. Null on a conflict |
| `role_source` | `publisher_rel`, `music_rel`, `default` or `conflict` |

When both sides state a value and the values are equal, `role_source` is
`publisher_rel`. When the values are different, the node does not select one.
Provenance First requires that the conflict is visible.

### 7. The artist count is derived and says so
A feed read of a publisher feed reports `distinct_release_artist_count`. It is
the number of distinct `itunes:author` values across the music feeds that name
this publisher feed.

The normalization before the comparison is:

1. Remove the white space at the start and at the end.
2. Replace each internal run of white space with one space.
3. Change each character to Unicode lowercase.

The node does not split credits such as "feat.". Such a credit can count as a
different artist. The response also lists the distinct raw values in
`distinct_release_artists`, so a client can examine the count.

### 8. The corrective pass reports what it cannot resolve
The `refresh` mode of ADR 0047 runs the new crawler steps. At the end, the pass
reports the number of `unresolved` back-links, read from the node. A change of
the Wavlake format thus becomes visible as an increase in that number.

### 9. The tests use real feeds
The fixtures come from these feeds:

- the DETOX Wavlake album and artist feed, where the listed `feedGuid` is
  incorrect,
- an RSS Blue or Fountain publisher feed that an artist operates,
- the Sir Libre Records label feed, with `rel="label"`,
- the Jimmy V feed, with `rel="producer"` on an album that names a different
  publisher,
- an album that names no publisher.

## Alternatives Considered
- **Keep the host rule.** The rule states a fact that RSS already gives through
  `itunes:owner`. It also hides the album artist and changes feeds that are not
  on Wavlake.
- **Resolve a link from the URL layout.** Wavlake uses the same identifier in
  both URL forms, so a pattern would work today. The operator refused it. A
  change of the layout would give an incorrect link and no error.
- **Ask Wavlake to correct `feedGuid`.** The operator refused it. The resolution
  must work on the feeds as they are.
- **Let the node fetch.** ADR 0006 keeps each fetch in the crawler. A fetch in
  the node adds network access to the signed path.
- **Store the resolution.** A stored resolution becomes stale when an album
  feed changes. A resolution at read time from observations cannot.

## Consequences
- This ADR supersedes the Wavlake exception of ADR 0035. The rest of ADR 0035
  stays in force. It also replaces the "Wavlake caveat" section of ADR 0038.
- The change spans the three repositories. The parser reads `rel`. The crawler
  follows publisher links. The node stores observations, resolves links and
  reports the new fields. Each commit in a crate repository names this ADR.
- The storage shape changes. A migration adds the URL observations and the
  `rel` columns.
- The signed ingest event carries the URL observation. A community node must
  accept the new field before the primary node sends it.
- For Wavlake albums, `reciprocal_declared` and `two_way_validated` change from
  false to true when a `feed_url` resolution exists. This is a correction, but
  a client sees a different value.
- For feeds that are not on Wavlake, `publisher_text` changes from the
  publisher feed title to the `itunes:owner` name. The title moves to
  `publisher_feed_title`.
- The new fields need ADR 0044 schemas.
- The first pass is expensive. The request estimates about 1,642 publisher feed
  fetches and about 6,652 Wavlake album fetches. At the default host pacing of
  1500 ms, the Wavlake album fetches alone take about 2 hours 46 minutes. The
  crawler does not send a conditional GET for a feed, so each later pass costs
  the same. Conditional GET helps each crawler mode, so a separate ADR decides
  it.
- A feed that first entered the index through one URL form keeps that URL. A
  client that compares `feed_url` with a listed `feedUrl` gets a false result
  for 22 of the 30 DETOX albums. The `publisher` view gives the correct
  relationship.

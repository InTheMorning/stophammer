# ADR 0088: A Feed Is Found By Its URL

## Status
Accepted on 2026-11-08 by the operator, for musicindex.org request 27. The build
is complete. The primary runs it in the candidate `v0.19.0-rc.5`, and no release
carries it.

## Date
2026-11-08

## Context
musicindex.org request 27 asks that the search field of the site find a feed by
its RSS URL. No route of the node gives a feed for a URL. A search with the full
URL in `q` gives no row, because the search index holds words and not URLs.

The node already holds the URLs that name a feed:

| Source | Rows on 2026-11-07 | Replicated |
|---|---|---|
| `feeds.feed_url`, one URL for each feed, `UNIQUE` | 10,710 | Yes, in the feed events |
| `feed_url_observations`, each URL that gave a GUID (ADR 0049 §1) | 12,415 | Yes, `FeedUrlObserved` |
| `feed_copies.url`, the URL of a copy (ADR 0058) | 130 | Yes, `FeedCopyObserved` |

The node compares a URL as an exact text. The stored URLs have few other forms.
In the backup of 2026-11-07 20:56 UTC:

- No stored host has an upper-case letter, and no stored URL has a fragment or a
  port.
- 2 feed URLs use `http`, 1 ends with a slash, and 2 have a query string.
- 1,444 observations hold the short Wavlake form `wavlake.com/feed/<id>`. The
  feed URL of those feeds is the form `wavlake.com/feed/music/<id>`.
- Each of the 130 copy URLs is also an observation of the GUID of its record.

The request gives the example `https://feeds.wavlake.com/feed/music/0d1aa48a-…`.
That host answers `404`, and the node holds no feed with that ID. So the example
gives `404` with each design.

## Decision

### 1. The route and the forms of a URL
`GET /v1/feeds/by-url?url=<url>` gives the feed that the URL names. The node
does not fetch the URL.

The value must be an absolute `http` or `https` URL of at most 2,048 bytes, with
no user name or password. Otherwise the answer is `400`.

The node compares these forms in this sequence. The first match is the answer.

1. The value as given, with no white space at each end.
2. The parsed URL with no fragment. Parsing makes the scheme and the host
   lowercase, and removes a default port.
3. Form 2 with the other one of `http` and `https`.
4. Forms 2 and 3 with the trailing slash of the path added or removed. The root
   path `/` does not change.

The query string and the path keep their text and their case. A `www.` host and
a host with no `www.` are two hosts. The node adds no rule for one host, and the
Wavlake short form matches through its observations.

### 2. The sources of a match
For one form, the node reads three sources in this sequence:

| Sequence | Source | `match` | The feed |
|---|---|---|---|
| 1 | `feeds.feed_url` | `feed_url` | The feed with that URL |
| 2 | `feed_copies.url` | `copy` | The indexed record of the copy |
| 3 | `feed_url_observations.url` | `observed_url` | The feed of the observed GUID |

A form goes through all three sources before the next form starts. A match whose
feed the node no longer holds does not give that feed. When a GUID change retired
it (ADR 0052 §5), the answer is `404` with `superseded_by`, as on
`GET /v1/feeds/{guid}`. Otherwise the next match counts. With no match, the
answer is `404` with `{"error": "feed not found"}`.

### 3. The answer gives the GUID, not the feed
A `200` answer has the usual envelope. Its `data` holds `feed_guid`, `href`
(`/v1/feeds/<guid>`), `match` and `matched_url`, which is the form that matched.
The client reads the feed at `href`.

### 4. A capability names the route
`GET /v1/node/capabilities` lists `feed_by_url`. A node with no such flag answers
the path as a feed read of the GUID `by-url`, which gives `404`. So a client sends
the route only when the flag is listed.

### What this ADR does not decide
- A rule for one host, such as `feeds.wavlake.com` for `wavlake.com`.
- A match by a part of a URL, or by its host. A search with `host` does that
  (ADR 0080 §5).
- A crawl of a URL that the node does not hold.

## Alternatives Considered

### Give the full feed row
This is the form of the request. The route then duplicates the feed read and its
includes, and each change to that read changes two routes. The site already
reads `GET /v1/feeds/{guid}`, so one more request gives the same row.

### A redirect to the feed read
A `307` to `/v1/feeds/<guid>` gives the row in one request for a client that
follows redirects. The client then loses `match`, and cannot tell a copy URL from
the URL of the feed.

### Compare normalized text in SQL
A stored column with a normalized URL, and an index, finds each form in one read.
It needs a migration, a repair for each row and a rule for each new URL. The
stored URLs have almost no other forms, so at most five exact reads give the
same result.

### Follow a GUID change to the new feed
The route could give the new feed in place of `404` with `superseded_by`. The feed
read does not do this, and the two routes must agree.

## Consequences
- The site can find a feed by its URL, and can tell a copy URL from the URL of the
  feed.
- A lookup reads at most five forms, each in three sources. Two sources have an
  index on the URL. `feed_copies` has its key on `(feed_guid, url)`, so a copy
  lookup reads each copy row. The table holds 130 rows, and ADR 0058 §1a limits
  the rows for each GUID.
- A URL with a stored form that is not in §1 does not match. One observation in
  the backup ends with a space.
- No event and no table changes. A primary and a community node can upgrade in
  each sequence. The change is an addition, so it belongs in a MINOR release
  (ADR 0066).

## Invariants
- The route never fetches a URL.
- A form of the value as given comes before each other form.
- The `feed_url` of a feed comes before a copy URL and an observed URL of another
  feed.
- The `404` of the route has the form of the `404` of `GET /v1/feeds/{guid}`.

## Acceptance Criteria

### Mechanical
Tests in `tests/adr0088_feed_by_url_tests.rs` prove each of these:

1. §2 and §3: a `feed_url` gives its feed, `match` `feed_url`, `href` and
   `matched_url`.
2. §1: the other scheme, an upper-case scheme and host, a default port, a trailing
   slash, a fragment and white space each find the stored URL.
3. §1: a different query string, no query string or a different path case gives
   `404`.
4. §2: an observed URL, such as the Wavlake short form, gives `observed_url`.
5. §2: a copy URL gives the indexed record with `copy`, before its observation.
6. Invariants: a `feed_url` comes before an observation of another feed.
7. Invariants: the form as given comes before the other scheme.
8. §2: a URL of a retired GUID gives `404` with `superseded_by`.
9. §2: an unknown URL gives `404` with no `superseded_by`.
10. §1: no `url`, an empty value, a relative URL, `ftp`, a user name and a value
    over 2,048 bytes each give `400`.
11. §4: the capabilities list `feed_by_url`.

The guards of ADR 0044 cover the OpenAPI path and the `FeedUrlMatch` schema.

### Visual
None in this repository. The site owns its search field.

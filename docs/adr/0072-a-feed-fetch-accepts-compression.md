# ADR 0072: A Feed Fetch Accepts Compression

## Status
Proposed on 2026-10-20, for the research request on crawler compression. It
amends ADR 0050 §2 and §6. It supersedes the section "Content hash strategy"
of ADR 0011.

## Date
2026-10-20

## Context

[The research request](../plans/research-crawler-compression-and-value-time-split-request.md)
of 2026-10-20 found that a feed fetch sends no `Accept-Encoding` header. The
crawler declares `reqwest` without the `gzip` and `brotli` features, so it
cannot decode a compressed body. A feed request also sends no `Accept` header.

[The verification record](../reviews/research-crawler-compression-and-value-time-split-verification.md)
confirmed these facts on stophammer-crawler `b142c79`. The two tests of the
request fail on that revision and pass with the fix.

The request gives these measurements from one live sample on 2026-10-20. The
verification did not do the sample again.

- 279 of 355 feeds that answered `200` sent a compressed body when asked.
- A Wavlake body compresses to 0.297 of its plain size. Wavlake has 9,291 of
  the 10,581 feeds in the index.
- One full pass transfers about 49.7 MB plain and about 16.8 MB compressed.

Two statements in the project expect compression. ADR 0054 §2 says
that the 16 MiB limit "applies after decompression". A comment on
`read_capped_body` in `src/crawl.rs` says that each chunk is "already decoded".

The verification found one regression that the request did not examine. An
Apache server with `mod_deflate` adds `-gzip` to the `ETag` of a compressed
body. When the crawler sends that `ETag` back in `If-None-Match`, the server
compares it with the plain `ETag`, finds no match, and answers `200`. On
2026-10-20, two hosts with 155 feeds in the index did this:
`music.behindthesch3m3s.com` and `www.doerfelverse.com`. Each host answered
`304` to the plain `ETag`, and to a list that held both tags.

The fetch cache of ADR 0050 writes the validators only after a `200`. A `304`
does not change them. Thus the stored `-gzip` tag stays until the feed changes.

## Decision

### 1. A feed fetch asks for compression

Each client that fetches a feed accepts `gzip` and `br` bodies, and decodes
them. This applies to each mode that calls `crawl_feed_report`: `feed`,
`refresh`, `gossip` and `import`.

### 2. A feed fetch states the types it accepts

`build_hop_request` in `src/crawl.rs` sets this header on each feed request,
with `User-Agent`:

```text
Accept: application/rss+xml, application/atom+xml, application/xml;q=0.9, text/xml;q=0.9, */*;q=0.8
```

The header is on the request and not on one client, so each mode sends it.

### 3. The snapshot download does not ask for compression

The blocking client of `download_snapshot_archive` in `src/modes/import.rs`
turns off `gzip` and `brotli`. The snapshot is a `.tgz` archive, so HTTP
compression saves almost no bytes and costs CPU on both sides.

### 4. The content hash is over the decoded body

The crawler computes the SHA-256 over the decoded body. That is the body that
the parser reads. A compressed and a plain transfer of the same feed give the
same hash. In the request, 114 of 114 feeds gave the same hash both ways.

The crawler also keeps the decoded body in the fetch cache (ADR 0050 §1).

### 5. A stored `ETag` with a compression suffix is sent with its base tag

This amends ADR 0050 §2.

The cache keeps the `ETag` as the server sent it. A stored `ETag` can end with
`-gzip"` or `-br"`. For such a tag, the crawler sends a list of two tags in
`If-None-Match`: the stored tag, and the same tag without the suffix.

```text
If-None-Match: "1288-63c94b87e7da9-gzip", "1288-63c94b87e7da9"
```

The crawler changes only the request. It does not change the stored value.

A list is permitted by RFC 9110 §13.1.2. A server whose own tag ends with
`-gzip` matches the first tag. An Apache server matches the second tag.

### 6. The report counts a wasted `200`

This amends ADR 0050 §6.

A wasted `200` is a `200` answer to a conditional request, with a body whose
hash is the same as the cached hash. At the end of a batch pass, the crawler
reports the number of wasted `200` answers for each host. The `gossip` mode
prints one line for each wasted `200`, with the host.

A host with many wasted `200` answers sends a validator that does not match.
This shows such a host also when its suffix is not in §5.

## Not Decided Here

- A suffix other than `-gzip` and `-br`. A new suffix needs a record of the host
  that sends it.
- `zstd` and `deflate`. The sample did not measure them.

## Alternatives Considered

### Keep plain fetches

This changes nothing. Most of the bytes of a pass are Wavlake bodies, and
Wavlake makes a new body one or more times in 12 hours (ADR 0050 phase plan). The
bandwidth stays high, and ADR 0054 §2 states a condition that cannot occur.

### Remove the suffix from the stored `ETag`

This works on the two Apache hosts. But the cache then does not hold the value
that the server sent. A server whose own tag ends with `-gzip` then does not
match, and gives a wasted `200` on each fetch.

### Send only `If-Modified-Since` when the cache has a date

Both Apache hosts answered `304` to this. But a date has a resolution of one
second, and some hosts send an incorrect date. The crawler then does not use the more
accurate validator for each host that sends both values.

### Ask for compression from a host only after a test

This has no regression on a host. But the crawler must keep a record for each
host, and test it again when the host changes.

## Consequences

- A full pass transfers fewer bytes. The request estimates 66% less.
- The crawler and the server use more CPU to decode and encode a body.
- A decoded body has no `Content-Length`, so the check before the first chunk
  does not apply. The chunk limit of ADR 0054 §2 applies to the decoded bytes.
- The node does not change. Its content-hash check sees the same hash for the
  same feed.
- The first fetch after the deploy sends the plain `ETag` from the cache.
  The verification found that both Apache hosts answer `304` to it.

## Invariants

- A compressed and a plain transfer of the same feed give the same content hash.
- The fetch cache keeps each `ETag` as the server sent it.
- The 16 MiB limit applies to the decoded bytes.
- The snapshot download sends no `Accept-Encoding`.

## Guards

- The two crawler tests of the request: a feed request sends
  `Accept-Encoding` and `Accept`, and a gzip body parses as the plain body.
- A crawler test: a stored `ETag` that ends with `-gzip` gives the list of §5,
  and the stored value does not change.
- A crawler test: the snapshot client sends no `Accept-Encoding`.

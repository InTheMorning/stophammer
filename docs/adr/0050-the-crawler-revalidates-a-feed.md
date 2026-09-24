# ADR 0050: The Crawler Revalidates A Feed

## Status
Accepted

## Date
2026-09-24

## Context
Each crawler fetch downloads the full body of a feed. The node then often
answers that the content did not change. Most of the index is on one host,
Wavlake, and that host answers HTTP `429` when a crawler sends too many
requests.

On 2026-09-24 two passes ran over the index. The full corrective pass of ADR
0047 fetched about 15,000 feeds. The pass over the publisher feeds of ADR 0049
fetched about 9,900. At a host delay of 3 seconds, each pass took several hours.
ADR 0049 names conditional GET as an open question and leaves it to a separate
decision. This is that decision.

On 2026-09-24 Stophammer sent one GET and then one conditional GET to a feed on
each of four hosts. Each host answered the conditional GET with `304` and an
empty body:

| Host | Validator that it sends | Other headers |
|---|---|---|
| Wavlake | `ETag` | `cache-control: public, max-age=43200`, Vercel CDN |
| RSS Blue | `Last-Modified` | |
| Fountain | `ETag` and `Last-Modified` | |
| JustCast | weak `ETag` (`W/"…"`) | Cloudflare, `must-revalidate` |

A `304` tells the crawler that the body did not change since its last fetch. It
does not tell the crawler that the node needs no body:

- A normal crawl needs no ingest after a `304`. The node already holds that
  content.
- A corrective pass (`--force`) needs the body, because it runs a changed ingest
  rule on each feed again. A `304` carries no body.

So conditional GET alone does not shorten a corrective pass. It does so only
when the crawler also keeps the last body.

A local snapshot of the feeds from 2026-04-03 exists, but it cannot replace a
fetch. Its bodies are months old, and a replay would set each feed back to that
content. A kept body is different: a `304` proves that it is current.

Three fetch paths exist. The batch path (`feed` and `refresh`), the `gossip`
mode and the `import` mode all fetch through `crawl_feed_report` in
`stophammer-crawler/src/crawl.rs`. The `ndjson` mode does not fetch.

## Decision
The crawler keeps, for each URL that it fetches, the validators and the last
body. It sends a conditional GET, and after a `304` it uses the kept body.

### 1. The fetch cache
The crawler keeps one SQLite file, `/data/feed_cache.db` by default, in its own
volume. All modes read and write it. A row has these values, keyed by the URL
that the crawler requested:

- the `ETag` value, unchanged, including a weak `W/` prefix,
- the `Last-Modified` value, unchanged,
- the SHA-256 of the body,
- the body, compressed with gzip,
- the final URL after redirects,
- the time of the fetch,
- the last answer of the node: accepted, no change, or rejected with its reason.

The file uses WAL mode and a busy timeout. A write is one short transaction. Two
crawler containers can write at the same time.

### 2. The request
When the cache holds a row for a URL, the crawler sends `If-None-Match` with the
stored `ETag`, and `If-Modified-Since` with the stored `Last-Modified`. It sends
each header for which it has a value.

### 3. The answer
- **`200`.** The crawler parses and submits the body as today. Then it replaces
  the row with the new validators, body, hash and node answer.
- **`304` in a normal crawl.** The crawler sends no ingest, and it reports the
  outcome as no change. The node answer in the row does not change. One
  exception: when the row holds no node answer, or its last answer is an ingest
  error, the node does not hold that content. The crawler then submits the
  kept body, as in a `--force` pass.
- **`304` in a `--force` pass.** The crawler submits the kept body, with the
  kept hash, as if it had just fetched it. Then it records the node answer.
- **`304` with no kept body.** The crawler cannot trust the answer. It sends one
  GET with no conditional header, and then it continues as for `200`.
- **Any other status.** The crawler behaves as today, and it does not change the
  row.

### 4. A rejected feed
A feed that the node rejected also gets a row. A normal crawl sends no ingest
after a `304`, even when the last answer was a rejection. A changed ingest rule
reaches such a feed through a `--force` pass, as ADR 0047 states.

### 5. A pass that ignores the cache
A command-line flag, `--no-revalidate`, makes a pass send no conditional header.
The crawler still writes the rows. This is for a host that sends a wrong `ETag`.

### 6. The report
At the end of a batch pass, the crawler reports how many fetches gave `200`,
`304` and `429`. These numbers answer the open question below.

### 7. The node does not change
The node keeps its content-hash check as a second guard. It does not know about
the fetch cache. ADR 0006 keeps the crawler an untrusted client, and the cache is
the crawler's own data.

## Not Decided Here
A `refresh` pass could send no request at all for a URL that it fetched within
the host's `max-age`, which is 12 hours for Wavlake. That saves requests only if
a `304` counts against the `429` limit. The report of decision 6 measures that.
A later decision can use the measurement. The `gossip` mode must never skip a
fetch for `max-age`, because a podping says that the feed changed.

## Alternatives Considered
- **Conditional GET with no body.** Normal crawls save their bodies, but a
  corrective pass fetches each body as today. The long passes of 2026-09-24
  stay as long as they were.
- **Replay the snapshot of 2026-04-03.** It sets each feed back to its April
  content.
- **Keep the cache in the node.** The node would store the state of a crawler,
  and it would need a new route to give that state back. More than one crawler
  can run, and each has its own view of the hosts.
- **One cache for each mode.** A `refresh` pass could not use what `gossip`
  fetched.
- **Keep the cache in `feed_skip.db`.** One file fewer, but it mixes the skip
  memory with the fetch cache, and a change to one then touches the other.

## Consequences
- A normal crawl of an unchanged feed sends a request with no body back, and
  no ingest.
- A corrective pass transfers almost no feed body. It still sends one request
  for each feed.
- The crawler volume grows. The snapshot of 2026-04-03 held 6,896 bodies in
  160 MB uncompressed. With gzip, 10,000 bodies take roughly 30 to 50 MB.
- A host that keeps the same `ETag` for changed content hides the change until
  a pass with `--no-revalidate`.
- A CDN with a `max-age` can give an old body for a time after a change, with or
  without this decision.
- The work is in `stophammer-crawler` only. Its commits name this ADR.

## Open Question
Does a `304` count against the `429` limit of Wavlake? Decision 6 measures it
during the first pass that runs with this change.

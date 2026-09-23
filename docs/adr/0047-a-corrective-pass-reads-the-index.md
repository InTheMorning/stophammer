# ADR 0047: A Corrective Pass Reads The Index

## Status
Accepted

## Date
2026-09-22

## Context
Ingest logic changes. ADR 0043 changed how a feed publication date is derived,
so each stored feed must be read again before the index agrees with the current
code. This recurs: a parser rule, a verifier or a claim path changes, and the
stored result of every earlier ingest is stale.

The crawler has no mode for this. `import` walks a PodcastIndex snapshot and
`gossip` follows podping notifications. Both are incremental by design: they
find the feeds that are new or that changed. A corrective pass needs the
opposite group, the feeds that are neither.

Today the operator builds a URL list outside the crawler for each pass. The
rule that a pass covers the feeds the index holds lives in that list rather
than in the tool, so it is rebuilt, and can be got wrong, every time.

## Decision
The crawler gains a corrective pass. Its corpus is the node's feed list, read
from `GET /v1/feeds/recent` through that route's cursor.

1. The index is the definition of the set of feeds the node holds. A corpus
   read from it is therefore complete by construction. A record derived from
   the crawler's own activity cannot give that guarantee.
2. The pass asks for `medium=all`. The feed list filters to the music medium
   by default, and the index also holds publisher and `musicL` feeds, so a
   pass that takes the default covers one medium and omits the rest without
   saying so.
3. The pass reads a public route that needs no credential. It gains no
   authority from the read, and ADR 0006 keeps the node's distrust of a
   crawler submission unchanged.
4. The pass derives the query origin from `INGEST_URL`, which already names
   the node and always ends in the ingest path. The query route is on that
   same origin, so the pass adds no required configuration.
5. The pass reuses the existing pipeline through `batch::run_urls`, so the
   host interleave, the concurrency pool and the failed-feeds output all
   apply.
6. The operator combines the pass with `--force`, because a corrective pass
   needs the content-hash check bypassed.

## Alternatives Considered

### Build the corpus from the crawler's own records

`import_feed_memory` and `feed_outcomes` record what the crawler attempted and
what the node told it. Neither records what the index holds. A feed the node
accepted through a path that wrote no record is absent from both, and the size
of that gap cannot be measured from inside the crawler. A corpus built this way
is incomplete without saying so. Rejected.

### Keep building the list outside the crawler

This is the present practice, and it gives the right answer because it reads
the same index. It is rebuilt for each pass, and its scope rule lives in a
script rather than in the tool. Rejected as the durable form. It stays valid
for a one-off list.

## Consequences

- The crawler reads from the node for the first time. It needs no new
  configuration, because `INGEST_URL` already names the node.
- A corrective pass becomes one command with no external list.
- The corpus is as current as the index. A feed added while a pass runs belongs
  to the next pass.
- The node serves one page request for each 100 feeds. That cost is small
  beside the fetches the pass then performs.

## Invariants

- The corpus comes from the node's feed list, never from a crawler-local
  record.
- A corrective pass fetches only feeds the index holds.
- The pass sends the node no request it does not already serve to any client.

## Guards

A partial corpus is silent, so paging earns a test.

- A pass over a paginated feed list visits every page.
- A page that fails stops the pass. It never continues with a short corpus.

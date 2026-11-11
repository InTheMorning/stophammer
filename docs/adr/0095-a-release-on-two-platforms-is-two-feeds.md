# ADR 0095: A Release On Two Platforms Is Two Feeds

## Status
Proposed on 2026-11-11. It answers L1 to L4 of
[the research request of 2026-11-03](../plans/research-lookalike-feeds-and-first-seen-request.md).
It also answers M2 of [the research request of 2026-11-04](../plans/research-feed-move-confirmation-request.md).
The operator accepts, changes or rejects it.

This ADR changes no code and no schema. It keeps the copy rule of ADR 0058 and
the move rules of ADR 0052 and ADR 0083.

## Date
2026-11-11

## Context
ADR 0058 records a copy only when a second URL declares the `podcast:guid` of a
stored feed. The two requests ask for more data in the operator copy queue:

- L1: read the PodcastIndex lookalike pairs.
- L2: find a copy that declares a different `podcast:guid`.
- L3: put the two feeds of a pair in sequence by the PodcastIndex first-seen times.
- L4: compare the payment routes of the two feeds of a pair.
- M2: store the tag of a mirror that names itself.

The data of the requests gives these facts:

- **Our own match finds more pairs than PodcastIndex.** Of 99,883 PodcastIndex
  pairs, 23 have a URL that the node holds. A match on the artist, the title, the
  track titles and the track lengths finds 150 pairs in the backup of 2026-11-07.
  The PodcastIndex detector finds only one Haleen pair of 40. It needs shared item
  GUIDs, or times less than 48 hours apart.
- **The PodcastIndex times do not show the original.** The API gives no
  `createdOn` and no `valueCreatedOn` on a feed. `dateCrawled` is the first time
  that one feed ID gave one item GUID, and it can be months after publication. For
  "Damn Your Idols", `datePublished` and `dateCrawled` give opposite sequences.
- **A pair of class A is two correct uploads.** On 2026-11-11 the operator stated
  that Haleen uploads to Fountain and to Wavlake. Each platform makes the
  `podcast:guid` from its own feed URL. Each feed pays the artist through its own
  platform.
- **The operator can already see a pair of M2.** The body at the second URL writes
  a `feed_copies` row, and `GET /v1/copies` gives its URL. Only the tag of that
  body is not stored. No incident needed that tag.

## Decision

### 1. The node does not join two feeds by a match of content
Two feeds with different `podcast:guid` values are two records, also when they
carry the same release. The node does not merge, hide, retire or move either
feed because of a match of titles, lengths or a PodcastIndex label. A choice of
one feed decides who gets paid, and only the artist or the operator can make it.
This answers L2.

### 2. The node does not read the PodcastIndex lookalike files
The crawler gets no pass for `lookalike_feeds_daily.csv` or
`lookalike_feeds_all.csv`. The files find few pairs of this index, and they
cannot find the pairs of class A. This answers L1.

### 3. The node shows no PodcastIndex first-seen time
The copy queue gets no `createdOn` or `dateCrawled`. The API does not give
`createdOn`, and `dateCrawled` does not show which feed came first. This answers
L3.

### 4. The copy row does not store the tag of a mirror
`feed_copies` keeps its columns. The operator reads the URL of the second feed in
`GET /v1/copies`, and can read its body. This answers M2.

### 5. No route comparison for a pair with different GUIDs
ADR 0058 already compares the routes of a copy with the same `podcast:guid`. A
pair of class A always pays two different platform addresses, so a comparison
marks each pair and finds no fault. This answers L4.

### What this ADR does not decide
A client can show "also on another platform" for feeds with the same artist and
title. That is a feature for a client request, not a correction of the index.

## Alternatives Considered

### Import the lookalike pairs as information for the operator
This is the form of L1. It adds a crawler pass, a table and a queue design for 23
pairs. The pairs of class A stay invisible to it.

### A match of our own that writes a copy row
The match of the artist, the title and the track lengths finds 150 pairs. Each
pair needs a decision of the artist, and the known pairs are correct uploads. A
copy row for each pair gives the operator work with no fault to correct.

### Store the tag of a mirror (M2)
This adds one column to `feed_copies` and a write in the copy path. No incident
needed the value.

## Consequences
- The copy queue keeps the rule of ADR 0058: a copy declares the same
  `podcast:guid`.
- A copy with a new GUID and changed payment routes stays invisible to the node.
  An artist or a listener reports it, and the operator acts on the report.
- The research requests of 2026-11-03 and 2026-11-04 have no open section after
  this ADR is accepted.

## Invariants
- A match of content never changes a feed record. Only the rules of ADR 0052,
  ADR 0058, ADR 0083 and an operator action change it.

## Acceptance Criteria

### Mechanical
None. This ADR changes no code. The invariant holds today, and no incident broke
it, so it has no test.

### Visual
None.

# ADR 0067: A Gone Source Retires Its Feed

## Status
Accepted on 2026-09-27

## Date
2026-09-27

## Context
When a host removes a feed, the index keeps the record. The crawler gets
`404` from the source URL, logs a fetch error and sends nothing to the node.
Only the operator can remove the record.

The second `refresh` pass of 2026-09-27 got 373 fetch errors. Many are
Wavlake `404` answers with the body `Not found`. Wavlake answers that way for
a music feed that its artist removed.

ADR 0057 retires a record when its source URL states `podcast:block`. A removed
feed gives no body, so that rule cannot see it. ADR 0051 lets only the source
URL change a record.

A `404` is not always a statement of the host. A fault of a host or of its CDN
can answer `404` for a short time, and for many feeds at once. A `410` means
"Gone": the host states that it removed the resource on purpose.

## Decision

### 1. The crawler reports a gone source

The `feed`, `refresh` and `gossip` modes send a report for a gone answer. A
gone answer is `404` or `410` from a URL, with no redirect before it. The
report is an ingest request with that status and with no `feed_data`. The `import` mode sends no report, because it
fetches feeds that the index does not hold.

### 2. The node counts the answers

The node accepts a report only when:

- the URL is the stored source URL of a record (ADR 0051), and
- the status is `410`, or the status is `404` and the host of the URL is in
  the setting `SOURCE_GONE_HOSTS`.

The setting is a comma-separated list of host names. It is empty by default,
so a `404` retires nothing until the operator names a host. The operator of
`api.musicindex.org` sets `SOURCE_GONE_HOSTS=wavlake.com`.

For an accepted report, the primary keeps the time of the first gone answer
of the record. The row is local to the primary and makes no event. An ingest
of a body from the source URL deletes the row.

### 3. The second answer retires the record

When an accepted report comes 24 hours or more after the first gone answer
of the record, the node retires the record. It uses the transaction and the
signed `FeedRetired` event of ADR 0053 task 003, with the reason
`source_gone`, and it writes no row in `feed_blocks`. Community nodes apply the
retirement.

The response of the first report is `accepted: false` with the reason
`source_gone_observed`. The response of the retirement is `accepted: false`
with the reason `source_gone`, and the ID of the `FeedRetired` event in
`events_emitted`. A report that the node does not accept gets the reason
`source_gone_ignored` and changes nothing.

### 4. The feed can come back

The node writes no durable block. When the host serves the feed again, the
next crawl that reaches it admits it as a new feed, as ADR 0057 §4 gives.

### 5. The contract

- The ingest request does not change. `feed_data` is already optional.
- `docs/API.md` and the OpenAPI document give the three reasons.
- `SOURCE_GONE_HOSTS` is in `docs/operations.md` and the example env file
  of the primary.

## Alternatives Considered

### Retire on the first answer
A fault of a host that answers `404` for many feeds would retire each of them.
Each would come back only on its next crawl. Rejected.

### Name Wavlake in the code
A second host would need a change of the code and a release. The code would
also hold a host rule, against the direction of ADR 0049. Rejected for a
setting.

### Count in the crawler
The crawler would send one report when its own count holds. The node would
then trust a count that it cannot check, and each crawler store would need the
same history. The node already decides each retirement. Rejected.

### Retire a `404` from any host
Many hosts answer `404` for a fault of their own set-up, for example after a
move to a new path. A move with a redirect is ADR 0052. A move without one is
a fault that the publisher can correct. Rejected until a host is known to use
`404` for a removal.

## Consequences

- A Wavlake album that its artist removed leaves the index about one day
  after two crawls see it gone, with no operator step.
- The time to retire depends on the crawls. Wavlake sends no podping, so the
  second answer comes mostly from the next `refresh` pass.
- The primary gets one small ingest request for each gone fetch.
- The node, the crawler and the documentation change. Release 0.3.0 carries
  them. ADR 0066 makes it a MINOR change.

## Invariants

- Only an answer from the stored source URL counts.
- One `404` or one `410` never retires a record.
- A `404` from a host that is not in `SOURCE_GONE_HOSTS` changes nothing.
- This rule writes no row in `feed_blocks`.
- An operator block of ADR 0053 is checked before this rule.

## Non-Goals

- A retirement for a timeout, a `5xx` or a DNS failure.
- The removal of one track.
- A retirement from a URL that is not the source URL, for example a copy of
  ADR 0058.

## Guards

- A `404` from a listed host, then a second one 24 hours later, retires the
  record with `source_gone`.
- A second `404` less than 24 hours after the first does not retire it.
- A body from the source URL between the two answers resets the count.
- A `404` from a host that is not listed changes nothing.
- A `410` from any host counts.
- A report for a URL that is not a stored source URL changes nothing.

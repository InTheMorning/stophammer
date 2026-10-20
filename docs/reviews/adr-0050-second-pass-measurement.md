# ADR 0050: Measurement Of The Second Pass

This record is advisory. It states no binding rule.
[ADR 0050](../adr/0050-the-crawler-revalidates-a-feed.md) owns the rule. This
record gives the measurement that keeps the open question of ADR 0050 open.

## The Report

The second `refresh` pass after the deploy of ADR 0050 ended on 2026-09-27.
Its report:

```text
fetch: ok=9237 not_modified=1175 rate_limited=2 other=373
refresh: publisher links: listed=8248 guid=768 feed_url=7322 unresolved=158
```

The index held 10,424 feeds: 9,150 at `wavlake.com` and 1,274 at other hosts.
Almost each `304` thus came from a host other than Wavlake, and almost each
Wavlake feed gave `200`.

## The Cause

The cause is at Wavlake. On 2026-09-27 a request to a Wavlake music feed
gave `ETag`, and a request with `If-None-Match` gave `304`. But the body gives
the time of its build as `lastBuildDate`, and the CDN keeps a build for at most
12 hours (`cache-control: public, max-age=43200`). Each new build has a new
body and a new `ETag`, also when no item changed. A pass that comes more than
12 hours after the last fetch of a feed thus gets `200`.

## The Results

- **The open question stays open, and it has no effect now.** The pass got
  almost no `304` from Wavlake, so it cannot show whether a `304` counts
  against the limit. The pass got 2 `429` answers in about 10,800 fetches, so
  the limit does not stop a pass at the current concurrency.
- **A Wavlake body with a new `lastBuildDate` makes no event.**
  `feed_fields_changed` in `src/db.rs` does not compare `last_build_date`.
  The cost of a new build is the fetch and the ingest, not the event log.
- **A conditional GET saves a body only from a host with a stable `ETag`.** On
  this pass, that was 1,175 feeds.
- `other=373` holds the fetch errors. The end of the log shows Wavlake `404`
  answers, which are removed feeds.

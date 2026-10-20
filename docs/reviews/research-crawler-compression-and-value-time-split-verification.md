# Verification Of The Research Request On Compression And Value Time Splits

## Purpose

This document records the checks that Stophammer made on
[the research request](../plans/research-crawler-compression-and-value-time-split-request.md).

It holds the facts that the checks found. It states no rule.

## Method And Date

The checks ran on 2026-10-20. They used these sources:

- stophammer-crawler `b142c79` and stophammer-parser `9021262`, in a copy
  that is not in the repositories.
- The evidence folder `sources/captures/2026-10-20-stophammer-claim/` of the
  research repository.
- The production backup `stophammer-20261019T000753Z.db`, opened read-only.
- The valueTimeSplit specification at podcast-namespace commit `d4036dc`.
- Conditional requests to one feed on each of 9 feed hosts. Each feed got at
  most 6 requests.

## Result Summary

| # | Statement in the request | Result |
|---|---|---|
| 1a | The crawler sends no `Accept-Encoding` header and cannot decode a compressed body | Correct |
| 1b | `Cargo.lock` contains no compression crate | Not correct. `flate2` is in the lock file for the `import` mode. `reqwest` does not use it. |
| 1c | The two crawler tests fail before the fix, and 253 tests pass after it | Correct |
| 1d | The fix passes `clippy` and `fmt` | Correct |
| 1e | The `Accept` header of the fix does not apply to the `gossip` mode | Correct |
| 1f | Compression causes no incorrect change of the content hash | Correct for the hash. The request did not examine revalidation. See [Compression Stops Some Revalidation](#compression-stops-some-revalidation). |
| 2a | The parser drops each split with `remotePercentage` | Correct. [ADR 0011](../adr/0011-rss-crawler.md) decides this skip. The request does not cite it. |
| 2b | The specification has no `split` attribute on `valueTimeSplit`, and the default share is 100 | Correct |
| 2c | The parser drops a split with `valueRecipient` children | Correct |
| 2d | The parser drops a fractional `startTime` | Correct. The specification gives "seconds" and does not limit the value to an integer. |
| 2e | Five parser tests fail, and the control test passes | Correct |
| 2f | The backup has 0 rows in `value_time_splits` and 27,589 tracks | Correct |
| 2g | The Podhome feed has three splits, and each split has `remotePercentage` | Correct. The feed is in the index, with GUID `b39fc3b2-2742-5944-a800-e87c42a093a3`. |
| 3 | The index has 10,581 feeds, and 9,291 are on `wavlake.com` | Correct |

The check did not do again the live sample of 385 feeds or the hash check of 114
feeds. The bandwidth figures in the request are not verified.

## Compression Stops Some Revalidation

[ADR 0050](../adr/0050-the-crawler-revalidates-a-feed.md) sends
`If-None-Match` and `If-Modified-Since` from the fetch cache. The crawler sends
both headers when the cache has both values.

An Apache server with `mod_deflate` adds `-gzip` to the `ETag` of a compressed
body. It then does not match that `ETag` on a subsequent request. The check
found this on two of the 9 hosts:

| Host | Feeds in the backup | Compressed `ETag` sent back | Plain `ETag` sent back | Both headers sent back |
|---|---:|---|---|---|
| `music.behindthesch3m3s.com` | 105 | `200` | `304` | `200` |
| `www.doerfelverse.com` | 50 | `200` | `304` | `200` |

With only `If-Modified-Since`, each host gave `304`. With both headers, the
`ETag` mismatch decides, and each host gave `200`.

Thus the minimal fix changes these feeds from `304` to `200` on each fetch, for
as long as the crawler stores the compressed `ETag`. The node finds the same
content hash, so no record changes. But each fetch transfers the body and posts
an ingest.

The other 7 hosts gave `304`, or sent no `ETag`:

- `feeds.fountain.fm` and `wavlake.com` send a weak `ETag` (`W/`) and match
  it.
- `serve.podhome.fm` sent no compressed body and kept its `ETag`.
- `epochradio.com`, `feeds.rssblue.com`, `headstarts.uk` and
  `publishers.rssblue.com` sent no `ETag`.

The check examined only 9 hosts. The number of other hosts with this server
behavior is not measured.

## Other Findings In The Crawler

- `build_hop_request` in `src/crawl.rs` sets `User-Agent` on each feed request.
  An `Accept` header at that point covers each mode that calls
  `crawl_feed_report`, `gossip` included.
- The `reqwest` feature applies to each client in the crate. The blocking
  client of `download_snapshot_archive` in `src/modes/import.rs` then also
  tells the host that it accepts compression. The check did not examine the snapshot host.
- The content hash is the SHA-256 of `raw_xml` in `src/crawl.rs`. With the
  feature, `raw_xml` is the decoded body, so the hash is the same as for a
  plain body.
- A decoded body loses `Content-Length`. The chunk limit of
  `read_capped_body` then applies to the decoded bytes, as ADR 0054 §2
  requires.

## The ADR 0011 Rule

ADR 0011 is Accepted. Its section on `remotePercentage` says that a split with
`remotePercentage` "instead of `split`" is skipped. Its cause is that each play
must calculate the revenue.

The specification does not agree with this premise. It has no `split`
attribute on `valueTimeSplit`. `remotePercentage` is a fixed share with the
default 100. Each play uses the same share.

ADR 0011 also states the raw response bytes as the input of the hash. Other
parts of it give a Bun crawler with no cache and no redirect handling. The
crawler is Rust today, it keeps a fetch cache (ADR 0050), and it follows
redirects (ADR 0052).

## Open Questions

- The request asks for an ADR on the storage of a value time split. That ADR
  supersedes the `remotePercentage` rule of ADR 0011.
- The compression change needs a decision on the `-gzip` `ETag`. This record
  gives no decision.

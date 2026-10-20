# Research Request: Crawler Compression And Value Time Splits

## Status

Open - 2026-10-20. This request comes from the research repository at
`~/build/musicindex-research`. That repository has no Git remote. Its index is
`AGENTS.md`. The background is `research_notes/aggregation/podcast-index-pipeline.md`
in that repository.

This document binds nothing in Stophammer. It asks Stophammer to examine two
defects and decide. Stophammer records each decision in its own ADR.

The evidence is in `sources/captures/2026-10-20-stophammer-claim/` in the research repository.
Each path in this document that starts with `sources/captures/2026-10-20-stophammer-claim/` refers to that folder.

## Purpose

This request asks for two changes.

| # | Change | Repository | Evidence |
| --- | --- | --- | --- |
| 1 | Accept compressed feed bodies, and send an `Accept` header | stophammer-crawler | 385 live feeds, 114 hash checks, 2 tests |
| 2 | Keep each `valueTimeSplit` that the specification permits | stophammer-parser | Production snapshot, 1 live feed, 6 tests |

Change 1 is small and has a tested fix.
Change 2 needs a model decision, so this request gives tests and no fix.

## Evidence Limits

- The index counts come from `stophammer-20261019T000753Z.db`, a backup from 2026-10-19 00:07Z. The deployed data can be different.
- The examined code is stophammer-crawler `b142c79` and stophammer-parser `9021262`, from GitHub `main` on 2026-10-20. The deployed revision is unverified.
- The live sample ran one time, from 18:34Z to 18:37Z on 2026-10-20. Hosts can change their behavior.
- This research did not measure the number of fetches of each feed in a day. Thus the bandwidth figures are for one full pass.

The observations record `sources/captures/2026-10-20-stophammer-claim/observations.json` gives each count and its source.

## 1. The Crawler Cannot Use Compression

**What happens.**
The crawler declares `reqwest` with `default-features = false` and without the `gzip` or `brotli` feature (`Cargo.toml:11`).
Its `Cargo.lock` contains no compression crate.
Thus a feed request sends no `Accept-Encoding` header, and the crawler cannot decode a compressed body.
The request also sends no `Accept` header.

The design expects compression now.
[ADR 0054](../adr/0054-a-fetch-reaches-only-public-feed-hosts.md) §2 says that the 16 MiB limit "applies after decompression".
The code comment at `crawl.rs:302` says that each chunk is "already decoded".
The two statements are not correct with the current features.

**The numbers.**

| Measurement | Result |
| --- | ---: |
| Feeds in the index | 10,581 |
| Feeds on wavlake.com | 9,291 (87.8%) |
| Sampled feeds | 385 |
| Feeds that answered 200 to the two requests | 355 |
| Responses compressed when asked (`gzip` 143, `br` 136) | 279 of 355 |
| Sampled hosts that compressed | 55 of 80 |
| Feeds on hosts that did not compress | 123 (1.2% of the index) |
| Compressed size of a Wavlake feed, as a fraction of plain size | 0.297 |
| Sample bytes, plain and compressed | 4,755,260 and 3,088,961 |
| One full pass, weighted by host, plain | approximately 49.7 MB |
| One full pass, weighted by host, compressed | approximately 16.8 MB (66% less) |

The sample saves less than the weighted figure.
Some large feeds come from hosts that do not compress.
An example is a 466,654-byte feed from `stream.wiirdo.com`.

**The hash stays stable.**
[ADR 0011](../adr/archive/0011-rss-crawler.md) computes the content hash over the response body, and the node uses that hash to find unchanged feeds.
This research fetched 114 feeds on all 55 compressing hosts three times: plain, compressed and decoded, then plain again.
Each plain body stayed the same between the two plain requests.
The decoded body had the same SHA-256 as the plain body in 114 of 114 feeds.

Thus compression caused no incorrect change in this check.
The text of ADR 0011 ("raw response bytes") can require a clarification for a decoded body.

**The tests.**
The crawler tests in `sources/captures/2026-10-20-stophammer-claim/tests/crawler-claim-tests.diff` add two tests to `src/crawl.rs`.
They use the production client from `build_feed_fetch_client`.

| Test | Before the fix | After the fix |
| --- | --- | --- |
| `claim_feed_request_advertises_compression_and_feed_types` | Fails: `accept-encoding sent: ""` | Passes |
| `claim_gzip_encoded_feed_is_decoded_and_accepted` | Fails: `ParseError("malformed XML: unknown token at 1:1")` | Passes |
| All other crawler tests | 251 pass | 251 pass |

**The fix is small.**
The minimal fix in `sources/captures/2026-10-20-stophammer-claim/tests/crawler-minimal-fix.diff` adds `gzip` and `brotli` to the `reqwest` features.
It also adds a default `Accept` header to `build_feed_fetch_client`.
It changes 8 lines.
With the fix, 253 of 253 tests pass.
`cargo clippy --all-targets -- -D warnings` and `cargo fmt -- --check` exit with 0.

The gossip mode builds its own client at `gossip.rs:26`.
The feature change covers it, but the `Accept` header in the minimal fix does not.

**Request.**
Enable compressed responses for each feed fetch client, and send an `Accept` header for feed types.
Keep the content hash over the decoded body, and write that in the ADR.

## 2. The Parser Drops Value Time Splits

**What happens.**
`extract_value_time_splits` in `src/engine.rs` has three defects.

| Line | Code | Effect |
| --- | --- | --- |
| `engine.rs:698` | `if vts.attribute("remotePercentage").is_some() { continue; }` | Each split with `remotePercentage` is dropped |
| `engine.rs:702` | `startTime` must parse as `i64` | A fractional `startTime` is dropped |
| `engine.rs:714` | `split` comes from a `split` attribute, with 0 as the default | The specification defines no `split` attribute on this element. Its default share is 100. |

The function also requires a `remoteItem` child.
The specification permits `valueRecipient` children as an alternative.
Those splits are dropped too.

The [valueTimeSplit specification](https://github.com/Podcastindex-org/podcast-namespace/blob/d4036dcb1047ad654682e1142734e3acf2c13f37/docs/tags/value-time-split.md) at commit `d4036dc` has two examples.
The first uses `remotePercentage="95"`, and the second uses `valueRecipient` children.
The parser keeps 0 of the 4 splits in these examples.
The existing regression test `vts_reads_remote_item_child` passes only because it uses `split="50"`.

**The numbers.**

| Measurement | Result |
| --- | ---: |
| Rows in `value_time_splits` in the production snapshot | 0 |
| Tracks in the production snapshot | 27,589 |
| Sampled feed bodies with `valueTimeSplit` | 1 of 356 |
| `valueTimeSplit` elements in that feed | 3 |
| Of these, elements with `remotePercentage` | 3 |
| Splits in the `stophammer-parse` output for that feed | 0 |

The feed is [The Satellite Skirmish: Boost Recap](https://serve.podhome.fm/rss/b39fc3b2-2742-5944-a800-e87c42a093a3), a V4V music show with `medium` set to `music`.
Its body is `sources/captures/2026-10-20-stophammer-claim/podhome-satellite-skirmish.xml`, and the parser output is `sources/captures/2026-10-20-stophammer-claim/podhome-satellite-skirmish.parsed.json`.

This defect is rare in the index today.
But the parser dropped each observed split, and the index cannot show this.
The table stays empty, and no warning occurs.

**The tests.**
The parser tests in `sources/captures/2026-10-20-stophammer-claim/tests/claim_value_time_split.rs` go in `tests/`.
The fixture goes in `tests/fixtures/podhome_satellite_skirmish.xml`.

| Test | Result on `9021262` |
| --- | --- |
| `spec_remote_example_keeps_both_splits` | Fails: 0, expected 2 |
| `spec_local_example_keeps_both_splits` | Fails: 0, expected 2 |
| `fractional_start_time_is_kept` | Fails: 1, expected 2 |
| `absent_remote_percentage_means_100` | Fails: stored `[0, 0]` |
| `podhome_feed_keeps_its_three_splits` | Fails: 0, expected 3 |
| `control_non_spec_split_attribute_parses` | Passes |
| All other parser tests | 112 pass |

The control test shows that the test harness and the parser call operate correctly.

**Why this request gives no fix.**
`IngestValueTimeSplit` and the `value_time_splits` table store one integer `split` and one remote item.
They cannot store a percentage with its default, a `remoteStartTime`, or a list of local recipients.
A correct fix changes the model, the schema, and the node.
That decision belongs in an ADR.

**Request.**
Decide how the index stores a value time split, with the two forms of the specification.
Use these tests as the acceptance tests, and adjust their assertions to the chosen model.

## Items Not Requested

This research also checked these items and asks for no change for them.

| Item | Result |
| --- | --- |
| `customKey` on `podcast:value`, as the Podtunes generator writes it | 0 of 356 sampled feeds |
| `method="lnaddress"` on `podcast:value`, which the parser does not read | 78 of 356 feeds. The recipient `type` already carries the address type. |
| Other proposals in the pipeline note | No measurement yet |

## Reproduce The Evidence

The scripts are in `sources/captures/2026-10-20-stophammer-claim/scripts/`.
`measure.py` and `hashcheck.py` fetch live feeds. Use them politely and only when necessary.

1. Put a read-only copy of the snapshot at `sh.db`, in the folder that contains `scripts`.
2. Go to the `scripts` folder.
3. Run `python3 measure.py`. It writes `results.json` and the bodies in that folder.
4. Run `python3 analyze.py` for the compression figures.
5. Run `python3 hashcheck.py` for the hash check.

To run the parser tests:

1. Copy `claim_value_time_split.rs` to `stophammer-parser/tests/`.
2. Copy the feed body to `stophammer-parser/tests/fixtures/podhome_satellite_skirmish.xml`.
3. Run `cargo test --test claim_value_time_split`.

To run the crawler tests:

1. Apply `crawler-claim-tests.diff` in `stophammer-crawler`.
2. Run `cargo test claim_`. The two tests fail.
3. Apply `crawler-minimal-fix.diff`, then run `cargo test`. All tests pass.

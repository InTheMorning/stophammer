# Research Request: The Search Index And The Search Contract

## Status

Answered in part - 2026-10-23. ADR 0078 (Accepted) answers the four defects
and the first change of priority 1. The other changes have no decision.

Received - 2026-10-23. This request comes from the research repository at
`~/build/musicindex-research`. That repository has no Git remote. Its index is
`AGENTS.md`. The report is
`reports/2026-10-23-search-code-sharing-and-stophammer.md` in that repository.

This document binds nothing in Stophammer. It asks Stophammer to examine four
defects and a list of changes, and to make a decision. Stophammer records each decision
in its own ADR.

The data is in `sources/captures/2026-10-23-search/` in the research
repository. `SHA256SUMS` in that folder gives the hashes.

The research examined Stophammer at commit `97aa28e`, release 0.13.2.

## Defects

| # | Defect | Data source |
|---|---|---|
| S1 | After a change of a title, an artist or a description, the words from before the change stay in the search index. A search for one of those words finds the entity, and the bm25 statistics become incorrect. | `populate_search_index` sends the new text in the FTS5 `'delete'` command (`src/search.rs:139-145`). A contentless table needs the text that it indexed. `src/db.rs:832-835` states that no table keeps that text. `fts5_contentless_delete_test.py` and its output reproduce the pattern. |
| S2 | A retired publisher feed stays in the index with a null title. This is an inference. | The retire paths delete only the entity type `feed` (`src/api.rs:4803`, `src/apply.rs:307`). The delete paths send `""` as `name`, but the index holds the artist in `name`. |
| S3 | The internal columns match words. `q=track` matches each track with a rank near 0. `q=music` finds music feeds through the medium text. | `entity_type` and `entity_id` are not `UNINDEXED`, and `tags` holds `raw_medium` (`src/schema.sql:538-561`, `src/db.rs:1381-1456`). Captures `search-q-track.json` and `search-q-music.json`. |
| S4 | Each publisher hit has a quality score of 0. | Quality is stored with the type `feed` (`src/db.rs:1419`). The search joins `entity_quality` on the type `publisher`. |

S1 has a direct correction. The bundled SQLite is 3.45.0 (`libsqlite3-sys`
0.28.0). The FTS5 option `contentless_delete=1` needs SQLite 3.43 or a
subsequent version. With this option, a plain `DELETE` removes the indexed
words. The test output shows the result.

The correction needs a new index and one rebuild. The rebuild of 2026-09-26
took 3.7 s (`docs/reviews/v4vmm-request-5-search-duplicates-verification.md:117`).
The same rebuild can also correct S2 and S3. ADR 0022 owns the index design.

## Requested Changes

The research gives each change a priority from its effect on the three clients:
v4vmm, the musicindex website and musicindex-studio. Two changes are also in the
v4vmm requests. This document cites them and does not give them again.

| Priority | Change | Effect | Related request |
|---|---|---|---|
| 1 | Correct S1 to S4 with a new index | Correct results | None |
| 1 | Put each word of `q` in quotation marks as an FTS5 phrase, so that each text gives 200. Capture `search-q-apostrophe.json` gives the 400 of today. | Each client can send all text | v4vmm request 10 item 1 |
| 1 | `newest_item_at` on a feed hit. `feed_search_summary` (`src/query.rs:1296-1317`) does not read the column. | v4vmm saved searches can group singles | v4vmm request 11 item 1 |
| 2 | Give `effective_rank`, or tell in the contract that the row order is the rank order | The website sorts each page again by `rank` and loses the quality weight (`search.html:5008` in the website) | None |
| 2 | A prefix index (`prefix='2 3'`) and a prefix match on the last word of `q`. `src/search.rs:231` removes `*`. | Live search in v4vmm (ADR 0087 D4) and on the website finds the start of a word | None |
| 3 | One request for all types, with a cursor or a count for each type | v4vmm sends three searches for one query, the website sends two, and the node runs the match three times (`src/query.rs:5147-5167`) | None |
| 3 | An `ETag` from `MAX(events.seq)`, as the artist name list uses, and a short `Cache-Control`. The captures `search-*.headers` do not show these headers. | Live searches cost less | None |
| 3 | Read the summaries of all hits in one query with `IN (…)` | Fewer queries for each page | None |
| 3 | An index on `tracks.publisher` for the count of each row of `/v1/publishers` (`src/query.rs:5440-5446`). This is an inference: the research found no such index in `src/schema.sql`. | Less work for each row | None |
| 4 | A test that measures search time on a copy of the production database | A measurement before work on the cost of each page | None |
| 4 | Log the length of a query, not its text (`src/search.rs:273`, `src/query.rs:5088`) | Less private data in logs | None |

## Shared Client Code

The report also proposes a small crate in the Stophammer workspace. It holds the
response structs, the envelope, the `include` names and the limits, with no
HTTP code. v4vmm and Studio can then decode with the same types.

This proposal needs decisions in Stophammer, v4vmm and Studio. Stophammer owns
the contract (ADR 0045 §5). v4vmm keeps a stored copy of the contract and guard
tests (v4vmm ADR 0075 §6). v4vmm has no license, and Stophammer is
AGPL-3.0-only. The operator selects the license first.

## Not Examined

The research did not measure the production database. It did not build or run
Stophammer.

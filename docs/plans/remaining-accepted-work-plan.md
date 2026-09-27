# Remaining Accepted Work Plan

Date: 2026-09-26. This plan states no rule. Each item names the ADR that owns
its rule.

## Scope

This plan holds each open item of an Accepted ADR. A Proposed ADR is not in
scope. ADR 0045 and ADR 0055 wait for a decision. The index lists ADR 0032,
ADR 0033 and ADR 0034 under "Status Needs A Check".

## Items

| # | Item | Owner | Kind | Repositories | Plan |
|---|---|---|---|---|---|
| 1 | Each documented response points at its schema. Deployed on 2026-09-26 | ADR 0044 task 002 | Code | `stophammer` | [ADR 0044 phase plan](adr-0044-contract-schema-phase-plan.md) |
| 2 | The contract guards, and the correction of `AGENTS.md`. Deployed on 2026-09-26 | ADR 0044 task 003 | Code | `stophammer` | Same |
| 3 | A feed can block this index with `podcast:block`. Deployed on 2026-09-26 | ADR 0057 | Code | Parser, node, crawler | [ADR 0057 phase plan](adr-0057-podcast-block-phase-plan.md) |
| 4 | A migration drops the two proof tables and changes the delete trigger | ADR 0056 §3 | Code | `stophammer` | [Task 002](../tasks/adr-0056-task-002-drop-proof-tables.md) |
| 5 | Read the gossip log for rejected hosts. Complete on 2026-09-26 | ADR 0054 task 004 step 3 | Operator check | None | [ADR 0054 phase plan](adr-0054-fetch-rule-phase-plan.md) |
| 6 | The second `refresh` pass: `304` and the Wavlake `429` limit | ADR 0050 plan decision 10 | Operator pass | None | [ADR 0050 phase plan](adr-0050-feed-revalidation-phase-plan.md) |
| 7 | After that pass, count the `feed_copy_observed` events | ADR 0058 task 005 step 7 | Operator check | None | [ADR 0058 phase plan](adr-0058-feed-copies-phase-plan.md) |
| 8 | The four pending GUID changes of Elijah Lied | ADR 0052 §5 | Decided: wait for Doerfelverse | None | Below |
| 9 | Fast polling of a feed with a pending live event | ADR 0021 | Closed: ADR 0064 supersedes ADR 0021 | None | Below |
| 10 | A station that runs 24 hours a day on a feed with `medium` `podcast` | None yet | Research, then a decision | Node | Below |
| 11 | A read at once after a `live` or `liveEnd` podping can get an old copy of the feed | ADR 0062 §4 | Measure, then a decision | Crawler | Below |
| 12 | A snapshot refresh of the import can fill the disk. Deployed on 2026-09-27 | ADR 0033, amended | Code | Crawler, `docker-compose.yml` | Below |
| 13 | Remove the compatibility artist credit and the unused tables | ADR 0034 §10 | Tasks first | `stophammer` | Below |
| 14 | The ADR archive and the two governance guards. Complete on 2026-09-27 | ADR 0045 §6 and "Guards" | Code and a file move | `stophammer` | Below |
| 15 | Confirmed and unconfirmed artists of a publisher, and album reads with only the publishers they name. Built on 2026-09-27, not deployed | ADR 0061 | Code | `stophammer` | Below |

## Sequence

1. Item 5 is complete on 2026-09-26. No music host was rejected.
2. **Now, agents:** item 1, then item 2. At the same time, item 3 task 001,
   the parser. The two change different repositories.
3. **Item 3 tasks 002 and 003** after item 2. Item 2 adds a guard that each
   route and each reason are in the document, and ADR 0057 adds a reason.
4. **One deploy** of the node and the crawler for items 1 to 3.
5. **Item 6, operator:** the second `refresh` pass, after that deploy. Item 7
   uses the result of the same pass.
6. **Item 4** on 2026-10-02 or after. ADR 0056 was deployed on 2026-09-25, and
   its plan waits a week of stable operation.
7. **Items 8 and 9** at any time. Each needs an operator decision before code.

## Item 5: The Gossip Log

Run on the VPS:

```bash
docker compose logs --no-color --since 48h gossip 2>&1 \
  | sed 's/\x1b\[[0-9;]*m//g' \
  | grep -E 'fetch_target_not_public|body_too_large' \
  | grep -oE 'https?://[^/ ]+' | sort | uniq -c | sort -rn | head -40
```

A music feed host in the result is a defect of ADR 0054. An empty result, or
only hosts that are not music hosts, closes step 3 of task 004.

## Item 6 And Item 7: The Second Refresh Pass

The ADR 0050 phase plan gives the command and the report. The report of the
pass gives the count of `304` answers and of `429` answers for Wavlake. Before
the pass, record the count of `feed_copy_observed` events. After the pass,
the count may grow only by the copy summaries that changed.

## Item 8: The Pending GUID Changes

On 2026-09-25 the four Doerfelverse releases of Elijah Lied are pending in
`GET /v1/guid-changes`. The publisher confirmed that the new GUIDs are a tool
error. The operator has two choices:

- Reject each change with `POST /v1/feeds/{guid}/guid-change`. The records
  keep their GUIDs. When Doerfelverse restores the GUIDs, nothing more
  happens.
- Wait for Doerfelverse to restore the GUIDs. The pending rows stay public.

On 2026-09-26 the operator decided to wait for Doerfelverse.

## Item 9: Fast Polling Of A Live Event

On 2026-09-26 the operator decided that real-time live state is outside the
index. [ADR 0064](../adr/0064-a-live-item-is-an-rss-fact.md), Accepted on
2026-09-26, supersedes ADR 0021. It removes the poll rule and adds two read
paths for a live item. Its tasks replace this item.

## Item 10: Stations On Podcast Feeds

A V4V music station that runs 24 hours a day usually has `medium` `podcast`.
The medium check rejects such a feed, so the index does not see the station.
First collect sample feeds, for example from the relay or from the Podcast
Index API. Then decide if the index admits a `podcast` feed for its live items
only.

## Item 11: A Read From A Cache

A publisher can send a `live` or `liveEnd` podping before its host serves the
new feed. The crawler then reads the old feed, and no second podping comes.
First measure how often the crawl after such a podping gives `no_change`. Then
decide on a second read some minutes later, as an amendment of ADR 0062 §4.

## Item 12: The Snapshot Refresh And The Disk

On 2026-09-27 a snapshot refresh of the `import` mode filled the disk of the
host, and the node could not write its database. The mode writes the new
PodcastIndex snapshot to `podcastindex_feeds.download`. The old file stays
until the new one is complete, so a refresh needs the size of the snapshot a
second time. When the write fails, `refresh_snapshot` in
`stophammer-crawler/src/modes/import.rs` keeps the partial file. Only the next
run deletes it.

The work:

1. Delete the `.download` file when the write fails.
2. Check the free space before the download. When it is less than the size
   of the snapshot plus a margin, stop the run with a clear error.
3. Give each container log a size limit in `docker-compose.yml`.

On 2026-09-27 the three steps are built and deployed. A refresh that
lacks the space for a second copy of the snapshot now gives a warning and
keeps the existing snapshot. Before this change, a failed refresh stopped the
whole run. Each container log keeps at most 3 files of 20 MB.

## Item 13: The Compatibility Artist Credit

[ADR 0034](../adr/0034-adopt-rebuild-first-source-first-v1-music-schema.md)
§10 names the work:

1. Stop the ingest from making `artists` and `artist_credit` rows, and stop
   `ArtistCreditCreated`. A node still applies the old events of the log.
2. Rebuild `feeds` and `tracks` with no `artist_credit_id`, in a migration.
3. Drop `artists`, `artist_aliases`, `artist_credit`, `artist_credit_name`,
   `artist_type`, `rel_type` and `external_ids`.
4. Move the internal SSE code off the artist IDs, or remove the parts that
   only they serve.

The work needs a phase plan and tasks before code, because it changes the
event protocol and rebuilds the two largest tables.

## Item 14: The ADR Archive And The Guards

[ADR 0045](../adr/0045-governance-model-and-contract-ownership.md) §6 and its
section "Guards" name the work:

1. Move each superseded ADR to `docs/adr/archive/`, and correct each link to
   it.
2. A test fails when `AGENTS.md` is not in the output of `git ls-files`.
3. A test compares the ADR files with the rows of `docs/adr/README.md`. It
   fails when a file has no row, or when a row names no file.

On 2026-09-27 the work is complete, by
[task 001](../tasks/adr-0045-task-001-archive-and-guards.md). The ten
superseded ADRs are in `docs/adr/archive/`, and
`tests/adr0045_governance_guards_tests.rs` holds the two tests.

## Item 15: The Confirmed Artists Of A Publisher

[ADR 0061](../adr/0061-a-publisher-read-counts-its-listed-artists.md) needs
one node task: the four fields of §1, the OpenAPI schema, `docs/API.md`, and
the tests of its section "Guards". The album read already follows §5, so a
test guards it and no code changes there. At the deploy, the request files of
v4vmm and musicindex.org get a note about the four new fields.

On 2026-09-27 the task is built, by
[task 001](../tasks/adr-0061-task-001-confirmed-artists.md), and
`tests/adr0061_confirmed_artists_tests.rs` holds the tests. The deploy comes
with the next release.

## Not In This Plan

- The removal of the backup files `pre-stale-reset.db`, `pre-adr0058.db` and
  `pre-adr0052.db` from the `primary-data` volume. This is an operator step
  with no owner ADR.
- The request of the slug `musicindex` in the namespace service slug list.
  ADR 0057 §1 makes it an operator step outside the code.

# Remaining Accepted Work Plan

Date: 2026-10-02. This plan states no rule. Each item names the ADR that owns
its rule.

## Scope

This plan holds each open item of an Accepted ADR, and the research that can
lead to a decision. A Proposed ADR is not in scope: ADR 0055 waits for a
crawler outside the host of the primary. Version control holds the items that
are complete.

## Items

| # | Item | Owner | Kind | Repositories | State |
|---|---|---|---|---|---|
| 1 | Release 0.5.0 | ADR 0065, ADR 0066 | Release | All three | Built. The candidate `v0.5.0-rc.1` goes on `a05f62a` |
| 2 | Fill the item titles and image of the copy rows | ADR 0058 §1c, [task 006](../tasks/adr-0058-task-006-copy-titles-and-image.md) | Operator pass | None | After the deploy of 0.5.0 |
| 3 | `role` is null when no feed states a `rel` | ADR 0049 §6, amended on 2026-10-02 | Code | `stophammer` | Built for 0.6.0 |
| 4 | A `304` to a request with no conditional header is a fetch error. The dead `refetch_unconditional` is deleted | ADR 0050 §3, amended on 2026-10-02 | Code | Crawler | Built for 0.6.0 |
| 5 | Each publisher link says how the album names it | ADR 0069, [task 001](../tasks/adr-0069-task-001-link-provenance.md) | Code | Parser, node, crawler fixtures | Built for 0.6.0 |
| 6 | A publisher read gives `co_credited_feeds` | ADR 0069, [task 002](../tasks/adr-0069-task-002-co-credited-feeds.md) | Code | `stophammer` | Built for 0.6.0. It adds about 160 ms to the read of the largest publisher feed |
| 6b | The reconciliation reads BLOB payloads and moves the archive cursor. A refused connection to the node is sent again. A dead reconciliation loop stops the crawler, and a cursor lag over 15 minutes gives a warning | ADR 0062 §8, amended on 2026-10-02 | Code | Crawler | Built for 0.6.0 |
| 7 | Write the new `source` values of each record | ADR 0069 task 001, deploy step | Operator pass | None | After the deploy of 0.6.0 |
| 8 | The four pending GUID changes of Elijah Lied | ADR 0052 §5 | Decided: no action by the index | None | Below |
| 9 | A station that runs 24 hours a day on a feed with `medium` `podcast` | None yet | Research, then a decision | Node | Below |
| 10 | A read at once after a `live` or `liveEnd` podping can get an old copy of the feed | ADR 0062 §4 | Measure, then a decision | Crawler | Below |

## Sequence

1. Tag and deploy release 0.5.0, then run item 2.
2. Release 0.6.0 holds items 3 to 6b, each built. Its notes tell
   v4vmm and musicindex.org that `role` can now be null.
3. Deploy 0.6.0, then run item 7.
4. Items 9 and 10 at any time. Each needs a measurement and a decision before
   code.

## Item 2: The Copy Rows

The deploy step of ADR 0058 task 006 gives the commands: one `refresh` pass
with `--no-revalidate`, and a count of the rows that still have no titles.

## Item 7: The `source` Values

The deploy step of ADR 0069 task 001: one `refresh` pass with `--force`. Each
record sends its body again, and the node stores the new `source` values.
Since release 0.5.0, the pass signs only the events of the changed remote
items.

## Item 8: The Pending GUID Changes

On 2026-09-25 the four Doerfelverse releases of Elijah Lied became pending in
`GET /v1/guid-changes`. The publisher confirmed that the new GUIDs are a tool
error. The operator has two choices:

- Reject each change with `POST /v1/feeds/{guid}/guid-change`. The records
  keep their GUIDs. When Doerfelverse restores the GUIDs, nothing more
  happens.
- Wait for Doerfelverse to restore the GUIDs. The pending rows stay public.

On 2026-09-26 the operator decided to wait for Doerfelverse, and confirmed
the decision on 2026-10-02. The feed author corrects the feeds. The index
does not correct a feed for its author. The pending rows stay public. When a
feed declares its old GUID again, the next crawl deletes its pending row, and
the record keeps its GUID (ADR 0052, "Guards").

## Item 9: Stations On Podcast Feeds

A V4V music station that runs 24 hours a day usually has `medium` `podcast`.
The medium check rejects such a feed, so the index does not see the station.
First collect sample feeds, for example from the relay or from the Podcast
Index API. Then decide if the index admits a `podcast` feed for its live items
only.

## Item 10: A Read From A Cache

A publisher can send a `live` or `liveEnd` podping before its host serves the
new feed. The crawler then reads the old feed, and no second podping comes.
First measure how often the crawl after such a podping gives `no_change`. Then
decide on a second read some minutes later, as an amendment of ADR 0062 §4.

## Not In This Plan

- The removal of the backup files `pre-stale-reset.db`, `pre-adr0058.db` and
  `pre-adr0052.db` from the `primary-data` volume. This is an operator step
  with no owner ADR.
- The request of the slug `musicindex` in the namespace service slug list.
  ADR 0057 §1 makes it an operator step outside the code.

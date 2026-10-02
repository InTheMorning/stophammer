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
| 8 | The four pending GUID changes of Elijah Lied | ADR 0052 §5 | Decided: no action by the index | None | Below |
| 9 | A station that runs 24 hours a day on a feed with `medium` `podcast` | None yet | Research, then a decision | Node | Below |
| 10 | A read at once after a `live` or `liveEnd` podping can get an old copy of the feed | ADR 0062 §4 | Measure, then a decision | Crawler | Below |

## Sequence

Items 9 and 10 at any time. Each needs a measurement and a decision before
code. Item 8 needs no action by the index.

Releases 0.5.0 and 0.6.0 of 2026-10-02 hold the items of this plan that were
built.

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

- The request of the slug `musicindex` in the namespace service slug list.
  ADR 0057 §1 makes it an operator step outside the code.

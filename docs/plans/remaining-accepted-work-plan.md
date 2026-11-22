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
| 8 | The four pending GUID changes of Elijah Lied | ADR 0052 §4 | Waits for ADR 0106 | None | Below |
| 9 | A station that runs 24 hours a day on a feed with `medium` `podcast` | None yet | Research, then a decision | Node | Below |
| 10 | A read at once after a `live` or `liveEnd` podping can get an old copy of the feed | ADR 0062 §4 | Measure, then a decision | Crawler | Below |

## Sequence

Items 9 and 10 at any time. Each needs a measurement and a decision before
code. Item 8 needs no code. The operator sends four approvals.

## Item 8: The Pending GUID Changes

On 2026-09-25 the four Doerfelverse releases of Elijah Lied became pending in
`GET /v1/guid-changes`. The publisher confirmed that the new GUIDs are a tool
error. The operator has two choices:

- Reject each change with `POST /v1/feeds/{guid}/guid-change`. The records
  keep their GUIDs. When Doerfelverse restores the GUIDs, nothing more
  happens.
- Wait for Doerfelverse to restore the GUIDs. The pending rows stay public.

Operator choice, 2026-11-22: approve each of the four changes (ADR 0052 §4,
the operator case). This choice replaces the wait of 2026-09-26. These are the
reasons:

1. Each feed declares the new GUID since 2026-09-11, 72 days. No revert came
   after the publisher reported the tool error on 2026-09-25.
2. On 2026-11-22 the Podcast Index API 1.0 gave the new GUID for each URL
   (`byfeedurl`, ids 7571710, 7365050, 7214947 and 7332989). `byguid` with
   each old GUID gave no feed. A client that finds a release through Podcast
   Index thus gets a GUID that the index does not hold.
3. The namespace makes the feed value canonical. ADR 0052 §5 keeps a link
   from the old GUID. If the publisher reverts, §5 runs again in the other
   direction.

The cost is known: each release gets new track identities (ADR 0040), and the
old GUID answers `404` with `superseded_by`.

| Source URL | Old GUID | New GUID |
|---|---|---|
| `https://www.doerfelverse.com/artists/elijahlied/killthemfbeat/kill-the-mf-beat.xml` | `01128ba4-19c4-53d4-b209-d4a7ece8b889` | `2b05d37c-fac5-5ae3-a5ad-3a9662038487` |
| `https://www.doerfelverse.com/artists/elijahlied/throughthemic/through-the-mic.xml` | `d6fe0d1c-bd8d-552b-ac66-cce620391810` | `ff59d578-2f7f-56e4-b38d-b0c0f3ab799c` |
| `https://www.doerfelverse.com/artists/elijahlied/all-in-a-day/all-in-a-day.xml` | `6fde038d-3585-5d28-93e5-aabe48fd7fa6` | `25f3e971-453c-507a-97e8-800f38f56415` |
| `https://www.doerfelverse.com/artists/elijahlied/m0mma/m0mma.xml` | `902ae9db-c74b-5fd4-bcf1-d206c19479b4` | `64645682-dbe0-5bc0-b52b-9f662d0b38d9` |

The operator sends `POST /v1/feeds/{old_guid}/guid-change` with the decision
`approve` for each old GUID. The change occurs at the next submission of the
new GUID from the source URL. The item closes when `GET /v1/feeds/{old_guid}`
answers `404` with `superseded_by` for each of the four.

The two other pending rows are not in this choice. They stay pending.

On 2026-11-22 the operator stopped the approvals until a decision on
[ADR 0106](../adr/0106-a-feed-changes-its-guid-in-place.md). That ADR is
Proposed. It can replace the approvals with a change in place at the next
submission.

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

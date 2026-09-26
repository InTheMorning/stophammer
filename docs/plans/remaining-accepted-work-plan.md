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
| 1 | Each documented response points at its schema | ADR 0044 task 002 | Code | `stophammer` | [ADR 0044 phase plan](adr-0044-contract-schema-phase-plan.md) |
| 2 | The contract guards, and the correction of `AGENTS.md` | ADR 0044 task 003 | Code | `stophammer` | Same |
| 3 | A feed can block this index with `podcast:block` | ADR 0057 | Code | Parser, node, crawler | [ADR 0057 phase plan](adr-0057-podcast-block-phase-plan.md) |
| 4 | A migration drops the two proof tables and changes the delete trigger | ADR 0056 §3 | Code | `stophammer` | [Task 002](../tasks/adr-0056-task-002-drop-proof-tables.md) |
| 5 | Read the gossip log for rejected hosts | ADR 0054 task 004 step 3 | Operator check | None | [ADR 0054 phase plan](adr-0054-fetch-rule-phase-plan.md) |
| 6 | The second `refresh` pass: `304` and the Wavlake `429` limit | ADR 0050 plan decision 10 | Operator pass | None | [ADR 0050 phase plan](adr-0050-feed-revalidation-phase-plan.md) |
| 7 | After that pass, count the `feed_copy_observed` events | ADR 0058 task 005 step 7 | Operator check | None | [ADR 0058 phase plan](adr-0058-feed-copies-phase-plan.md) |
| 8 | The four pending GUID changes of Elijah Lied | ADR 0052 §5 | Operator decision | None | Below |
| 9 | Fast polling of a feed with a pending live event | ADR 0021 | Decision first | Crawler | Below |

## Sequence

1. **Now, operator:** item 5. It needs no code. The gossip mode has run for
   more than a day since the ADR 0054 deploy.
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

## Item 9: Fast Polling Of A Live Event

ADR 0021 says: "the crawl scheduler reduces the poll interval for that feed
to 60 seconds". On 2026-09-26 no crawler mode has a poll interval. The
`gossip` mode reacts to podpings, and the `refresh` mode runs when the
operator starts it. Thus the rule names a part that does not exist. The
operator decides one of these:

- A new ADR supersedes this part of ADR 0021, because a live feed sends a
  podping when its state changes.
- A plan adds a small poll loop to the `gossip` mode for the feeds with a
  pending live event.

## Not In This Plan

- The removal of the backup files `pre-stale-reset.db`, `pre-adr0058.db` and
  `pre-adr0052.db` from the `primary-data` volume. This is an operator step
  with no owner ADR.
- The request of the slug `musicindex` in the namespace service slug list.
  ADR 0057 §1 makes it an operator step outside the code.

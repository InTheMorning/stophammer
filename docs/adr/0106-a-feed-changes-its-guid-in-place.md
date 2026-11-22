# ADR 0106: A Feed Changes Its GUID In Place

## Status
Accepted on 2026-11-22. The operator, through the orchestrator session,
selected the recommended option of each question in "Decision" and kept
`GET /v1/guid-changes` as an empty list. On the same day the operator also
accepted §6, §7 and the order of §8. No release holds it.
[Task 001](../tasks/adr-0106-task-001-guid-change-in-place.md) and
[task 002](../tasks/adr-0106-task-002-redirect-and-classify.md) give the
work.

- Bar: difficult to change later. The change writes new signed events, and
  each community node applies them.
- Supersedes: [ADR 0052](0052-a-source-moves-its-own-feed.md) §4, §5, the
  last two invariants and the guards for a GUID change. §1, §2, §3 and §6 of
  ADR 0052 stay.
- Changes: ADR 0052 §1, the first condition for a permanent redirect only
  (§6 below), and the ADR 0052 guard for a `301` to a different GUID.
- Uses: ADR 0040 for track identity, ADR 0044 rule 4 for a `v1` field, ADR
  0051 for the source URL, ADR 0079 for a retired GUID.

## Date
2026-11-22

## Context

On 2026-11-22 the operator gave a new direction. stophammer must follow the
behavior of Podcast Index for a GUID change at the same URL.

### What Podcast Index does

The research note
`musicindex-research/research_notes/aggregation/podcastindex-guid-behavior.md`
§2 and §6 gives the evidence.

- The namespace `docs/tags/guid.md` says: "if one exists in the RSS feed, that
  value is canonical."
- On 2026-11-22 the operator queried the Podcast Index API 1.0 for the four
  Elijah Lied URLs. `byfeedurl` gives the same record for each URL (ids
  7571710, 7365050, 7214947 and 7332989), with `podcastGuid` equal to the new
  GUID. `byguid` with each old GUID gives no feed. `url` and `originalUrl`
  are the same address.
- **Inference**, not from the code of Podcast Index: Podcast Index keys a
  record by its numeric id and its URL. When a feed declares a different
  GUID, Podcast Index writes the new value into the same record with no
  operator step. It keeps no alias.
- Not known: if Podcast Index held the old GUIDs before 2026-09-11. Not known:
  what Podcast Index does for a URL change with the same GUID, or for two URLs
  that declare one GUID. This ADR does not change those cases.

### What stophammer does today

ADR 0052 §4 holds each GUID change at the same URL as a pending row. It does
not hold a new GUID that is the UUIDv5 of the source URL. The operator
approves or rejects a pending row. An approval runs §5 at the next
submission. The node retires the old record, writes a link from the old GUID,
and admits a new record.

On 2026-11-22 the main session read the code:

- ADR 0040 makes the track identity `(feed_guid, track_guid)`. `track_guid` is
  the raw item GUID of the feed. `canonical_track_entity_id` in `src/db.rs`
  makes the search and quality key from the same two values. No other input,
  for example the enclosure URL, is part of the identity.
- 26 tables of `src/schema.sql` have a column with `feed_guid` in its name.
  `search_entities` and `entity_quality` hold a track key made from it. No
  code changes `feeds.feed_guid` of a record. The node deletes the record and
  writes a new one.
- `guid_change_transition` in `src/api.rs` commits the retirement and the link
  in one transaction. `db::ingest_transaction` then admits the new record in a
  second transaction. ADR 0052 §5 asks for one transaction. An error between
  the two leaves the old record retired and no new record.
- `classify_submission` in `src/db.rs` reads one row with
  `feed_url IN (source_url, canonical_url) AND feed_guid <> ?`. When one
  record holds the source URL and a different record holds the final URL of
  a redirect, either row can be the answer. Under the hold of ADR 0052 §4
  this wrote only a pending row. With an automatic change, it can change the
  GUID of the wrong record.

On 2026-11-22 the node held six pending rows. Four are Elijah Lied. The two
others are:

- `https://feeds-qa-854568123236.us-central1.run.app/v1/channel/595d12aa171f81733666b8f865030ea4`,
  `c1b21e66-…` to `b67b7b95-…`. The record has the title "E2E Test Community"
  and `medium` `publisher`. The host name says that it is a test service.
- `https://headstarts.uk/msp/publisher-feeds/nat-hills.xml`, `190dd27e-…` to
  `c080bce6-…`. The record has the title "THERAPY IN SESSION" and `medium`
  `music`.

## Decision

### 1. A GUID change at the source URL applies with no operator step

| Option | Good points | Bad points |
|---|---|---|
| A. Apply at the first admitted submission from the source URL (recommended) | Same as Podcast Index. A revert applies the same way. No pending state, no operator route | A tool error applies at once. A tool that changes the GUID in each fetch changes the record in each fetch |
| B. Apply after a count or a time, for example two submissions 24 hours apart | Stops a GUID that shows in one fetch only | The 2026-09-25 amendment of ADR 0052 found that a wait gives no evidence of intent. The Doerfelverse value stayed 72 days. Keeps a pending table |
| C. Keep the operator step of ADR 0052 §4 | No change | The operator direction rejects it. The index differs from Podcast Index for the time of the wait |

**Recommendation: A.** These guards stay, because each one stops a takeover
and not an error of the publisher:

- Only a body from the source URL of the record changes the GUID (ADR 0051). A
  body from a mirror or a copy URL changes nothing.
- The new GUID must be free. If a different record holds it, the answer stays
  `record_conflict`. If a retired-source hold of ADR 0079 holds it, the answer
  stays `retired_source_conflict`. A `guid` block of ADR 0053 stays a block.
- The body must pass each other verifier. For example, a body over the track
  limit changes nothing.

The UUIDv5 case of ADR 0052 §4 is no longer a special case. No limit applies
to the number of changes. If a tool changes the GUID in each fetch, a limit
can come after that incident.

### 2. The record keeps its row, and the node changes the GUID in place

ADR 0040 makes each track identity `(feed_guid, track_guid)`. Thus each option
below changes each track identity from `(old_guid, track_guid)` to
`(new_guid, track_guid)`. The raw `track_guid` does not change in any option.

| Option | Good points | Bad points |
|---|---|---|
| A. Change the GUID in place (recommended) | Same as Podcast Index: one record. Payment routes, value splits, source claims, URL history and move state stay with the record. One transaction and one event. No time with no record | New code that changes the feed GUID in each table of step 1 below. A table that a later change adds must join the change |
| B. Keep ADR 0052 §5: retire, link, admit a new record | The code exists | The record shows as new. The URL history, move state and copies of the old record go. Today the change is two transactions |
| C. A stable internal identity beside the declared GUID | A GUID change changes no key | ADR 0052 rejected it: a new primary key for about 15 tables and for each track identity of ADR 0040. A larger change than A for the same result |

**Recommendation: A.** In one transaction, the node:

1. Changes `feed_guid` from the old GUID to the new GUID in each table that
   holds the GUID of this record. This includes `feeds`, `tracks`,
   `payment_routes`, `value_time_splits` (`source_feed_guid`), each
   `source_*` table, `feed_url_observations`, `feed_left_urls`,
   `feed_move_state` and `feed_copies`.
2. Makes the search and quality keys of each track again from the new GUID.
3. Writes the link from the old GUID to the new GUID in
   `feed_guid_supersessions`. It deletes a link whose old GUID is the new
   GUID, so that a revert does not keep a link to itself.
4. Applies the body as an ordinary update.
5. Signs one new event, `FeedGuidChanged`, with `source_url`, `old_guid` and
   `new_guid`. A community node applies the same change.

A column that names a different feed does not change. Examples are
`remote_feed_guid` in `feed_remote_items_raw`, `track_remote_items_raw` and
`value_time_splits`. Each is a statement of the body of a different feed, and
ADR 0051 lets only that body change it.

A test reads each column with a feed GUID from the schema. It fails when the
change of step 1 does not name a column. The failure message names this ADR.
It tells the author to add the column to the change, or to the list of
columns that name a different feed.

### 3. A request for the old GUID gives `404` with `superseded_by`

| Option | Good points | Bad points |
|---|---|---|
| A. `404` with `superseded_by`, as today (recommended) | No change to the API. `musicindex-studio` reads the field (`crates/core/src/musicindex.rs`). A client finds the record with no guess | Podcast Index gives no answer for the old GUID |
| B. A redirect to the new GUID | A client follows it with no code | A client can then pay or store the new record with no decision. ADR 0052 forbids this for a payment route |
| C. The record, under the old GUID | Old clients go on | Two GUIDs give one record. The namespace makes the feed value the only canonical value |
| D. `404` with no field, as Podcast Index | Same as Podcast Index | A client that stored the old GUID cannot find the record. `musicindex-studio` loses a path |

**Recommendation: A.** The link is for navigation only, as in ADR 0052 §5.
The same `404` body applies to `GET /v1/feeds/{old_guid}` and to
`GET /v1/feeds/{old_guid}/tracks/{track_guid}`. `superseded_by` gives the new
feed GUID. The `track_guid` does not change.
`GET /v1/feeds/by-url` with the source URL gives the record under the new
GUID.

### 4. The pending rows

| Option | Good points | Bad points |
|---|---|---|
| A. The deploy deletes each pending row. Each source URL applies at its next submission (recommended) | One rule for all six rows. No operator step | The four Elijah Lied records wait for the release |
| B. The operator approves the four Elijah Lied rows now under ADR 0052 | They change before the release | They change by §5 of ADR 0052: retired, with new records. Option A of §2 then cannot keep their rows |

**Recommendation: A.** The node keeps no body of an earlier submission, so a
change at the deploy is not possible. The scheduled passes submit each URL.
The operator can start a submission with `stophammer-crawler feed --force`.

The `feeds-qa` record needs no rule in this ADR. A rule that depends on a host
name does not belong in the GUID rule. If the operator does not want a test
feed in the index, that is a separate decision about the record, not about
its GUID. The existing `DELETE /v1/feeds/{guid}` and the blocks of ADR 0053
can remove it.

The `nat-hills` record changes at its next submission, the same as the
others.

### 5. Events, API answers and tests

**Events.**

- New: `FeedGuidChanged` (§2). The rollout upgrades each community node
  before the primary emits one, as in ADR 0052 §6.
- `FeedGuidChangeObserved`, `FeedGuidChangeDecided` and `FeedGuidSuperseded`:
  the node does not emit them. Their apply code stays, because a node that
  applies the history from the start applies them.
- `FeedRetired` with the reason `guid_superseded`: the node does not emit it.
  Its apply code stays for the same reason.

**API answers.**

| Answer | Change |
|---|---|
| Ingest reasons `guid_change_pending` and `guid_change_rejected` | Removed. A GUID change is accepted with the warning `GUID changed from <old> to <new> (ADR 0106)` |
| `POST /v1/feeds/{guid}/guid-change` | Removed. It is an admin route, and no client uses it |
| `GET /v1/guid-changes` | Stays until a `v2`, and always gives an empty list. A client outside the suite can read it, as for `pending_guid_change` |
| `pending_guid_change` in `GET /v1/feeds/{guid}` | Stays, and is always `null`. ADR 0044 rule 4 makes a removal a breaking change |
| `superseded_by` in a `404` | Stays (§3) |
| `src/openapi.rs` and `docs/API.md` | Change with each line above |

**Tests deleted with ADR 0052 §4.** Each asserts the hold, the pending row or
the operator route:

- `tests/adr0052_guid_change_tests.rs`:
  `a_guid_change_that_is_not_the_uuidv5_is_pending_and_public`,
  `a_repeated_pending_guid_signs_no_new_event_and_touches_last_seen`,
  `a_submission_with_the_old_guid_deletes_the_pending_row`,
  `reject_holds_for_the_same_guid_and_a_different_guid_opens_a_new_row`,
  `approve_runs_the_transition_at_the_next_submission` and
  `the_guid_change_route_answers_403_with_no_admin_token`.
- `tests/adr0051_source_url_tests.rs`:
  `a_new_guid_at_a_held_url_answers_guid_change_pending` and
  `feed_count_is_unchanged_by_guid_change_pending`.
- `tests/adr0079_retired_source_tests.rs`: `a_guid_change_retire_writes_no_hold`.
  A GUID change retires nothing.

**Tests that change.**

- `a_uuidv5_guid_transitions_at_once_and_the_new_guid_then_updates`: a free
  GUID of any form changes in place. The track rows keep their raw
  `track_guid` and their payment routes.
- `a_uuidv5_guid_change_over_the_track_limit_does_not_retire_the_old_record`:
  a body over the limit changes nothing.
- `a_replica_applying_the_events_has_the_same_rows_and_supersession`: a
  replica applies `FeedGuidChanged`, and still applies the three old events.
- `tests/adr0078_retire_index_tests.rs` `guid_change_retire_deletes_each_index_row`:
  the search rows of the record change to the new key.

**The crawler.** `stophammer-crawler` is a separate repository. Its list of
node answers that it does not cache names `guid_change_pending`
(`is_uncached_node_answer` in `src/crawl.rs`). The node no longer sends that
answer, so the entry has no effect. A change in that repository can remove
it.

**Tests that stay.**
`tests/adr0079_retired_source_tests.rs` `a_guid_superseded_retire_gets_no_hold`
(history apply), `tests/adr0088_feed_by_url_tests.rs`
`a_retired_guid_gives_404_with_superseded_by`, and the two classification
tests of `tests/adr0051_classify_tests.rs`.

**New tests.** A revert changes the GUID back. A new GUID that a different
record holds gives `record_conflict`. An error in the change leaves the
record unchanged. The schema test of §2. One test for each case of §6 and §7,
and one for two records at `S` and `F` (§6).

**Tests deleted with the ADR 0052 guard of §6.** Each test that asserts that
a `301` to a body with a different GUID moves nothing.

### 6. A permanent redirect with a GUID change

The source URL `S` of record `R` answers `301` or `308` to `F`, and the body
at `F` declares a free new GUID `G2`. The node changes `R` to `G2` in place
and moves `R` to `F`, in one transaction. Both come from the source URL: `S`
gives the redirect, and §1 lets `S` change the GUID.

This changes the first condition of ADR 0052 §1 for the permanent redirect
trigger only. The condition becomes: the body at `F` declares the GUID of the
record, or a free GUID (ADR 0106). The ADR 0052 guard "A `301` from the source
URL to a URL that declares a different GUID moves nothing" no longer applies.

These conditions stay:

- The record that changes is always the record whose source URL is the
  `source_url` of the submission. If `F` is the source URL of a different
  record, the answer is `record_conflict`, and no record changes.
- The rules of ADR 0083 for a return stay. During a wait of ADR 0083 §5, the
  GUID change applies, and `R` keeps `S`.

### 7. A declared new URL with a GUID change stays a new feed

The body at `S` declares `itunes:new-feed-url` or a self link `F`, and the
body at `F` declares a free new GUID `G2`. The body at `F` is a new feed under
ADR 0051. The node admits a new record at `F`, and `R` does not change. ADR
0052 §1 stays for these two triggers.

A copied tag can name a different release. For example, album 2 is a copy of
album 1 and keeps the self link of album 1. A move with a GUID change would
then turn album 2 into album 1. A second record at `F` is the cost. When `S`
answers `404` or `410`, ADR 0067 retires `R`.

### 8. The order of the checks

The node does these steps in this order for each submission. The first step
that refuses gives the answer, and the submission changes nothing.

1. The crawl token of ADR 0055 and ADR 0056.
2. The block of ADR 0053 on the declared GUID, on `source_url` or on
   `canonical_url`. The answer is `blocked`.
3. `classify_submission`. The record that changes is only the record whose
   `feed_url` is `source_url`. Each other record at `source_url` or at
   `canonical_url` gives `record_conflict`.
4. For a GUID change, the new GUID is new to the index, so the guards of a
   new feed apply to it. An active hold of ADR 0079 on the new GUID refuses
   each URL that is not the held URL. The answer is `retired_source_conflict`.
   A new feed and a GUID change use one function for this check.
5. For a GUID change, a body that blocks this index (ADR 0057) retires the
   record under its stored GUID, as for an update. The GUID does not change
   first.
6. The verifier chain. A failure changes nothing.
7. The move decision of ADR 0052 §2 and ADR 0083 §5 and §6, for an update and
   for a GUID change (§6).
8. One ingest transaction: the GUID change, then the move under the new GUID,
   then the body, then `FeedGuidChanged` and the move events.

## Alternatives Considered

The tables of each question give the options. More options:

### A redirect with a GUID change keeps the source URL

The record changes its GUID and keeps `S`. Each later fetch follows the
redirect again. The redirect comes from the source URL, as the GUID does, so
the node has the same evidence for the move. Rejected.

### A move with a GUID change for `itunes:new-feed-url` only

This tag is an explicit statement of a move, so a wrong value is less
frequent than for a self link. No counted case exists, and a copied tag has
the same risk as a copied self link. Rejected.

### A pending row that applies by itself after a time

This is ADR 0052 before its 2026-09-25 amendment. It keeps the table, the
events and the operator route for a wait that gives no evidence. Rejected.

## Consequences

- A GUID change at the source URL applies on the first admitted submission,
  as in Podcast Index. A tool error also applies at once. The publisher
  corrects the feed, and the same rule changes the GUID back.
- The record keeps its row. A client that stored `(old_guid, track_guid)`
  gets `404` with `superseded_by`, and finds the same `track_guid` under the
  new GUID.
- A release feed that names the old GUID of a publisher feed in
  `podcast:remoteItem` keeps that value until its own body changes.
  `superseded_by` never selects a payment route.
- The operator has no step for a GUID change. The pending table, the operator
  route and two ingest reasons go.

## Invariants

These replace the last two invariants of ADR 0052:

- A GUID change applies only from a body at the source URL, and only to a GUID
  that no record, hold or block holds.
- A GUID change applies only to the record whose source URL is the
  `source_url` of the submission.
- A GUID change keeps the record row and each `track_guid`.
- `superseded_by` never selects a payment route.

## Acceptance

Mechanical. Each criterion is a test in the task file:

1. A free new GUID at the source URL changes the record in one transaction.
   The record count does not change.
2. After the change, no column of the schema list holds the old GUID for this
   record.
3. `GET /v1/feeds/{old_guid}` and `GET /v1/feeds/{old_guid}/tracks/{track_guid}`
   give `404` with `superseded_by`.
4. A revert changes the GUID back at the next submission, and leaves no link
   from a GUID to itself.
5. A new GUID held by a different record gives `record_conflict`.
6. A replica that applies `FeedGuidChanged` has the same rows as the primary.
7. An error in the change leaves the record unchanged.
8. `S` answers `301` to `F`, and `F` declares a free `G2`: `R` has `G2` and
   the source URL `F`.
9. `R` holds `S`, `R2` holds `F`, `S` answers `301` to `F`, and `F` declares a
   free `G2`: the answer is `record_conflict`, and neither record changes.
10. `S` declares `itunes:new-feed-url` `F`, and `F` declares a free `G2`: a
    new record holds `F` with `G2`, and `R` does not change.
11. `R` at `S` declares `G2`, and an active ADR 0079 hold on `G2` names a
    different URL: the answer is `retired_source_conflict`, and `R` keeps its
    GUID.
12. `R` at `S` declares `G2`, and an active ADR 0079 hold on `G2` names `S`:
    `R` changes to `G2`, and the hold row is deleted.
13. `R` at `S` declares `G2` and a block of this index (ADR 0057): `R` is
    retired under its stored GUID, and no record has `G2`.

Visual: none.

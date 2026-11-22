# ADR 0106: A Feed Changes Its GUID In Place

## Status
Proposed on 2026-11-22. No code is written. Each question in "Decision" gives
the options and a recommendation. The operator selects one option for each
question.

- Bar: difficult to change later. The change writes new signed events, and
  each community node applies them.
- Supersedes: [ADR 0052](0052-a-source-moves-its-own-feed.md) §4, §5, the
  last two invariants and the guards for a GUID change. §1, §2, §3 and §6 of
  ADR 0052 stay.
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
| `GET /v1/guid-changes` | Stays, and always gives an empty list. The operator can select its removal. ADR 0044 rule 4 covers a field, not a route |
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
- `stophammer-crawler/src/crawl.rs`
  `is_uncached_node_answer_is_true_for_each_of_the_five_reasons`: the list
  loses `guid_change_pending`.

**Tests that stay.**
`tests/adr0079_retired_source_tests.rs` `a_guid_superseded_retire_gets_no_hold`
(history apply), `tests/adr0088_feed_by_url_tests.rs`
`a_retired_guid_gives_404_with_superseded_by`, and the two classification
tests of `tests/adr0051_classify_tests.rs`.

**New tests.** A revert changes the GUID back. A new GUID that a different
record holds gives `record_conflict`. The schema test of §2.

## Alternatives Considered

The tables of each question give the options. One more:

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
- A GUID change keeps the record row and each `track_guid`.
- `superseded_by` never selects a payment route.

## Acceptance

Mechanical. Each criterion is a test in the task file:

1. A free new GUID at the source URL changes the record in one transaction.
   The record count does not change.
2. After the change, no column of the schema list holds the old GUID for this
   record.
3. `GET /v1/feeds/{old_guid}` gives `404` with `superseded_by`.
4. A revert changes the GUID back, and deletes the link to itself.
5. A new GUID held by a different record gives `record_conflict`.
6. A replica that applies `FeedGuidChanged` has the same rows as the primary.

Visual: none.

## Open Questions

- The operator selects one option for each of §1 to §4.
- §5 keeps `GET /v1/guid-changes` with an empty list. The operator can select
  its removal.

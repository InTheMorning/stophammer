# ADR 0052 Phase Plan 2: Moves And GUID Changes

Owner: [ADR 0052](../adr/0052-a-source-moves-its-own-feed.md). This plan
states no rule. It is phase 6 of the
[remediation plan](feed-trust-remediation-plan.md). Phase plan 1, the self
link, is [complete and deployed](adr-0052-self-link-phase-plan.md).

## Goal

A source URL moves its record with a permanent redirect or with
`itunes:new-feed-url`. A GUID change at the source URL is pending and public.
It applies when the new GUID is the UUIDv5 of the source URL, or when the
operator approves it.

## Non-Goals

- The check of ADR 0054 on the move target. It applies when ADR 0054 exists.
- An alias of the old GUID. ADR 0052 defers it.
- A move by a signal of section 3 of ADR 0052.

## Assumptions

- The crawler uses the default redirect policy of `reqwest`. It keeps the
  final URL and not the status of each hop.
- `IngestFeedData` is defined in `stophammer-parser/src/types.rs`, and the
  crawler sends it. The node has its own copy in `src/ingest.rs`.
- The self-link move of task 001 and task 003 is in `handle_ingest_feed`.
- A retirement of ADR 0053 writes blocks unless the caller asks for none.
- The next migration is `0041`.

## Decisions For The Tasks

1. **The parser.** `IngestFeedData` gets `new_feed_url: Option<String>` from
   the channel `itunes:new-feed-url`, `locked: Option<bool>` and
   `locked_owner: Option<String>` from `podcast:locked`. Trimmed. An empty
   value is `None`. The fields are additive.
2. **The redirect hops.** The crawler records each redirect hop as `{ url,
   status }` with a custom `reqwest` redirect policy, in the order of the
   hops. The ingest request gets `redirects`, a list, empty when there was no
   redirect. After a `304`, the crawler sends the hops of the current
   response.
3. **The follow list.** The crawler adds a `new_feed_url` value to its follow
   list, as it adds a publisher link.
4. **The node contract.** `src/ingest.rs` adds `redirects` to the request and
   the three fields to `IngestFeedData`, each with `#[serde(default)]`. An
   older crawler sends none, and the node moves nothing by those triggers.
5. **The stored targets.** Migration `0041` adds `declared_new_feed_url`,
   `podcast_locked` and `locked_owner` to `feeds`. Only an ingest in the update
   or new-feed case writes them, as for `declared_self_url`. Each relocation
   clears `declared_new_feed_url` with `declared_self_url`.
6. **One move function.** The self-link move, the new-feed move and the
   redirect move use one function for the move itself. Each move logs its
   trigger: `self link`, `new-feed-url` or `permanent redirect`.
7. **The redirect move.** The node moves the record to `canonical_url` and
   applies the content when all of these are true:
   - `source_url` is the stored source URL.
   - The list has one or more hops, and each hop is `301` or `308`.
   - The body declares the GUID of the record.

   A `302` or `307` hop applies the content and keeps the source URL. A move
   target that is the source URL of a different record answers
   `record_conflict` and moves nothing.
8. **The pending GUID change.** A table, `feed_guid_changes`, with one row for
   each source URL: `old_guid`, `new_guid`, `first_seen`, `last_seen`, and the
   decision columns. A new or changed row signs `FeedGuidChangeObserved`. The
   decision signs `FeedGuidChangeDecided`. `last_seen` is local to the
   primary.
9. **The automatic case.** The new GUID is the UUIDv5 of the source URL, by
   `guid_origin_matches` of ADR 0058.
10. **The transition.** In one transaction: retire the old record with reason
    `guid_superseded` and **no block**, sign `FeedGuidSuperseded { old_guid,
    new_guid, source_url }`, and admit the new record from the body. A block
    would block the source URL, and a revert would then be impossible.
11. **The public read.** `pending_guid_change` in the feed response.
    `GET /v1/guid-changes`. `GET /v1/feeds/{old_guid}` answers `404` with
    `superseded_by`.
12. **The operator route.** `POST /v1/feeds/{guid}/guid-change`, admin token,
    `{ decision: approve | reject, reason }`.

## Sequence

| Task | Repository | Needs |
|---|---|---|
| [004](../tasks/adr-0052-task-004-parser-move-fields.md) Parser fields | `stophammer-parser` | — |
| [005](../tasks/adr-0052-task-005-crawler-redirects.md) Crawler hops and follow | `stophammer-crawler` | 004 |
| [006](../tasks/adr-0052-task-006-node-move-triggers.md) Node move triggers | `stophammer` | 003 |
| [007](../tasks/adr-0052-task-007-node-guid-changes.md) Node GUID changes | `stophammer` | 006 |
| [008](../tasks/adr-0052-task-008-deploy.md) Deploy | VPS | 004 to 007 |

Task 004 can run beside task 003, because they change different
repositories. Task 005 can run beside task 006 for the same reason.

## Schema And API Implications

- Migration `0041`: three columns on `feeds`, and the table
  `feed_guid_changes`.
- Three event types: `FeedGuidChangeObserved`, `FeedGuidChangeDecided` and
  `FeedGuidSuperseded`.
- The ingest request gets `redirects`, and `IngestFeedData` gets three
  fields. All are optional.
- One public route, one admin route, two response fields.

## Risk Areas

- **A wrong redirect.** A host that answers `301` to an error page. The GUID
  check stops a move to a body with a different GUID.
- **A block on a GUID change.** Decision 10.
- **The crawler order.** The node deploys first, because it accepts the new
  fields as optional. The crawler follows.

## Test Strategy

- The Guards of ADR 0052, one test file for each node task.
- The Doerfelverse shape: the same URL, a new GUID that is not the UUIDv5 of
  the URL, and the same five item GUIDs. The change stays pending, and a
  return to the old GUID deletes the row.
- The replica test for each new event type.

## Rollback

Deploy the previous images. The columns, the table and the events stay, and
those binaries ignore them. A GUID change that applied stays applied.

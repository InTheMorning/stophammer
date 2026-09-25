# Feed Trust Remediation Plan

Date: 2026-09-24.

Status: Plan. No task is started. This plan states no rule. Each rule belongs
to the ADR that the task names:

- [ADR 0051](../adr/0051-feed-content-comes-from-its-source-url.md): feed
  content comes from its source URL.
- [ADR 0052](../adr/0052-a-source-moves-its-own-feed.md): a source moves its
  own feed.
- [ADR 0053](../adr/0053-a-correction-stays-applied.md): a correction stays
  applied.
- [ADR 0054](../adr/0054-a-fetch-reaches-only-public-feed-hosts.md): a fetch
  reaches only public feed hosts.
- [ADR 0055](../adr/0055-the-primary-fetches-what-it-signs.md): the primary
  fetches what it signs.

The findings are in
[the security audit](../reviews/security-audit-2026-09-24-feed-trust.md),
[the conflict evidence](../reviews/feed-identity-conflict-evidence.md),
[the conflict analysis](../reviews/feed-identity-conflict-analysis.md) and
[the Podcast Index evidence](../reviews/feed-identity-podcastindex-evidence.md).

## Sequence

Each phase needs the one before it. A phase is complete when its mechanical
criteria pass, its gate is green in each changed crate, and it is deployed.

1. ADR 0051. It closes the takeover by a copied GUID now.
2. The repair pass of ADR 0051 section 6.
3. The ADR 0018 defects. No new ADR is necessary.
4. ADR 0053.
5. ADR 0054.
6. ADR 0052.
7. ADR 0055.

Phases 3, 4 and 5 do not depend on each other. They can run in any order
after phase 2.

When the operator accepts an ADR, the same commit changes its status, the
index, `AGENTS.md`, and each earlier ADR that it narrows. ADR 0051 narrows
ADR 0005 and ADR 0015.

## Phase 1: Content Comes From The Source URL (ADR 0051)

Crates: `stophammer`, `stophammer-crawler`.

The [ADR 0051 phase plan](adr-0051-source-url-phase-plan.md) gives the task
packets.

Tasks:

- The node checks `crawl_token` before any database read. `crawl_token` leaves
  `VERIFIER_CHAIN`.
- The primary does not start with an empty `CRAWL_TOKEN`.
- The writer classifies each submission with the five cases of ADR 0051
  section 2, in the ingest transaction.
- `IngestResponse` adds the optional `source_url` field, and the OpenAPI
  document declares it.
- The crawler does not store `source_conflict`, `record_conflict` or
  `guid_change_pending` as the node answer in its fetch cache.

Mechanical criteria. Each one is a test in the named crate:

| Criterion | Crate |
|---|---|
| A mirror body with a held GUID changes no feed, track or route row, and adds one observation | `stophammer` |
| A redirect from the source URL to a new `canonical_url` applies the content | `stophammer` |
| A held URL with a new GUID returns `guid_change_pending` and changes no row | `stophammer` |
| A held URL that declares the GUID of a different held record changes neither record and no observation | `stophammer` |
| `crawl_token` in `VERIFIER_CHAIN` fails startup. An empty `CRAWL_TOKEN` fails startup | `stophammer` |
| A rejection of ADR 0051 section 2 does not change the artist-credit row count | `stophammer` |
| The ingest schema in the OpenAPI document has `source_url` | `stophammer` |
| A conflict answer is not stored as the node answer, so a `304` sends the kept body again | `stophammer-crawler` |

The tests in `tests/adr0049_url_observation_tests.rs` that expect content from
a second URL change in the same commit. The tests that expect an observation
stay.

## Phase 2: Repair Existing Records (ADR 0051 Section 6)

This phase runs on the VPS after phase 1 is deployed.

1. Make a consistent backup of the primary database.
2. List the candidate records. A candidate has an observation at a URL that
   is not its source URL. On 2026-09-24, 1,619 feeds had more than one
   observed URL. Most of them are true mirrors.
3. Apply each source URL body again with `force_reingest`. Replay the fetch
   cache of a recent `refresh` pass (ADR 0051 task 006), or run
   `refresh --force`.
4. For each candidate, compare the title and the payment routes before and
   after the pass. Record each changed record in a review record.
5. Make sure that each community node has the same routes as the primary for
   a sample of the changed records.

Mechanical criterion: after the pass, a fetch of each source URL from a
sample of candidates gives the routes that the API reports.

Manual criterion: an operator reads the list of changed records and decides
if any change needs a publisher contact. This check has no test, because the
decision needs a person.

## Phase 3: ADR 0018 Defects

Crate: `stophammer`. These fixes enforce ADR 0018 as written. They need no
new ADR.

| Defect | Evidence | Fix |
|---|---|---|
| Proof accepts `podcast:txt` in an item | `src/proof.rs:405` searches all descendants. A probe on 2026-09-24 reproduced it | Read only a `podcast:txt` that is a direct child of the RSS channel |
| A new challenge cancels the pending challenge of a different requester | `src/api.rs:3670`. A probe on 2026-09-24 reproduced it | Needs a design that also stops an attacker who fills the challenge slots. Commit `73dffce` added the replacement to stop that attack |
| Expiry is checked before the fetch, not when the token is issued | Code inspection | Check the expiry again in the transaction that issues the token |
| A change of the URL from A to B and back to A escapes the check at token issue | Code inspection | Compare a revision number of the source URL, not the URL string |

Mechanical criteria: one test for each row. The test for the challenge row
shows that a publisher can complete a proof while a second requester sends
challenges.

The challenge fix needs a design decision first. If that design adds a
rule, it goes in an amendment to ADR 0018.

## Phase 4: Durable Corrections (ADR 0053)

Crates: `stophammer`.

Tasks:

- The `feed_blocks` table, the `FeedBlocked` and `FeedUnblocked` events, the
  admin routes, and the block check before the verifier chain.
- `DELETE /v1/feeds/{guid}` blocks by default. `?block=false` does not block.
- At startup, the environment blocklist becomes block rows.
- The `stale_submission` rule for `last_build_date`.
- `GET /v1/feeds/{guid}/route-history` and the `warn` log for a change of
  recipients.

Deployment gate: each community node runs a version that knows the two new
event types before the primary emits one.

Mechanical criteria are the Guards of ADR 0053.

## Phase 5: Fetch Safety (ADR 0054)

Crates: `stophammer`, `stophammer-crawler`.

Tasks:

- The address rule of ADR 0054 section 1 in the crawler, with DNS pinning and
  a check of each redirect hop.
- The body limit, read as a stream, and the redirect limit of 5.
- The follow limits of ADR 0054 section 3.
- The scheme check of URL fields at ingest.
- The case list of ADR 0054 section 5 as a test in each crate.

Before the body limit is final, measure the largest feed body in the index.

Manual criterion: the operator examines the network rules of the crawler
container on the VPS. ADR 0006 asks for that isolation. No test can see a
firewall rule, so this check stays manual. Until it runs, report it as open.

## Phase 6: Moves And GUID Changes (ADR 0052)

Crates: `stophammer-parser`, `stophammer-crawler`, `stophammer`. This is a
change across three repositories, so each repository gets one commit that
names ADR 0052.

Tasks:

- The parser reads `itunes:new-feed-url` and `podcast:locked` into typed
  fields.
- The crawler records each redirect hop with its status, and adds
  `redirects` and `new_feed_url` to the ingest request. After a `304`, the
  crawler reports the hops of the current response, not the final URL of the
  kept body.
- The crawler adds a `new_feed_url` value to its follow list.
- The node applies the two move triggers, the pending GUID change table, the
  24-hour rule, and the transition of ADR 0052 section 5.
- `PATCH /v1/feeds/{guid}` returns `409` when the new URL is the source URL of
  a different record.
- The `FeedGuidSuperseded` event, and `superseded_by` on
  `GET /v1/feeds/{guid}`.

Deployment gate: each community node knows `FeedGuidSuperseded` before the
primary emits one.

Mechanical criteria are the Guards of ADR 0052. Add one test for the
Doerfelverse shape: the same URL, a new GUID and the same five item GUIDs.

## Phase 7: Nominators And The Fetch Worker (ADR 0055)

Crates: `stophammer`, `stophammer-crawler`.

The transition of ADR 0055 section 7 gives three steps. Each step is one
deployment. Compare the result of each moved mode with the old path for one
full pass before the next mode moves.

Before this phase starts, measure how many requests a normal day sends to
Wavlake. ADR 0055 must not increase that number.

## Open Measurements

| Measurement | Needed by |
|---|---|
| The largest feed body in the index, and the largest ingest request | Phase 5 body limit. The node ingest limit is 2 MiB and 500 tracks today |
| How many crawls give `guid_change_pending` after phase 1 | Phase 6 |
| If the other 50 Doerfelverse feeds changed their GUID | Phase 6 |
| How many mirror submissions arrive after phase 1 | Phase 7 queue limits |
| Requests to Wavlake for each day, and if a `304` counts against its limit | Phase 7. ADR 0050 owns the `304` question |
| What a community node on the current version does with an unknown event type | Phases 4, 6 and 7 deployment gates |

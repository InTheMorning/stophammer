# Resolver + TUI Revamp Rollout Plan

This document covers the next-stage rollout after the current feed-scoped
resolver, diagnostics API, and review binaries. The goal is to make resolver
work more visible, more incremental, and more operator-friendly without
turning the codebase into a second ad-hoc resolver living in the TUI layer.

This plan is intentionally operational:

- what to build first
- what to expose for review
- what should remain conservative
- how to roll changes out without losing trust in the corpus

Related documents:

- [resolver-refactor-plan.md](/home/citizen/build/stophammer/docs/resolver-refactor-plan.md)
- [primary-diagnostics-plan.md](/home/citizen/build/stophammer/docs/primary-diagnostics-plan.md)
- [wallet-entity-plan.md](/home/citizen/build/stophammer/docs/wallet-entity-plan.md)

## Problem Statement

The current stack is better than the original backfill-only flow, but it still
has real gaps:

- resolver can finish a batch while leaving too many "quietly unresolved"
  artist and wallet headaches
- review tooling is split between several binaries and several operator
  mental models
- the current TUIs are useful for manual cleanup, but they do not yet feel
  like one coherent operational surface
- feed authors and external observers can now inspect diagnostics, but the
  operator still needs better workflows for turning diagnostics into decisions

We need a staged revamp that improves three things together:

- resolver claim generation
- resolver observability
- operator review and override workflows

## Design Rules

- source facts remain authoritative; resolver derives state from them
- deterministic merges stay conservative
- new heuristics should usually raise claims before they auto-merge
- TUI tools must read and write durable resolver state, not shadow state
- the API, CLI, and TUI must all be thin clients over the same stored review
  and override model
- operator tooling must explain "why" before it offers "merge"

## Desired End State

By the end of this rollout, operators should be able to:

- see why an artist, wallet, feed, release, or track produced resolver work
- see all candidate links touching one entity with confidence and provenance
- review artist and wallet ambiguity from one coherent TUI workflow
- apply merge / do-not-merge / force-link decisions durably
- rerun incremental resolver work and see the consequences clearly

And feed authors or friendly downstream tools should be able to:

- inspect read-only diagnostics
- understand what metadata inconsistency created the ambiguity
- understand what changes would likely lead to deterministic convergence next
  time

## Phase 0: Stabilize Current State

Goal:

- make today's resolver and review surface trustworthy enough to build on

Scope:

- keep improving incremental wallet and artist claim generation
- make resolver logs and counters explicit enough to spot "queue drained, but
  nothing interesting happened"
- close obvious diagnostics blind spots such as unlinked feed wallets on
  artist pages
- ensure TUI tools always read the same review and override tables the
  resolver writes

Done:

- `resolverctl status` shows `review_artist_identity_pending` and
  `review_wallet_pending` counts
- `stophammer-resolverd` startup diagnostic prints review counts when > 0
- batch-completed log includes artist seed/candidate/merge counts and wallet
  review item counts

Remaining:

- close diagnostics blind spots (unlinked feed wallets on artist pages)
- verify a full `re-resolve` visibly produces both artist and wallet review
  items on a real corpus
- confirm TUI tools read the same override tables the resolver writes (no
  shadow state)

Exit gate (on a real corpus, not just tests):

Run `stophammer-resolverctl re-resolve` against the production database and
confirm all of the following:

- `resolverctl status` shows non-zero `review_artist_identity_pending` AND
  non-zero `review_wallet_pending`
- both TUIs can list and inspect the review items those counts reflect
- no review item exists that is visible in a TUI but invisible in
  `resolverctl status` or the diagnostics API (no shadow state)
- the batch-completed log for the re-resolve run reports non-zero
  `artist_candidate_groups` and `wallet_review_items_created`

## Phase 1: Normalize Review State

Goal:

- create one durable review model shared by resolver, CLI, API, and TUIs

Scope:

- standardize the review item shapes for:
  - artist identity
  - wallet identity
  - later, release/recording/source claims if needed
- define shared fields across review items:
  - `status`
  - `source`
  - `confidence`
  - `evidence_key`
  - `note`
  - `target_entity_id`
  - override state
- make sure every review item can be traced back to:
  - feed(s)
  - source claims
  - entity rows
  - resolver heuristic

TUI impact:

- current TUI tools should stop inventing display-only concepts that are not
  represented durably
- a review item shown in a TUI must map directly to something the API can also
  return

Schema target:

The artist pattern is the reference. `wallet_identity_review` aligns to it.
No polymorphic union table — the two domains have different entity keys and
JSON shapes, so they stay as separate tables with a shared column contract.

Shared columns (both tables):

| Column | Type | Notes |
|--------|------|-------|
| `source` | TEXT NOT NULL | which heuristic created the item |
| `evidence_key` | TEXT NOT NULL | specific evidence that triggered it |
| `status` | TEXT NOT NULL | `pending` / `merged` / `blocked` / `resolved` |
| `created_at` | INTEGER NOT NULL | epoch seconds |
| `updated_at` | INTEGER NOT NULL | epoch seconds, replaces wallet's `resolved_at` |

Entity-specific columns:

- artist: `feed_guid`, `name_key`, `artist_ids_json`, `artist_names_json`
- wallet: `wallet_id`, `wallet_ids_json`, `endpoint_summary_json`

Migration path for `wallet_identity_review`:

- rename `review_type` → `source`
- add `evidence_key` (NOT NULL, populate from existing `review_type` +
  `wallet_id` for legacy rows)
- replace `details` text blob with `wallet_ids_json` (array of wallet IDs
  involved) and `endpoint_summary_json` (structured endpoint evidence)
- add `merged` to status CHECK constraint
- rename `resolved_at` → `updated_at`, backfill from `resolved_at` or
  `created_at` for existing rows

The `artist_identity_review` table requires no schema changes. The artist
pattern is already the target.

Success criteria:

- one review item can be fetched and explained identically via CLI, API, and
  TUI using the same column names
- operator actions change durable review state instead of only affecting one
  tool's local interpretation

## Phase 2: Raise More Claims, Not More Auto-Merges

Goal:

- make resolver notice more likely problems without becoming reckless

Scope:

- expand review-only heuristics before broadening auto-merge logic
- candidate heuristics, in recommended priority order:
  1. track-author vs feed-artist disagreement — first implementation slice;
     fits the current artist review model cleanly
  2. wallet-alias and artist-name normalization collisions
  3. collaboration-credit detection (`feat`, `with`, `and`) — valuable, but
     deferred until review state can represent credit-splitting cleanly
  4. same-feed name variants from contributor and author fields
  5. cross-feed alias families (most complex, save for later)
- represent these as new review sources rather than hidden side effects

First implemented heuristic: track-author vs feed-artist disagreement.

Current state: feed artist credits and track artist credits are preserved
separately, but the resolver only notices some same-feed variant cases when a
wallet alias or stronger cross-feed evidence happens to connect them.

What to build:

- a feed-scoped resolver heuristic that groups artist IDs when:
  - one artist comes from the feed credit
  - one artist comes from a track credit on that same feed
  - their normalized similarity keys collapse to the same stripped key
- for each match, raise a review item — do not auto-merge
  - `source`: `track_feed_name_variant`
  - `evidence_key`: `feed_guid`
  - entity JSON: existing artist IDs and names already stored in the current
    artist review table
  - `status`: `pending`
  - confidence remains implicit in the source for now; explicit confidence
    becomes a Phase 1 follow-up
- wire into the resolver's feed-scoped pass and the diagnostics plan builder
  so the same review item appears in:
  - resolver queue output
  - diagnostics API
  - CLI / TUI review surfaces

What it does NOT do:

- auto-merge artists
- change the ingest path
- parse multi-artist collaboration strings into `artist_credit_name` rows

Deferred follow-up: collaboration-credit detection.

That remains valuable, but it is a worse first slice because it is not a
normal "these two artist rows may be the same" problem. It is really a
credit-splitting problem, and needs richer review state before it can land
cleanly.

Confidence model:

- start with deterministic confidence bands:
  - `high_confidence`
  - `review_required`
  - `blocked`
- if we later add a probabilistic scorer, it should write its confidence as
  explicit review metadata, not bypass review by magic

Success criteria:

- operators see more pending review items for real ambiguous cases
- duplicate entity complaints become explainable by review rows, not mystery
- auto-merge rate stays conservative

## Phase 3: Write-Side Review APIs

Goal:

- let the website and future tooling resolve review items without requiring the
  CLI or TUI as the only write surface
- establish the shared write path before TUI consolidation so TUI actions are
  built on it from day one

Scope:

- add admin-gated write endpoints for the four actions below
- keep read-only diagnostics open for now
- record actor, timestamp, and rationale on every write

Write-action contract:

**merge** — "these entities are the same; combine them."

- writes an override row with `override_type = 'merge'` and `target_entity_id`
  pointing to the surviving entity
- artist path: insert into `artist_identity_override`, then re-resolve the
  affected feed — resolver applies the merge via `merge_artists_sql()` and
  emits a signed `artist_merged` event; review status becomes `merged`
- wallet path: insert into `wallet_identity_override`, then
  `apply_wallet_merge_overrides_with_recorder()` executes the merge with full
  audit in `wallet_merge_apply_batch` / `wallet_merge_apply_entry`; review
  status becomes `merged`
- the override is durable: future resolver runs see it and skip re-raising
  the same candidate group

**do_not_merge** — "these are NOT the same; stop suggesting this."

- writes an override row with `override_type = 'do_not_merge'`
- artist path: insert into `artist_identity_override`; re-resolve marks the
  review `blocked`
- wallet path: insert into `wallet_identity_override`; review becomes
  `resolved` (should become `blocked` after Phase 1 alignment)
- the override is durable: future resolver runs see it and suppress the
  candidate group

**force_link** — "assert a relationship the resolver did not find."

- writes an override row with a type-specific `override_type`:
  - artist: `force_merge` (new — currently only merge/do_not_merge exist on
    the artist side; this extends the artist override types to allow forcing
    a merge the resolver never proposed)
  - wallet: `force_artist_link` or `force_class` (already exist in
    `wallet_identity_override`)
- no review item is required as input — the operator can force a link
  directly from a diagnostics view
- re-resolve applies the forced relationship and emits the corresponding
  event

**undo** — "reverse a previous override."

- deletes the override row (artist) or marks it undone (wallet, which has
  `wallet_merge_apply_batch.undone_at` for audit)
- re-resolve reprocesses the affected entities without the override
- if the original action was a merge, undo must reverse the entity merge:
  - wallet: restore from the JSON snapshots in `wallet_merge_apply_entry`
    (`old_wallet_json`, `old_endpoint_ids_json`, etc.)
  - artist: restore from the `artist_merged` event payload
    (`source_artist_id`, `aliases_transferred`) — requires implementing
    `unmerge_artists_sql()`
- the review item reverts to `pending`
- undo of a do_not_merge simply deletes the override; the candidate group
  becomes eligible for review again

Shared requirements for all four actions:

- every write records `actor` (who), `rationale` (why), and `timestamp`
- every write is exposed identically via API endpoint, CLI command, and TUI
  action — same DB helper, same validation, same audit trail
- the API endpoint is admin-gated; read-only diagnostics remain open

Why before TUI consolidation:

- if the TUI writes directly to the DB first, rerouting through APIs later is
  rework
- the write API layer defines the action semantics that all surfaces (CLI, TUI,
  web) share

Success criteria:

- web UI, CLI, and TUI can all apply the same override semantics through the
  same code path
- every write action is auditable with actor, rationale, and timestamp
- undo is possible for all four actions where the entity merge has not
  propagated beyond the local node

## Phase 4: TUI Consolidation

Goal:

- make the TUI feel like one resolver workbench instead of several unrelated
  binaries

Scope:

- unify the navigation model across:
  - `review_artist_identity_tui`
  - `review_wallet_identity_tui`
  - `review_source_claims_tui`
- define a shared interaction pattern:
  - queue view
  - detail pane
  - evidence pane
  - action pane (backed by Phase 3 write APIs)
- add consistent concepts:
  - pending / blocked / resolved
  - confidence
  - redirected entities
  - feed / track / wallet provenance
- support jumping between related entities:
  - artist -> feed diagnostics
  - wallet -> touching feeds
  - review item -> source evidence

Non-goal:

- do not collapse everything into one enormous binary immediately

Expected rollout:

1. shared helper library for TUI rendering and state
2. consistent screens and terminology
3. optional later convergence into one `stophammer-review-tui`

Success criteria:

- an operator can stay inside the TUI while moving from symptom to evidence to
  decision
- TUI mental model matches the public diagnostics endpoints closely

## Phase 5: Operator Workflow Revamp

Goal:

- make day-to-day resolver operation predictable

Scope:

- define explicit queues/views:
  - hottest feeds creating the most review churn
  - highest-confidence unresolved artist links
  - highest-confidence unresolved wallet links
  - newly created review items
  - long-stale review items
- add summary counters and trend reporting
- add operational playbooks:
  - after import
  - after resolver heuristic change
  - after large feed repair

Success criteria:

- operators can answer "what should I review next?" immediately
- after a deploy or re-resolve, the operator can see what got better and what
  got noisier

## Phase 6: Probabilistic Scoring Prototype

Goal:

- borrow from Splink/OpenAlex-style workflows without surrendering control

Scope:

- introduce one scored review source first, not a general AI merger
- recommended first targets:
  - `likely_same_artist`
  - `likely_wallet_owner_match`
- inputs may include:
  - normalized names
  - wallet aliases
  - shared feed neighborhoods
  - contributor claims
  - platform claims
  - publisher relations
  - conflicting external IDs
- output must remain review-oriented:
  - confidence
  - explanation
  - evidence
  - recommended action

Guardrails:

- no silent high-impact merges from the probabilistic layer initially
- every scored claim must remain inspectable in diagnostics and TUI

Status:

- first review-first scored source is now live as `likely_same_artist`
- wallet-side review-first scored source is now live as `likely_wallet_owner_match`
- current implementation is intentionally narrow:
  - same feed only
  - requires at least two agreeing evidence families among feed/track credit
    variants, contributor variants, and wallet-name variants
  - still writes a normal pending artist review item, never an automatic merge
  - wallet-side source requires same normalized alias plus shared-feed evidence,
    and still writes a normal pending wallet review item, never an automatic merge

Success criteria:

- the system raises better review candidates than today
- operators can understand why the scorer made the suggestion
- false positives stay acceptable because the action remains review-first

## Rollout Order

Recommended sequence:

1. finish Phase 0 cleanup work that blocks trust
2. normalize review state in Phase 1
3. raise more review claims in Phase 2
4. add write-side review APIs in Phase 3
5. consolidate the TUI in Phase 4
6. tighten operator workflows in Phase 5
7. prototype probabilistic scoring in Phase 6

Why this order:

- better heuristics without better review surfaces creates noise
- write-side APIs before TUI consolidation ensures the TUI's action layer is
  built on the durable shared write path from day one
- better write APIs without normalized review state creates drift
- probabilistic scoring before trustworthy review workflows would magnify
  confusion

## Explicit Deferrals

These should not block the rollout:

- community-node write-side review
- fully automatic broad fuzzy merges
- moving all review workflows to the web UI first
- replacing all current maintenance binaries at once

## First Concrete Deliverables

The next practical slices:

1. close remaining Phase 0 items (diagnostics blind spots, `re-resolve`
   verification, TUI/override table alignment)
2. document current review table schemas and define the migration path to
   align `wallet_identity_review` with the artist pattern (Phase 1 entry)
3. add collaboration-credit detection as the first review-only heuristic
   after the simpler track/feed variant slice has proven out (Phase 2 follow-up)
4. add one admin write endpoint for artist review resolution (Phase 3 entry)

That gives us a real vertical slice:

- resolver raises a better claim
- diagnostics expose it
- operator resolves it via API
- override is durable
- TUI consolidation (Phase 4) then builds on all of the above

Current scoring and conflict policy are documented in [identity-evidence-policy.md](./identity-evidence-policy.md).

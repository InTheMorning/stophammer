# Plan: Conservative Resolver Cleanup And Rebuild

Last verified against HEAD on 2026-03-22.

## Goal

Make the resolver conservative and convergent before rebuilding the database.

The target end state is:

- artist identity uses only evidence that is defensible today
- canonical release/recording clustering does not depend on
  `publisher_guid`
- resolver passes run in an order that can converge without stale promoted
  artist IDs
- a fresh rebuild produces cleaner artist and canonical identities than the
  current database

## Scope

In scope:

- remove bad identity heuristics
- tighten resolver flow
- fix worker pass ordering
- add safe orphan cleanup
- rebuild the database after code changes land
- compare old/new database outputs before treating the rebuild as canonical

Out of scope for this plan:

- wallet-recipient heuristics
- contributor-claim heuristics
- platform-specific identity adapters
- DNS-based ln-address verification

Those are future enrichment projects. They should not be mixed into the
correctness pass.

## Soundness Constraints

The plan is only sound if these rules hold:

1. `publisher_guid` is never treated as artist identity.
2. Strong artist anchors come from explicit feed-scoped evidence only:
   - a single verified feed-level `nostr_npub`
   - a feed website URL
   - normalized website grouping as corroboration, not as a replacement for
     explicit feed data
3. Canonical state is built before artist identity consumes release-cluster
   evidence.
4. Artist promotions are refreshed after artist merges, not before.
5. Orphan cleanup only deletes artists and credits with zero live references.

## What The Current Code Actually Does

Verified against HEAD on 2026-03-22.

### Worker pass order in `src/resolver/worker.rs`

Current order (lines 120–159):

1. source read models
2. canonical state
3. canonical promotions  ← runs before artist merges; promotions are stale
4. canonical search
5. artist identity

This is wrong. Canonical promotions call
`collect_high_confidence_artist_external_ids_for_feed` (see
[`src/db.rs:8491`](/home/citizen/build/stophammer/src/db.rs#L8491)), which
reads the current feed credit's artist owner. Artist merges happen after, so
promotions are computed against pre-merge artist ownership.

### Current problematic identity uses

All of the following are confirmed still present in the code:

- [`resolve_feed_artist_from_source_claims`](/home/citizen/build/stophammer/src/db.rs#L507)
  still reuses artists by `publisher_guid` (lines 534–550)
- [`find_existing_artist_by_publisher_guid_and_name`](/home/citizen/build/stophammer/src/db.rs#L466)
  still exists and is called during feed ingest
- [`collect_artist_groups_by_publisher_guid`](/home/citizen/build/stophammer/src/db.rs#L4845)
  still creates merge groups from `publisher_guid` and is called in
  `backfill_artist_identity`
- [`artist_has_strong_identity_claims`](/home/citizen/build/stophammer/src/db.rs#L4983)
  still counts `publisher_guid` presence as strong evidence
- [`collect_artist_groups_by_anchored_name`](/home/citizen/build/stophammer/src/db.rs#L5030)
  still requires `strong && feed_count >= 2` to anchor (line 5061)
- [`preferred_artist_target`](/home/citizen/build/stophammer/src/db.rs#L5102)
  still ranks by `(feed_count, created_at, artist_id)` — no evidence weight
- [`feed_artist_evidence_key`](/home/citizen/build/stophammer/src/db.rs#L1517)
  still falls back to `publisher_feed_guid:{guid}` before `artist_credit_display`
  (lines 1530–1540)
- `cleanup_orphaned_artists` does not exist

## Database Facts Supporting The Plan

Reviewed in `stophammer.db` on 2026-03-22.

### Baseline counts

| Metric | Count |
|--------|-------|
| Total artists | 2,073 |
| Artists with feeds | 2,032 |
| Artists with no feed attachment | 41 |
| Truly unreferenced artists | 21 |
| Orphan artist credits | 4 |
| Single-feed artists | 1,260 |
| Multi-feed artists | 772 |
| Artists with feed-level npub | 10 |
| Total feeds | 6,895 |
| Feeds falling back to `publisher_guid` in `feed_artist_evidence_key` | 6,051 |
| Feeds falling back to `artist_credit_display` | 828 |
| Feeds using a single feed-level npub evidence key | 16 |

### Anchored-name gate is too strict

The current `feed_count >= 2` rule is blocking plausible merges:

- `984` single-feed artists already have explicit website or npub evidence
- `79` repeated-name groups include at least one such single-feed strong artist
- `9` repeated-name groups match the exact intended pattern:
  one strong single-feed artist plus one or more weak single-feed duplicates

Concrete examples from the database:

- `A Cold Trip Nowhere`
- `Daves Not Here`
- `False Finish`
- `Perceptronik`
- `Rusty Gate`

This is enough evidence to justify anchoring on strong evidence alone.

### `publisher_guid` is wrong, but the breakage is mostly latent

Current database observations:

- `0` same-name multi-artist review groups currently come from
  `(LOWER(display_name), publisher_guid)`
- `0` `artist_identity_review` rows currently use `source = 'publisher_guid'`

That means the current DB is not yet full of visible publisher-guid mistakes.
The reason to remove it is semantic correctness and future safety, not because
the current review table is already noisy.

### Canonical blast radius is large

Current canonical maps:

| Match type | Rows |
|------------|------|
| `exact_release_signature_v1` | 6,802 |
| `single_track_cross_platform_release_v1` | 67 |
| `feed_guid_identity_v1` | 26 |
| `exact_recording_signature_v1` | 21,727 |
| `single_track_cross_platform_recording_v1` | 67 |
| `track_guid_identity_v1` | 89 |

Because `6,051` feeds currently use `publisher_guid` as the canonical artist
evidence key fallback, removing it will materially change canonical release and
recording IDs. That is acceptable for a full rebuild, but it should be treated
as intentional churn.

## Active Plan

All three phases must land before the rebuild. They can be implemented in
parallel within Phase 1, but Phase 2 and Phase 3 both depend on Phase 1 code
being in place before the rebuild runs.

### Phase 1: Fix resolver logic and ordering

This is a single implementation batch. All five items should land together
before anything is rebuilt.

Priority within this phase:

1. Worker reorder — smallest change, immediately makes per-feed resolution
   more coherent
2. `publisher_guid` identity removal — fixes the most consequential
   semantic bug
3. Anchored-name gate loosening — unblocks 9 known merge cases
4. Merge target preference — prevents silently choosing the wrong target
5. Orphan cleanup — safe housekeeping, depends on correct identity first

#### 1a. Fix worker pass ordering

Change the `resolve_feed` function in
[`src/resolver/worker.rs`](/home/citizen/build/stophammer/src/resolver/worker.rs)
so artist identity runs before canonical promotions:

1. source read models
2. canonical state
3. artist identity   ← move here
4. canonical promotions
5. canonical search

This is a reorder of five `if dirty_mask & ...` blocks. No logic changes.

Reason: canonical promotions must see post-merge artist ownership. Reordering
is cleaner than re-marking feeds dirty after every artist merge.

#### 1b. Remove `publisher_guid` from artist identity

Delete or remove use of:

- `find_existing_artist_by_publisher_guid_and_name` and its call site in
  `resolve_feed_artist_from_source_claims` (lines 534–550)
- `collect_artist_groups_by_publisher_guid` and its call site in
  `backfill_artist_identity`
- the `publisher_guid` clause in `artist_has_strong_identity_claims`

After this change, artist identity should only treat feed-level npub and feed
website evidence as strong. `publisher_guid` may remain stored in source
tables; it must not influence artist identity decisions.

#### 1c. Loosen anchored-name gating conservatively

Update `collect_artist_groups_by_anchored_name` so:

- a group anchors when exactly one artist has strong evidence
- `feed_count >= 2` is no longer required for the anchor
- weak-side candidates remain restricted to low-confidence single-feed cases
  on the existing platform allowlist

Keep the weak-side platform restriction for now. That is the conservative
part.

#### 1d. Make merge target choice prefer evidence, not volume

Update `preferred_artist_target` so ranking is:

1. explicit identity evidence (has strong claims)
2. feed count
3. oldest row
4. stable `artist_id` tie-break

This avoids choosing the wrong target just because a noisier artist row
collected more feed attachments.

#### 1e. Add safe orphan cleanup

Add `cleanup_orphaned_artists` with this deletion rule:

- delete an artist only if no `artist_credit_name` row for that artist is
  referenced by any live `feeds`, `tracks`, `releases`, or `recordings`

Delete associated rows for the orphan artist as needed:

- `artist_aliases`
- `artist_credit_name`
- `external_ids`
- `artist_tag`
- `artist_artist_rel`
- empty `artist_credit` rows left behind

Do not delete `artist_id_redirect` targets during this pass. Redirects are
part of merge history, not orphan detection.

Current DB evidence says this cleanup can remove about `21` truly unreferenced
artists and `4` orphan credits.

### Phase 2: Remove `publisher_guid` from canonical clustering

Update `feed_artist_evidence_key` so the fallback order becomes:

1. single feed-level verified npub → `nostr_npub:{value}`
2. otherwise → `artist_credit_display:{name}`

Remove:

- the `publisher_feed_guid:{guid}` branch (lines 1530–1540 in `src/db.rs`)

This changes canonical IDs for ~6,051 feeds. That is expected and is one
reason the rebuild must happen after this code lands.

### Phase 3: Rebuild

This plan assumes a fresh rebuild, not an in-place migration.

#### Preflight

Before rebuilding:

1. Snapshot current metrics from the existing DB:
   - artist count
   - duplicate-name group count
   - orphan artist count
   - release map count by `match_type`
   - recording map count by `match_type`
   - promoted artist external-ID count
2. Land tests for the new resolver behavior.
3. Keep a short list of manual spot-check artists from the current DB.

#### Rebuild

1. Start from an empty database with the new code.
2. Re-import/re-ingest source feeds.
3. Run `resolverd` until `resolver_queue` drains.
4. Run a global `backfill_artist_identity` pass once after the queue drains.
   This is the convergence pass.
5. Rebuild canonical promotions for all feeds after the global artist backfill.
   This ensures `resolved_external_ids_by_feed` reflects post-merge artist
   owners.
6. Rebuild canonical search if needed after canonical-state convergence.

#### Postflight verification

Compare old and new databases on:

- duplicate artist-name groups
- orphan artist count
- artists with promoted npubs
- release count and recording count
- release/recording match-type distributions
- a manual sample of known multi-feed artists
- a manual sample of prior duplicate-name trouble cases

Expected outcomes:

- fewer duplicate artist rows
- fewer unreferenced artists
- more defensible artist merges
- canonical release/recording IDs that no longer depend on publisher-host data

## Required Tests

At minimum, add tests for:

- publisher GUID no longer reuses an artist during feed ingest
- publisher GUID groups no longer appear in artist-identity backfill
- a single-feed artist with one website can anchor same-name weak duplicates
- a single-feed artist with one npub can anchor same-name weak duplicates
- `preferred_artist_target` chooses the explicit-identity artist over the
  merely larger one
- resolver worker ordering keeps promotions consistent after artist merges
- orphan cleanup only deletes truly unreferenced artist and credit rows

## Future Work

After the rebuild succeeds, start a separate plan for:

- contributor-claim corroboration
- payment-route classification
- normalized identity-name utilities
- stronger website normalization and domain ownership checks

Those may become useful later, but they are not prerequisites for a sound
resolver rebuild.

# Wallet Entity Plan

This document plans a wallet-identity layer for Value-for-Value recipients such
as `Music Side Project`, `StevenB`, `Fountain`, and `Boostbot` so they can be
organized without being conflated with canonical artist or musician identity.

## Why

Current behavior stores wallet-like identities only as free-text
`recipient_name` values on:

- `payment_routes`
- `feed_payment_routes`

That is not enough once the corpus contains wallet labels that look like artist
names, platform names, and service names all mixed together.

Observed problems:

- the same wallet label can appear on multiple payment addresses
- the same payment address can appear under multiple labels
- some wallet labels overlap with canonical artist names
- service/bot recipients such as app-fee destinations do not belong in the
  artist identity graph
- route-level labels are not currently queryable or reviewable as stable
  identities

The goal is to create a first-class wallet layer that is:

- separate from `artists`
- anchored by payment endpoints instead of names
- incrementally maintainable from current ingest data
- backfillable for the existing corpus
- conservative about linking wallets to artists or organizations

## Invariants

These rules implement the primary rule from
[security-guidelines.md](security-guidelines.md): prefer reversible,
evidence-backed decisions over clever irreversible inference.

- source payment-route rows remain the authoritative extracted layer
- wallet entities are derived state built from `payment_routes` and
  `feed_payment_routes`
- wallet labels must not be inserted into the canonical `artists` layer merely
  because they share a name with a performer
- artist links require explicit or high-confidence same-feed evidence; global
  name-only matching is not sufficient
- ambiguous cross-address grouping stays unresolved unless an operator override
  says otherwise
- early passes must not lock in classifications or groupings that later passes
  cannot safely override — weak conclusions never become identity anchors
- later resolution passes may consume only: source facts, operator overrides,
  and prior high-confidence assertions; they must not build on provisional
  guesses

## Fact vs. Derived State

The wallet layer separates **observed facts** (immutable once recorded) from
**derived interpretations** (provisional, may change as evidence accumulates or
operators intervene).

Observed facts:

- normalized endpoint identity (4-tuple)
- alias observations (which labels appeared on which endpoints, with timestamps)
- route mappings (which source route rows use which endpoints)
- fee flags (from source route rows)
- split amounts (from source route rows)
- source platform claims (from feed metadata)
- provenance (which feed/track provided each observation)

Derived interpretations:

- wallet owner grouping (which endpoints belong to the same real-world entity)
- wallet classification (person_artist, organization_platform, bot_service)
- artist links (which wallet owners correspond to which canonical artists)
- display names (chosen from aliases of grouped endpoints)
- review items (ambiguous groupings flagged for operator attention)

The resolution ladder (below) processes facts first and derives interpretations
in later passes, with explicit confidence tracking at each stage. A derived
conclusion at confidence `provisional` is a hypothesis, not a settled truth.

## Target Architecture

Add a separate wallet subsystem alongside the existing source and canonical
music layers. The architecture separates the **endpoint fact layer** (stable,
exact) from the **owner layer** (derived, enriched over time).

### 1. Endpoint fact layer

Normalized endpoint records and their observed labels. This is the ground truth
from which everything else is derived.

- `wallet_endpoints`
  - one row per normalized `(route_type, address, custom_key, custom_value)`
  - the 4-tuple is the stable identity anchor — it never changes
  - `custom_key` and `custom_value` are `NOT NULL DEFAULT ''` (not nullable);
    source NULL values are coalesced to `''` at normalization time so that
    SQLite's UNIQUE constraint works correctly (SQLite treats each NULL as
    distinct, which would allow duplicate "identical" rows)
  - `UNIQUE(route_type, normalized_address, custom_key, custom_value)`
  - for keysend/node routes, `custom_key` + `custom_value` select a specific
    wallet on a shared node; all four columns are always considered together
  - `wallet_id` is a **nullable FK** to the owner layer — `NULL` until an
    owner-assignment pass groups this endpoint under a wallet

- `wallet_aliases`
  - all observed labels for an **endpoint** (not a wallet)
  - FK to `wallet_endpoints.id`
  - preserves first/last seen timestamps
  - `UNIQUE(endpoint_id, alias_lower)`
  - an alias is a fact: "endpoint E was labeled X at time T"

### 2. Route mapping layer

Map source route rows to normalized endpoints. These are facts — "source route
R uses normalized endpoint E."

- `wallet_track_route_map`
  - maps `payment_routes.id` → `wallet_endpoints.id`
- `wallet_feed_route_map`
  - maps `feed_payment_routes.id` → `wallet_endpoints.id`

Route maps preserve route-level facts, including `split` and `fee`, on the
source rows they point back to. The wallet layer may consume those as evidence,
but does not copy them into identity keys.

Route maps point to **endpoints**, not wallets. To reach the owner, queries join
through the endpoint: `route → endpoint → wallet`. This extra join is the cost
of keeping fact recording separate from owner interpretation.

### 3. Owner layer (derived)

Group endpoints under wallet owner entities. This is derived state — initially
one wallet per endpoint, progressively grouped as evidence accumulates.

- `wallets`
  - owner/group record — represents a real-world entity that controls one or
    more payment endpoints
  - stores `wallet_id`, display name, normalized display name, classification,
    classification confidence, and timestamps
  - display name is derived from the aliases of the wallet's endpoints
  - `wallet_class` carries a `class_confidence` column:

    `provisional`, `high_confidence`, `reviewed`, `blocked`
  - a wallet at `provisional` confidence is a hypothesis, not settled truth
- `wallet_id_redirect`
  - merge history for retired wallet IDs (same pattern as `artist_id_redirect`)

### 4. Enrichment layer (derived, advisory)

Conservative links from wallets to artists. These are **advisory and eventually
consistent** — not ingest-synchronous, may be stale between `--refresh` runs,
and must not be treated as authoritative for routing or payment correctness.

- `wallet_artist_links`
  - many-to-many: a wallet can link to multiple artists (e.g. collaboration
    credits, shared endpoints across feeds) and an artist can link to multiple
    wallets; `UNIQUE(wallet_id, artist_id)` prevents duplicate pairs
  - each row records provenance: `evidence_entity_type` (`'feed'` or `'track'`),
    `evidence_entity_id` (the feed_guid or track_guid that provided the
    evidence)
  - each row carries its own `confidence` state:

    `provisional`, `high_confidence`, `reviewed`, `blocked`
  - links created only when same-feed or same-track evidence is exact **and**
    the wallet's classification is at `high_confidence` or above; links must not
    be derived from provisional classifications
  - ambiguous cases stay unlinked and create review items

V1 does not introduce a separate canonical organization table. Platform/app/bot
distinctions live as wallet classes.

### 5. Review and override layer

Mirror the existing artist-identity review pattern for wallet ambiguity:

- `wallet_identity_review`
  - pending review items for same-name multi-endpoint groups, conflicting
    labels, or uncertain class/link outcomes
- `wallet_identity_override`
  - durable overrides for:

    - merge
    - do-not-merge
    - forced wallet class
    - forced artist link
    - blocked artist link

## Identity Rules

### Endpoint identity (fact layer)

Endpoint identity is address-first and exact. The normalized 4-tuple is the
stable identity anchor. It never changes, never merges, and never depends on
labels or classifications.

Normalization rules:

- `lnaddress`: trim and lowercase
- `node` / `keysend`: trim and lowercase the pubkey
- `wallet`: trim and lowercase consistently for keying while preserving the
  original source text separately

Endpoint creation is pure fact recording:

- create one endpoint row per unique normalized 4-tuple
- attach all observed `recipient_name` values as aliases on the endpoint
- do not classify, group, or interpret during endpoint creation

### Owner identity (derived layer)

Owner identity groups endpoints under a single wallet entity. Owner assignment
is a **derived, enrichable** step — not part of fact normalization.

Initial owner assignment:

- create one wallet per endpoint (provisional — one-to-one)
- assign `wallet_endpoints.wallet_id` to point to the new wallet

Later enrichment passes may group multiple endpoints under one wallet when
evidence supports it (same-feed same-name, operator merge). After grouping:

- one wallet has multiple endpoints
- the wallet's display name is derived from the first-seen non-empty alias
  across all its endpoints; tie-breaker when `first_seen_at` collides:
  `ORDER BY first_seen_at ASC, alias_lower ASC, id ASC LIMIT 1`
  (deterministic, stable under rebuild, no global counting needed)

Ambiguity handling:

- same endpoint with multiple labels stays one endpoint with multiple aliases
- same label with multiple endpoints creates multiple wallets and review items
- cross-feed or cross-platform same-name collisions stay separate until an
  operator override merges them

## Resolution Ladder

Wallet state is built in ordered passes. Each pass consumes only the outputs of
prior passes (facts, overrides, and high-confidence assertions). No pass may
build on provisional guesses from a prior pass.

### Pass 1: Normalize endpoint facts

Input: source route rows (`payment_routes`, `feed_payment_routes`).

Output:

- `wallet_endpoints` rows (one per unique normalized 4-tuple)
- `wallet_aliases` rows (observed labels per endpoint, with timestamps)
- `wallet_track_route_map` / `wallet_feed_route_map` (route → endpoint)

This pass records **only observed facts**. No wallets are created. No
classifications are assigned. No grouping is attempted.

### Pass 2: Provisional owner creation + hard-signal classification

Input: endpoint facts from Pass 1, operator overrides, source route metadata.

Output:

- `wallets` rows (initially one per endpoint)
- `wallet_endpoints.wallet_id` assigned
- `wallet_class` set from hard signals only:

  - operator override → class from override, confidence = `reviewed`
  - `fee = true` on any source route mapping to this endpoint →
    `bot_service`, confidence = `high_confidence`
  - all others → `unknown`, confidence = `provisional`

**No name-based classification in this pass.** A label like "Wavlake" or
"Fountain" does not drive classification here — it stays `unknown/provisional`
until stronger evidence arrives.

### Pass 3: Same-feed / same-track artist evidence

Input: endpoint facts, wallet assignments from Pass 2, canonical artist credits.

Output:

- `wallet_artist_links` where evidence is exact and unambiguous
- only for wallets at `high_confidence` classification or with
  `unknown/provisional` class (not for wallets classified as `bot_service` with
  `high_confidence` — those are fee/service destinations, not artists)

Constraint: this pass consumes only source facts and high-confidence wallet
assignments. It does not consume provisional classifications as input.

### Pass 4: Ambiguous candidate generation

Input: all prior outputs.

Output:

- `wallet_identity_review` items for:

  - same alias appearing on multiple wallets (cross-endpoint name collision)
  - endpoints with conflicting fee/non-fee signals across feeds
  - wallet-artist link candidates that did not meet the confidence threshold

### Pass 5: Global refresh and owner grouping

Input: all facts, all overrides, all high-confidence assertions.

Output:

- owner grouping: merge endpoints under one wallet when same-feed same-name
  evidence is strong (and no `do_not_merge` override blocks it)
- display name re-derivation across grouped endpoints
- cross-feed review items for same-name wallets that were not auto-grouped
- soft classification from known platform/app signals (label patterns, address
  domains, `source_platform_claims`) → confidence = `provisional`
- split-shape heuristics as weak evidence only:

  - repeated small-share patterns across many unrelated feeds may support
    `organization_platform/provisional` or `bot_service/provisional`
  - dominant non-fee share patterns may support `person_artist/provisional`
  - split-derived evidence may adjust only `provisional` classification; it is
    never sufficient for `high_confidence`
- orphan cleanup

This pass is the periodic `backfill_wallets --refresh` operation. It has access
to the full settled corpus and can make corpus-level decisions that per-feed
passes cannot.

## Classification Model

V1 classifies wallets into:

- `person_artist`
- `organization_platform`
- `bot_service`
- `unknown`

Each classification carries a confidence state:

- `provisional` — hypothesis based on weak signals; may change; must not be
  consumed as input by other derived passes
- `high_confidence` — derived from hard signals (fee=true, exact same-feed
  artist evidence); safe for downstream passes to consume
- `reviewed` — operator has confirmed or set the classification
- `blocked` — operator has explicitly blocked a classification change

Classification sources, ordered by confidence:

1. operator override → `reviewed`
2. explicit fee/service signals (`fee = true`) → `bot_service` at
   `high_confidence`
3. exact same-feed or same-track evidence matching the canonical artist credit
   → `person_artist` at `high_confidence`
4. known platform/app signals from label, address domain, or
   `source_platform_claims` → `organization_platform` at `provisional`
5. fallback → `unknown` at `provisional`

**Weak classifications must never become identity keys.** Wallet identity stays
anchored to normalized endpoints. Names and class guesses must not drive merges,
canonical grouping, or downstream enrichment by themselves.

## Misleading Evidence Examples

These examples motivate the evidence-first architecture and show why early
classification is dangerous:

- **fee=true route labeled with an artist-like name.** A feed sets
  `recipient_name = "Music Side Project"` on a fee route. A classification-first
  approach would label this wallet `person_artist` based on the name, then
  create an artist link. But `fee = true` is a hard signal that this is a
  service fee destination. Pass 2 correctly classifies it as `bot_service` at
  `high_confidence`, and Pass 3 skips artist linking for it.

- **artist wallet on a platform-branded feed.** A Wavlake-hosted feed has
  `recipient_name = "StevenB"` on a keysend route with a per-artist
  `custom_value`. A naive classifier might see "Wavlake node" and classify the
  wallet as `organization_platform`. But the `custom_value` distinguishes this
  as an artist-specific destination on a shared node. Pass 2 leaves it as
  `unknown/provisional`; Pass 3 checks same-feed artist credits and may promote
  it to `person_artist/high_confidence` if "StevenB" matches the feed's artist
  credit.

- **platform/service labels reused in artist-facing contexts.** A feed has
  `recipient_name = "Fountain"` on a non-fee route. This could be the Fountain
  app taking its share, or it could be an artist/podcast literally named
  "Fountain." Labeling it `organization_platform` early would prevent artist
  linking. The correct behavior: Pass 2 leaves it `unknown/provisional`, Pass 5
  may soft-classify it as `organization_platform/provisional` from known
  platform patterns, but this stays provisional and does not block future
  reclassification if an operator reviews it.

- **same name, different entities across feeds.** "Dave" appears as a wallet
  label in 50 different feeds, most of which are unrelated. Name-only inference
  would merge these into one wallet or create spurious artist links. The plan
  prohibits name-only merges or links — each feed's "Dave" stays on its own
  endpoint until same-feed evidence or operator review says otherwise.

## Inference Constraints

These rules constrain what derived passes may do:

1. **No name-only inference.** A label alone (without supporting endpoint,
   feed-level, or operator evidence) is never sufficient for: wallet
   classification, owner grouping, artist linking, or any other derived
   conclusion. This applies to all derived passes, not just artist links.

2. **Later passes consume settled inputs only.** A pass may use: source facts,
   operator overrides, and prior assertions at `high_confidence` or `reviewed`.
   It must not build on `provisional` guesses. If a prior pass left a
   classification as `provisional`, later passes must treat it as `unknown`.

3. **Endpoint identity is immutable.** The normalized 4-tuple never changes.
   Owner assignment (which wallet an endpoint belongs to) can change. Endpoint
   rows are never deleted except through orphan cleanup when the source route
   rows that created them are gone.

4. **Owner grouping requires evidence beyond names.** Same-feed same-name is
   sufficient for grouping endpoints under one wallet. Cross-feed same-name is
   not — it creates a review item. Operator merge overrides can force grouping.

5. **Split is weak evidence only.** Split percentages may contribute to
   `provisional` classification in later corpus-level passes, but must never be
   used as: an endpoint identity key, sufficient owner-grouping evidence, or
   sufficient artist-link evidence by themselves.

## API and Query Surface

Wallets are primary-only derived state. Community nodes converge via signed
resolved-state events (see
`docs/adr/0029-primary-resolved-replication-authority.md`) and do not run
resolver logic. Wallet data must not appear on replicated endpoints until
signed wallet-state events exist.

Query path for wallet-aware route responses:

```text
route → endpoint_id (via route map)
  → wallet_id (via endpoint, nullable)
    → wallet metadata (display_name, wallet_class, class_confidence)
    → artist_id (via wallet_artist_links, optional)
```

The extra join through endpoints is the cost of keeping facts separate from
derived state. It ensures that route responses remain correct even when owner
assignment is incomplete or changing.

V1 API changes (primary-only):

- add `GET /v1/wallets/{id}` as a new primary-only endpoint returning:

  - wallet metadata (including class_confidence)
  - aliases (aggregated from endpoints)
  - endpoints
  - linked artists (with link confidence)
  - recent feed/track usage summary

Gated behind replication (requires signed wallet-state events first):

- extend feed and track route responses to embed wallet references:

  - `endpoint_id`
  - `wallet_id` (if assigned)
  - `display_name`
  - `wallet_class`
  - `class_confidence`
  - optional linked `artist_id`
- adding wallet refs to existing replicated endpoints without a signed
  event story would cause community reads to diverge or return silently
  incomplete data

V1 non-goals:

- no wallet entries in default `/v1/search`
- no wallet/org mixed search surface
- no automatic wallet-to-artist joins in artist endpoints beyond route-attached
  references

## Resolver and Backfill Plan

Wallet organization should be derived state maintained the same way other
resolver-owned layers are maintained.

### Phase 1: Schema and whole-corpus backfill

Scope:

- add wallet tables (endpoint fact layer + owner layer + enrichment layer)
- add DB helpers for endpoint normalization, alias tracking, route mapping,
  provisional owner creation, hard-signal classification, and review/override
  persistence
- add a maintenance binary to rebuild wallet state for a full database
- backfill the existing corpus: run Passes 1-4 of the resolution ladder
- Pass 5 (global refresh / owner grouping) runs as a separate backfill mode

Outcome:

- every route row maps to a stable endpoint
- every endpoint has a provisional wallet owner
- hard-signal classifications are applied
- same-feed artist links are created where evidence is exact
- ambiguous groups are captured as review items
- display names, soft classifications, and owner grouping reflect the first
  global refresh

### Phase 2: Incremental resolver upkeep + signed replication

Scope:

- add a `DIRTY_WALLET_IDENTITY` bit to resolver processing (after
  `DIRTY_CANONICAL_PROMOTIONS`, before `DIRTY_CANONICAL_SEARCH`)
- mark feeds dirty for wallet recomputation whenever feed-level or track-level
  routes change
- per-feed resolver pass: run Passes 1-2 of the resolution ladder for the dirty
  feed's routes (fact normalization + hard-signal classification)
- per-feed passes do NOT run Passes 3-5 — artist linking, review item
  generation, owner grouping, and soft classification are corpus-level concerns
  handled by the backfill binary
- add `backfill_wallets --refresh` mode to run Pass 5 (global refresh) after
  major corpus changes
- emit signed wallet-state events for community node replication (required
  before wallet refs can appear on replicated endpoints)

Maintenance model:

- per-feed (ingest-synchronous): endpoint facts, aliases, route maps,
  hard-signal classifications (Passes 1-2)
- periodic backfill (eventually consistent): artist links, owner grouping,
  display name re-derivation, soft classifications, review items, orphan cleanup
  (Passes 3-5)
- artist links and owner grouping live on the slower/global path because:

  - link evidence requires cross-referencing artist credits which may not be
    fully resolved when the per-feed wallet pass runs
  - owner grouping requires corpus-level alias comparison
  - backfill has access to the settled artist layer and full alias history

Staleness contract:

- display names, artist links, owner grouping, soft classifications, and review
  items may be stale between `--refresh` runs; this is acceptable because they
  are all advisory derived state:

  - display names are cosmetic
  - artist links are enrichment hints, not routing authority — a stale or
    missing link never affects payment delivery
  - soft classifications are provisional and do not drive downstream decisions
  - review items are operator queue entries
  - none of these affect routing correctness or payment split computation
- operators should run `backfill_wallets --refresh` after:

  - bulk re-ingests (e.g. replaying feed_audit.ndjson)
  - feed retirements or blocklist changes that remove routes
  - schema migrations that alter wallet tables
  - periodically (e.g. weekly) if the corpus is actively growing
- individual feed ingests do NOT require a refresh — the per-feed resolver pass
  keeps endpoint facts and hard classifications current

Outcome:

- new ingests and edits keep endpoint facts and hard classifications current
- operators run `backfill_wallets --refresh` after bulk changes for global
  consistency
- community nodes can converge on wallet state via signed events

### Phase 3: Review tooling and API attachment

Scope:

- add a CLI review tool parallel to `review_artist_identity`
- add the primary-only `GET /v1/wallets/{id}` endpoint
- expose wallet refs in existing route response payloads (gated behind Phase 2
  signed replication — do not ship on replicated endpoints until community
  nodes can converge)

Outcome:

- operators can resolve ambiguous wallet groups safely
- client developers can inspect wallet identity without confusing it with
  artist identity

## Testing

Required coverage:

- migration tests for all wallet tables, uniqueness rules, and redirects
- endpoint fact tests showing:

  - one endpoint per normalized 4-tuple
  - keysend with distinct `custom_value` → distinct endpoints
  - same endpoint with multiple labels stays one endpoint with multiple aliases
  - alias timestamps track first/last seen correctly
- route map tests showing:

  - route maps point to endpoint_id, not wallet_id
  - route → endpoint → wallet join works for assigned and unassigned endpoints
- resolution ladder tests:

  - Pass 1 creates only facts, no wallets
  - Pass 2 creates provisional wallets, applies only hard-signal classification
  - Pass 2 does NOT classify based on name patterns (e.g. "Fountain" stays
    unknown/provisional)
  - Pass 3 creates artist links only from high-confidence wallets
  - Pass 3 skips bot_service wallets for artist linking
  - Pass 4 generates review items for ambiguous cases
  - Pass 5 groups same-feed same-name endpoints under one wallet
  - Pass 5 does NOT group cross-feed same-name without operator override
- classification confidence tests:

  - fee=true → bot_service/high_confidence
  - operator override → reviewed
  - name-only pattern → provisional (never high_confidence)
  - split-shape heuristic → provisional only (never high_confidence)
  - provisional classification does not propagate to downstream passes
- misleading evidence tests:

  - fee=true route with artist-like name → bot_service, no artist link
  - platform node with per-artist custom_value → unknown/provisional (not
    organization_platform)
  - repeated small splits across unrelated feeds → at most provisional
    platform/service classification
  - dominant non-fee split → at most provisional person_artist support
  - same name across feeds → separate wallets, review items
- artist-link tests proving:

  - global name overlap alone never creates a wallet link
  - same-feed evidence creates link; many-to-many for collaborations
  - links only created when wallet is at high_confidence or unknown (not
    provisional bot_service that was overridden)
  - ambiguous cases produce review items, not links
- wallet merge tests:

  - merge repoints endpoints/aliases/route maps/links/reviews
  - redirect chains are repointed (like `artist_id_redirect`)
  - display name re-derived from merged endpoint aliases
- backfill idempotent: running twice produces same result
- cleanup removes orphaned wallets: only wallets with no endpoints deleted

## Implementation Status

### Completed

- **Phase 0: Feed→track inheritance** — tracks without own payment_routes or
  source_contributors fall back to parent feed's data at query time
  (`src/query.rs:build_track_response`)
- **Phase 1: Schema + backfill** — migration 0016, all fact-layer and owner-layer
  helpers in `src/db.rs`, backfill binary (`src/bin/backfill_wallets.rs`),
  33 tests in `tests/wallet_entity_tests.rs`
- **Phase 2: Incremental resolver** — `DIRTY_WALLET_IDENTITY` bit wired into
  resolver worker, runs Passes 1-2 per feed
- **Phase 3: API endpoint** — `GET /v1/wallets/{id}` primary-only endpoint with
  redirect following
- **Pass 5: Owner grouping** — `backfill_wallets --refresh` mode, same-feed
  same-name grouping, review item generation, orphan cleanup

Corpus state after backfill (2026-03-22):

| Layer | Count |
|-------|-------|
| Endpoints | 25,293 |
| Wallets | 12,411 (after Pass 5 grouping) |
| Redirects | 12,882 (from merges) |
| Artist links | 309 |
| Pending reviews | 5,694 |
| Classification | 2 bot_service/high_confidence, 12,409 unknown/provisional |

### Remaining: Soft-Signal Classification

`classify_wallet_soft_signals(conn, wallet_id) -> Result<bool, DbError>`

Applies provisional classification from known platform/app signals. Only runs
on wallets that are still `unknown/provisional`. Never overrides
`high_confidence`, `reviewed`, or `blocked`.

Signal sources:

1. **Alias exact match** against hardcoded platform list (matching the existing
   `classify_platform_url` and `classify_platform_owner` functions in
   `src/api.rs:1081-1103`): fountain, wavlake, alby, breez, podcast addict,
   rss blue, rssblue, buzzsprout, podverse, podhome, justcast
2. **lnaddress domain match**: `@getalby.com`, `@fountain.fm`, `@wavlake.com`,
   `@breez.technology`

All matches produce `organization_platform/provisional`. Exact alias match
only — "Fountain Valley Podcast" does not match.

Integrates into `backfill_wallet_pass5` after display name re-derivation,
before review item generation.

### Remaining: Split-Shape Heuristics

`classify_wallet_split_heuristics(conn, wallet_id) -> Result<bool, DbError>`

Applies split-shape weak evidence. Only runs on wallets still
`unknown/provisional` after soft signals. Never produces `high_confidence`,
never creates endpoints, never auto-merges, never creates artist links.

Thresholds (as constants):

- small share: `split <= 5` (app-fee level)
- dominant share: `split >= 50` (primary recipient)
- unrelated feeds: `>= 3` distinct feed_guids

Heuristics:

1. **Repeated small non-fee share across ≥3 unrelated feeds** →
   `organization_platform/provisional`
2. **All non-fee routes have dominant share, ≤2 feeds** →
   `person_artist/provisional`

Integrates into `backfill_wallet_pass5` after soft signals.

### Remaining: CLI Review Tool

`src/bin/review_wallet_identity.rs` — new binary following the pattern of
`src/bin/review_artist_identity.rs` (473 lines).

Modes:

| Flag | Action |
|------|--------|
| (default) | List pending wallet reviews |
| `--show-review ID` | Show review + wallet detail (endpoints, aliases, feeds) |
| `--show-wallet ID` | Show wallet detail without a review |
| `--resolve-merge ID --target-wallet WID` | Store merge override, resolve review |
| `--resolve-reject ID` | Store do_not_merge override, resolve review |
| `--resolve-class ID --class CLASS` | Store force_class override, resolve review |
| `--resolve-link ID --artist AID` | Store force_artist_link override, resolve review |
| `--resolve-block-link ID --artist AID` | Store block_artist_link override, resolve review |
| `--json` | JSON output |
| `--limit N` | Limit results (default 50) |

Supporting db.rs helpers: `list_pending_wallet_reviews`,
`get_wallet_review_detail`, `get_wallet_detail`,
`set_wallet_identity_override_for_review`.

### Deferred: Signed Wallet-State Events

New `EventPayload::WalletIdentityFeedResolved` — summary event (not full
snapshot), emitted by resolver after `resolve_wallet_identity_for_feed`.
Community nodes use it as signal to run their own wallet resolution from
replicated source data. Deferred until classification rules and review workflow
stabilize.

### Deferred: RouteResponse Wallet Enrichment

Add optional `endpoint_id`, `wallet_id`, `wallet_display_name`, `wallet_class`,
`class_confidence` fields to `RouteResponse` in `src/query.rs`. Query joins
through route map → endpoint → wallet with LEFT JOINs. Gated on signed events
so community nodes have wallet data to serve.

## Open Follow-On Work

Likely later work, but not required for v1:

- richer wallet taxonomy beyond the initial three classes plus `unknown`
- wallet search/browse APIs
- canonical organization entities
- cross-feed auto-merge heuristics based on stronger evidence signals (e.g.
  wallet-specific npub links, explicit identity tags on recipient elements)

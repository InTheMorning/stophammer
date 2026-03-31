# Primary Diagnostics Plan

This document captures the plan for a primary-only diagnostics API that exposes
resolver claims, review items, and cross-entity evidence without duplicating
resolver logic in the HTTP layer.

## Why

The current CLI and TUI tools can explain:

- why artist identities remain split
- why wallets were linked or left provisional
- which feed or track facts produced a review item

That information is not yet available through a primary-only HTTP API, which
makes browser-based debugging and operator tooling harder than it needs to be.

The goal is not to create a second resolver in the API layer. The goal is to
expose the resolver's stored state and feed-scoped plans directly.

## Constraints

- diagnostics must be primary-only
- diagnostics must remain read-only
- diagnostics are temporarily public for open debugging and feed-author tooling
- write-side review actions remain admin-gated
- the HTTP layer must not reimplement merge heuristics
- responses should expose confidence and provenance, not just final merged IDs

## Data We Want To Expose

For operators, the useful questions are:

- for one feed, which artist identities are in scope right now?
- which candidate merge/review groups did resolver derive, and from what source?
- which wallets touch this feed or its tracks?
- which wallet-to-artist links exist, and with what confidence?
- which routes, contributor claims, source links, and platform claims support
  or weaken a potential identity match?

That suggests three diagnostics surfaces:

- feed-scoped diagnostics
- artist-scoped diagnostics
- wallet-scoped diagnostics

## Proposed Endpoint Family

All endpoints are primary-only and read-only. For now they are intentionally
open without `X-Admin-Token` so external tools can inspect resolver
consequences. Write-side review APIs remain a separate admin-gated concern.

Phase 1:

- `GET /v1/diagnostics/feeds/{feed_guid}`
  - feed summary
  - feed and track artist credits
  - `explain_artist_identity_for_feed(...)`
  - current `artist_identity_review` rows for the feed
  - wallets touching the feed, including:
    - wallet classification and confidence
    - wallet→artist links and confidence
    - route evidence for this feed
    - staged source claims for this feed

Phase 2:

- `GET /v1/diagnostics/artists/{artist_id}`
  - canonical artist row
  - redirected-from IDs
  - credits, feeds, tracks, releases
  - active review groups involving this artist
  - linked wallets and link confidence

Phase 3:

- `GET /v1/diagnostics/wallets/{wallet_id}`
  - wallet detail
  - endpoint facts and aliases
  - wallet review items
  - wallet→artist links and evidence
  - feeds/tracks touched by the wallet

## Architectural Rule

The API must be a thin reader over existing DB helpers.

Good:

- call `explain_artist_identity_for_feed`
- call `list_artist_identity_reviews_for_feed`
- call wallet detail / claim-feed helpers
- serialize the result

Bad:

- duplicate resolver grouping logic in `api.rs`
- invent new merge heuristics in the handler
- make the browser infer identity claims by joining unrelated endpoints

## Confidence Model

The diagnostics API should surface the confidence already present in stored
state, not synthesize a new score.

Examples:

- wallet classification confidence:
  - `provisional`
  - `high_confidence`
  - `reviewed`
  - `blocked`
- wallet→artist link confidence:
  - `provisional`
  - `high_confidence`
  - `reviewed`
  - `blocked`
- artist identity candidate groups:
  - `source`
  - `review_status`
  - override state

Later, if we need an operator-facing rollup, it should be explicitly marked as
derived presentation data rather than canonical resolver state.

## Initial Slice

Start with `GET /v1/diagnostics/feeds/{feed_guid}`.

Why this first:

- it maps directly to the current debugging workflow
- most identity confusion is easiest to understand from one feed outward
- existing helpers already expose most of the needed information
- it supports both artist and wallet investigation without forcing a full UI
  rethink first

## Non-Goals For V1

- no write APIs for review resolution in this pass
- no community-node access
- no attempt to replace the existing TUIs
- no new global fuzzy matching logic in the API layer

The first pass is about observability, not automatic decisions.

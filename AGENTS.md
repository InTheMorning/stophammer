# Stophammer Agent Guidelines

Guidelines for any agent (human or AI) making changes in this repository
and its sibling crates. Treat as authoritative. When reality drifts,
update this file in the same change that drifts it.

## Where The Work Stands

Current priority - 2026-09-23: the MusicIndex API metadata contract.

v4vmm reported four defects in the read API. Stophammer verified each one and
found each one correct. The client document is
[the change request](docs/plans/v4vmm-musicindex-api-change-request.md). The
measurements are in
[the verification record](docs/reviews/v4vmm-musicindex-api-change-request-verification.md).

Complete and deployed:

- [ADR 0042](docs/adr/0042-query-responses-name-the-field-owner.md). A response
  that carries a track reports `track_image_url` and `feed_image_url` beside
  the resolved `image_url`, and `image_url` means the same on every route. A
  search result carries the values a client needs to show a row.
- [ADR 0043](docs/adr/0043-feed-publication-date-records-its-source-element.md).
  `lastBuildDate` no longer supplies a release date, and the API returns
  `last_build_date` beside it. Migration 0034 adds the column, and
  [ADR 0046](docs/adr/0046-migration-versions-are-array-positions.md) covers
  the repair that applies it to a database the runner cannot advance.
- [ADR 0047](docs/adr/0047-a-corrective-pass-reads-the-index.md). The `refresh`
  mode reads the feed list of the node, then runs the crawl pipeline over it.
  The pass has run. Task 002 corrected `GET /v1/feeds/recent`: a feed with a
  null `newest_item_at` now sorts after each dated feed, at cursor `-1`.
  Verified on production on 2026-09-24.
- Task 001 of [ADR 0044](docs/adr/0044-api-contract-declares-its-fields.md).
  The response types derive `utoipa::ToSchema`, and the document declares 36
  schemas under `components.schemas`.
- [ADR 0049](docs/adr/0049-publisher-relationships-are-rss-facts.md), tasks 001
  to 013, with 004b, 006b and 010b. The publisher view reports each
  relationship fact by an RSS fact, never by a host rule. `resolve_listed_feed`
  also accepts the stored `feed_url`, so a new community node resolves a link
  the same way the primary node does. `rel` splits on commas into a role set.
  `GET /v1/publisher-links/stats` gives the link counts.

  The `feed` and `refresh` crawler modes follow a publisher link in three
  waves. The `gossip` mode follows in two levels, with its own throttle.
  Deployed on 2026-09-24. After the pass over the publisher feeds, the stats
  route gave 8,249 listed links: 767 resolved by GUID, 7,316 by URL and 166
  unresolved. The feeds that failed are listed in `/data/failed_feeds.txt` on
  the VPS.
- [ADR 0048](docs/adr/0048-every-track-resolves-to-a-payment-route.md). The V4V
  gate is track coverage. A feed needs a channel-level `podcast:value` block
  only when a track declares none. Deployed on 2026-09-24.
  [The evidence record](docs/reviews/adr-0048-track-value-coverage-evidence.md)
  holds the measurement that led to it.

The node at `api.musicindex.org` serves the OpenAPI document that commit
`264706e` makes. `GET /node/info` gives the revision of the running node.

The client requests of v4vmm and musicindex.org follow
[the client requests work plan](docs/plans/client-requests-work-plan.md). Wave
1 is complete and deployed on 2026-09-26: plan items 1 to 4 and item 6. Item
7 is decided. Item 5,
[ADR 0059](docs/adr/0059-an-entry-that-names-a-feed-gives-its-summary.md), is
complete and deployed on 2026-09-26. Next is item 8, which is
[ADR 0060](docs/adr/0060-a-list-feed-keeps-its-items.md), Proposed.

The work that remains:

1. Task 002 of ADR 0044. Each documented response must point at its schema. No
   response points at one today, and all 54 carry an inline shape or none.
   `QueryResponse<T>` needs one utoipa alias for each instantiation, because
   the derive removes the type parameter.
2. Task 003 of ADR 0044. The guards, and the correction of this file where it
   describes the document. The
   [phase plan](docs/plans/adr-0044-contract-schema-phase-plan.md) and the
   [review checklist](docs/reviews/adr-0044-review-checklist.md) hold the
   sequence.
3. [ADR 0050](docs/adr/0050-the-crawler-revalidates-a-feed.md) is Accepted on
   2026-09-24. Tasks 001 to 005 are complete and deployed. The crawler sends
   a conditional GET and keeps the last body, so a corrective pass transfers
   almost no feed body.

   The [phase plan](docs/plans/adr-0050-feed-revalidation-phase-plan.md) gives
   two passes of plan decision 10. The first pass filled the cache: the
   `refresh` pass that started on 2026-09-24 at 23:52 UTC. The second pass
   measures, and it has not run. The open question, whether a `304` counts
   against the Wavlake `429` limit, stays open until the second pass.

4. The feed trust work. ADR 0051 and ADR 0053 are Accepted and deployed.
   ADR 0054 is Accepted, and tasks 001 to 003 are complete and deployed on
   2026-09-25. ADR 0055 is Proposed
   and waits. The operator runs the only crawler, on the same host as the
   primary, so a leak of `CRAWL_TOKEN` is close to a compromise of that host.
   [ADR 0056](docs/adr/0056-the-public-proof-flow-is-offline.md) and
   [ADR 0057](docs/adr/0057-a-feed-can-block-this-index.md) are Accepted.
   The [remediation plan](docs/plans/feed-trust-remediation-plan.md) gives the
   sequence.

   Deployed on 2026-09-25, in one deploy of the primary and the crawler:

   - ADR 0051 tasks 001 to 004. Only content from the stored source URL
     changes a record, and the node checks the crawl token first.
   - ADR 0053 tasks 001 to 007. A block is a signed row that each node
     applies. A submission with an older `lastBuildDate` changes nothing.
     `GET /v1/feeds/{guid}/route-history` shows each change of the payment
     recipients.
   - ADR 0052 task 001. A record moves to the self link that its source
     declares.
   - ADR 0056 task 001. The public proof flow is removed, and each write
     route needs the admin token.

   The repair ran on 2026-09-25 from the fetch cache of the `refresh` pass
   that started on 2026-09-24 at 23:52 UTC. The replay sent 10,202 source
   bodies. The stale rule rejected 1,403 records, because a mirror had
   written a newer `lastBuildDate` before ADR 0051. Their stored dates were
   cleared, and their bodies were sent again. The copy before that change is
   `pre-stale-reset.db` in the `primary-data` volume. After the repair, 7,713
   records have `declared_self_url`, and 1,516 records can move on their next
   crawl.

   The work that remains:

   - The ADR 0057 code. It changes the parser, the ingest contract and the
     node.
   - [ADR 0058](docs/adr/0058-a-copy-of-a-feed-is-public.md), Accepted on
     2026-09-25. The API shows each copy of a feed at a URL that is not its
     source. The operator keeps the source or relocates the record. Tasks
     001 to 004 are complete and deployed on 2026-09-25. The two copies of
     Strange Love albums are resolved with `keep_source`. A relocation
     through `PATCH /v1/feeds/{guid}` now needs a `reason`. Step 7 of task 005
     stays open: after the next `refresh` pass, the count of
     `feed_copy_observed` events must grow only by the summaries that
     changed. The [phase plan](docs/plans/adr-0058-feed-copies-phase-plan.md)
     gives the sequence.
   - Wavlake does not send podpings for its feeds, and no service sends them
     for Wavlake by automation. Any person can send one by hand. Thus a
     Wavlake record moves to its self link mostly through a crawl of the music
     URL form, or through the cache replay with
     `export-feed-cache-ndjson.py --self-links`.
   - [ADR 0052](docs/adr/0052-a-source-moves-its-own-feed.md), Accepted on
     2026-09-25. Tasks 003 to 007 are complete and deployed on 2026-09-25,
     the node and then the crawler. A record moves on its self link, on
     `itunes:new-feed-url`, or on a chain of `301` and `308` redirects from its
     source URL. The crawler sends each redirect hop. A GUID change at the
     source URL is pending and public. It applies when the new GUID is the
     UUIDv5 of the source URL, or after the operator approves it. After the
     deploy, the four Doerfelverse releases of Elijah Lied are pending in
     `GET /v1/guid-changes`. The publisher confirmed that their new GUIDs are
     a tool error. The
     [phase plan](docs/plans/adr-0052-moves-and-guid-changes-phase-plan.md)
     gives the sequence.
   - ADR 0054 task 004, step 3. After a day of podpings, read the hosts that
     the gossip log rejects with `fetch_target_not_public` or
     `body_too_large`. A music feed host in that list is a defect. The
     [phase plan](docs/plans/adr-0054-fetch-rule-phase-plan.md) gives the
     sequence.
   - A migration that drops the two proof tables and changes the trigger
     `trg_feeds_cleanup_before_delete`, after the ADR 0056 deploy is stable.

[ADR 0045](docs/adr/0045-governance-model-and-contract-ownership.md) is
Proposed. The shared `project-baseline` skill and the two crate `AGENTS.md`
files follow it.

The recorded status of ADR 0032, ADR 0033 and ADR 0034 disagrees with the code.
[The index](docs/adr/README.md) lists each one under "Status Needs A Check".

---

## Repository Layout

This repository hosts three independent Cargo crates, not a Cargo
workspace:

| Path                        | Crate                 | Purpose                                             |
|-----------------------------|-----------------------|-----------------------------------------------------|
| `./`                        | `stophammer`          | Primary / community node, signed event log, API    |
| `./stophammer-crawler/`     | `stophammer-crawler`  | Feed crawler (gossip, podping, import, ndjson)     |
| `./stophammer-parser/`      | `stophammer-parser`   | Canonical RSS parser                                |

Each crate builds, tests, and lints independently. Changes that span
crates must be verified in each affected crate.

## Project Overview

Stophammer is a quality-gated V4V (Value-for-Value) music podcast index
built in Rust. Primary nodes ingest feeds, run verifiers, and sign
events. Community nodes are read-only replicas that receive pushed
events.

- **Edition**: Rust 2024
- **Database**: SQLite (WAL mode)
- **HTTP**: Axum 0.8 on Tokio
- **Identity**: Ed25519 signed events

---

## Build / Lint / Test Commands

Run from the crate root of the crate you are changing. For cross-crate
changes, run in each affected crate.

### Build

```bash
cargo build --release
cargo build
```

### Test

```bash
cargo test                       # All tests
cargo test <name>                # Single test by name substring
cargo test --test <file>         # One integration test file
cargo test --lib                 # Library unit tests only
cargo test --test migration_tests  # Migration tests (stophammer)
```

### Lint & Format

```bash
cargo fmt -- --check
cargo fmt
cargo clippy --all-targets -- -D warnings
```

### Required Pre-Commit Gate

Every commit must be green for the crate(s) touched:

1. `cargo build`
2. `cargo test`
3. `cargo clippy --all-targets -- -D warnings`
4. `cargo fmt -- --check`

Do not skip. Do not use `--no-verify`.

### Running Binaries

```bash
# Primary node
cargo run --release

# Community node
NODE_MODE=community cargo run --release

# OpenAPI spec generator
cargo run --bin gen_openapi
```

### Crawler Subcommands (`stophammer-crawler/`)

```bash
cd stophammer-crawler

# Real-time podping via Iroh gossip-listener SSE (recommended)
CRAWL_TOKEN=xxx \
  INGEST_URL=http://localhost:8008/ingest/feed \
  cargo run -- gossip [--since-hours 24] [--concurrency 5]

# Import from PodcastIndex snapshot
cargo run -- import [--db ./podcastindex_feeds.db]

# Replay cached NDJSON (no re-fetch)
CRAWL_TOKEN=xxx \
  INGEST_URL=http://localhost:8008/ingest/feed \
  cargo run -- ndjson [--input ./stored-feeds.ndjson] [--concurrency 5]

# Fetch an explicit URL list (`crawl` is an alias of `feed`)
cargo run -- feed <urls.txt

# Corrective pass over the feeds the node already holds (ADR 0047)
CRAWL_TOKEN=xxx \
  INGEST_URL=http://localhost:8008/ingest/feed \
  cargo run -- refresh [--concurrency 5] [--force]
```

There are five modes: `feed`, `import`, `ndjson`, `gossip` and `refresh`. There
is no `podping` mode. The `gossip` mode consumes the stream that carries podping
notifications. `stophammer-crawler/AGENTS.md` holds the rules for that crate.
The `feed`, `refresh`, `gossip` and `import` modes keep a fetch cache (ADR
0050).

`feed`, `refresh` and `gossip` follow a publisher link. ADR 0049 section 2
owns the rule. `feed` and `refresh` follow in three waves: the input feeds,
the feeds they name, and the album list of a publisher found through an
album. `gossip` follows in two levels, with its own throttle.

### Parser CLI (`stophammer-parser/`)

```bash
cd stophammer-parser
cargo run --bin stophammer-parse -- <path-or-url>
```

---

## Code Style

### Formatting

- `cargo fmt` with tool defaults (4-space indent). No `rustfmt.toml`.
- Never hand-format; always run `cargo fmt`.

### Naming

| Element       | Convention           | Example                |
|---------------|----------------------|------------------------|
| Types / Enums | PascalCase           | `struct Feed`          |
| Functions     | snake_case           | `fn ingest_feed()`     |
| Variables     | snake_case           | `feed_guid`            |
| Constants     | SCREAMING_SNAKE_CASE | `MAX_SSE_CONNECTIONS`  |
| Modules       | snake_case           | `db`, `verifiers`      |

### Imports

Group std / external / internal, each group separated by a blank line.
Use `crate::` for internal paths. No wildcard imports outside tests.

```rust
use std::collections::HashMap;

use axum::{Json, Router};
use serde::{Deserialize, Serialize};

use crate::{db, model, signing};
```

### Error Handling

- Define per-module error enums with `Display`, `Debug`, and
  `std::error::Error`.
- Provide `From` impls for ergonomic `?` conversion.
- Do not swallow errors. `map_err_ignore` is a lint; handle or surface.

### Async / Tokio

- `#[tokio::main]` for binaries, `#[tokio::test]` for async tests.
- Wrap blocking SQLite calls in `tokio::task::spawn_blocking`.
- Use `Arc<Mutex<Connection>>` for shared-sync DB access where a pool
  is overkill; use `db_pool::DbPool` for request-path reads.
- Prefer `tokio::sync` channels over `std::sync::mpsc` for async code.

### Logging

- Use `tracing` macros — never `println!`/`eprintln!` in library code.
- Emit structured fields: `tracing::warn!(feed_guid, "feed blocked")`.
- Choose levels deliberately: `error!` for failures that need action,
  `warn!` for recoverable oddities, `info!` for lifecycle events,
  `debug!` for diagnostics.

### Documentation

- Module-level `//!` docs on every module.
- For intentional lints, use `#[expect(clippy::..., reason = "...")]`.
- Annotate security-sensitive code with a security note (see below).

### Comment Conventions

- Issue tracking: `// Issue-<NAME> — YYYY-MM-DD`
- Security (critical findings): `// CRIT-0X ... — YYYY-MM-DD`
- Feature-gate rollouts: `// FG-0X ... — YYYY-MM-DD`
- Audit findings: `// Finding-N ... — YYYY-MM-DD`

Dates are ISO-8601. When removing such a comment, remove it cleanly —
do not leave `// removed ...` breadcrumbs.

### Serde

- `#[serde(rename_all = "snake_case")]` for enums crossing the wire.
- `#[serde(default, skip_serializing_if = "Option::is_none")]` for
  optional fields.
- `#[serde(tag = "type", content = "data")]` for tagged unions.

### Tests

- Integration tests go in `tests/` and share helpers via
  `tests/common/mod.rs`.
- Use `common::test_db()`, `common::test_db_arc()`, or
  `common::test_db_pool()` depending on access pattern.
- Use `common::now()` for deterministic timestamps.
- Use `common::temp_signer(label)` for ephemeral Ed25519 keys.
- Write assertions with descriptive messages:
  `assert_eq!(a, b, "expected X to equal Y")`.
- Tests gated on internal APIs require the `test-util` feature.

---

## Lint Configuration

This section describes the `stophammer` crate. `stophammer-crawler` and
`stophammer-parser` declare `[lints.clippy] pedantic = "deny"` and nothing
more. Each crate's own `AGENTS.md` states its lint set.

Lints are declared in this crate's `Cargo.toml` under `[lints]`. Do not
override them with broad `#[allow(...)]`. When an exception is needed,
use `#[expect(..., reason = "...")]` at the narrowest scope and justify
the `reason`.

### Rust lints (warn)

- `unsafe_op_in_unsafe_fn`, `missing_debug_implementations`,
  `ambiguous_negative_literals`, `redundant_imports`,
  `redundant_lifetimes`, `trivial_numeric_casts`, `unused_lifetimes`

### Clippy categories (all warn)

- `cargo`, `complexity`, `correctness`, `pedantic`, `perf`, `style`,
  `suspicious`

### Selected restriction lints (warn)

- `allow_attributes_without_reason`, `clone_on_ref_ptr`,
  `deref_by_slicing`, `empty_drop`, `empty_enum_variants_with_brackets`,
  `empty_structs_with_brackets`, `fn_to_numeric_cast_any`,
  `if_then_some_else_none`, `map_err_ignore`,
  `redundant_type_annotations`, `renamed_function_params`,
  `undocumented_unsafe_blocks`, `unnecessary_safety_comment`,
  `unnecessary_safety_doc`, `unneeded_field_pattern`,
  `unused_result_ok`

### Project-wide allows

- `literal_string_with_formatting_args` — needed for structured logging.
- `multiple_crate_versions` — unavoidable across transitive deps.

---

## Module Structure (`stophammer` crate)

Authoritative list comes from `src/lib.rs`. Keep both in sync.

| Module         | Purpose                                              |
|----------------|------------------------------------------------------|
| `api`          | Axum router, handlers, shared `AppState`             |
| `apply`        | Idempotent application of signed events to the DB    |
| `blocks`       | Durable feed blocks: environment seed (ADR 0053)     |
| `community`    | Community-node sync, push-receive, tracker register  |
| `db`           | SQLite schema init, core queries, pragmas            |
| `db_pool`      | WAL connection pool (1 writer, N readers)            |
| `event`        | Signed event envelope and serialisation              |
| `fetch_guard`  | SSRF guard: URL validation, DNS-pinned fetches       |
| `ingest`       | Crawler submission DTOs                              |
| `medium`       | Podcast `<podcast:medium>` handling                  |
| `model`        | Core domain types (`Feed`, `Track`, `Artist`, …)     |
| `openapi`      | OpenAPI schema (utoipa) for the HTTP surface         |
| `quality`      | Feed quality scoring heuristics                      |
| `query`        | Read-only query routes                               |
| `search`       | FTS5 full-text search                                |
| `signing`      | Ed25519 node-identity key management                 |
| `sync`         | Event-log pagination and push/pull protocol          |
| `tls`          | ACME / Let's Encrypt automatic TLS                   |
| `verify`       | Feed-ingest verification pipeline (`VerifierChain`)  |
| `verifiers/`   | Built-in verifier implementations (plugin pattern)   |

Binaries live in `src/bin/`. Current binaries:

- `gen_openapi` — prints the OpenAPI document to standard output.

---

## Adding New Code

### New Verifier

1. Create `src/verifiers/<name>.rs` implementing the `Verifier` trait.
2. Register the module in `src/verifiers/mod.rs`.
3. Add a match arm to `build_chain()` in `src/verify.rs`.
4. Expose via the `VERIFIER_CHAIN` env var (document in
   `docs/verifier-guide.md`).
5. Add integration coverage in `tests/`.

### New Database Migration

1. Create `migrations/000N_<description>.sql` using the next sequence
   number. File names are stable once committed.
2. Migrations are additive and idempotent. Never rewrite a merged
   migration — add a new one.
3. Keep `src/schema.sql` consistent with the union of all migrations
   (used for fresh bootstraps).
4. A migration version is its position in the `MIGRATIONS` array, not its
   file name. An existing database may already record that version, in which
   case the runner skips the migration and reports it with a `tracing::error!`
   at start. When that happens, add an `ensure_<name>_schema` repair in
   `src/db.rs` and call it from `open_db`. ADR 0046 owns this step.
5. Verify with `cargo test --test migration_tests`.

### New API Endpoint

1. Add the handler to `src/api.rs` (or `src/query.rs` for read-only).
2. Register the route in `build_router()`.
3. Add the path to `spec_value()` in `src/openapi.rs`. That function holds
   the whole OpenAPI document as a literal. A handler annotation does not
   reach the document.
4. Check the document with `cargo run --bin gen_openapi`, which prints it to
   standard output. `api.html` is a hand-written explorer page that reads
   `/openapi.json` at run time, so it needs no regeneration.
5. Add an integration test in `tests/`.
6. Document in `docs/API.md`.

### New Crate-Level Dependency

- Justify load-bearing deps in the commit message or an ADR.
- Prefer a narrow feature set over `features = ["full"]`.
- Check `cargo deny check` does not regress.

---

## Feature Flags

| Flag        | Crate          | Purpose                                  |
|-------------|----------------|------------------------------------------|
| `test-util` | `stophammer`   | Exposes test helpers to integration tests|

Production builds must not enable `test-util`.

---

## Foundational Mandates

These rules apply to every change, regardless of scope.

### 1. Single Source of Truth

- Treat `docs/adr/` as the decision record. If a change contradicts an
  ADR, write a new ADR first (or supersede the old one); do not smuggle
  reversals into unrelated commits.
- When a `docs/vision/` plan file is in flight, follow its phasing.
- No "black box" inference: behaviours not described in code, ADR, or
  plan must not appear silently in the implementation.

### 2. Governance & CI Compliance

- **ADR-first for architecture.** New subsystems, protocol changes,
  storage-shape changes, or cross-crate boundaries require a short ADR
  in `docs/adr/000N-<slug>.md` before the implementing PR.
- **Zero warnings.** `cargo clippy --all-targets -- -D warnings` and
  `cargo fmt -- --check` must be green on the branch tip.
- **Surgical context.** Do not read whole files of 1k+ lines when a
  targeted `grep` + focused read suffices. This keeps reviews and agent
  runs cheap.

### 3. Provenance First

When working on metadata, preserve the **source** layer. Never discard
raw source data (e.g., `itunes:author` vs `podcast:person`) based on
heuristic guesses. Surface conflicts to the operator; do not resolve
them silently.

### 4. Security Posture

- Annotate security-critical code with `// CRIT-0X ... — YYYY-MM-DD`.
- Use constant-time comparisons (`subtle::ConstantTimeEq`) for
  authentication tokens, signatures, and any secret-derived value.
- Validate external input at ingest boundaries (verifiers, handlers).
  Internal callers may assume validated types.
- Sign every event before it leaves the primary node. Community nodes
  must verify signatures before applying.

### 5. Commit Hygiene

- One logical change per commit. Squash WIP before merging.
- Commit messages describe the *why*. A bug fix names the bug; a
  feature names the user-visible outcome.
- Do not commit generated artefacts unless they are load-bearing and
  reviewed. `api.html` is hand-written, not generated. Commit a change to it
  in the same commit that changes the API surface it shows.
- Do not commit secrets (`signing.key`, `*.env`, local DB files).

---

## What Binds You

[`docs/adr/README.md`](docs/adr/README.md) gives each current ADR with its
scope and status. It is the answer to what binds a change today. ADR 0045 owns
that index.

This file owns how an agent works. An ADR owns how the code is shaped. The two
do not overlap, and neither one holds the other's rules.

- **Agent behavior lives here.** What to run, what not to run, what a report
  contains, and how to leave a gate open. These need no ADR, and this file is
  their owner.
- **Code shape lives in an ADR.** Layering, ownership, a contract between two
  modules, storage shape, and the HTTP contract. This file may point at such a
  rule and name the ADR that decides it. It does not state one as its own.

Stophammer is three repositories. `stophammer`, `stophammer-crawler` and
`stophammer-parser` each have their own GitHub upstream under `InTheMorning`.
Only `stophammer` holds decision records. A change that spans two repositories
needs one commit in each, and the commit in a crate repository names the owning
ADR.

---

## Directory Reference

- `docs/adr/` — architecture decision records (numbered).
  `docs/adr/README.md` is the index of current decisions.
- `docs/plans/` — in-flight plans, and requests received from a client
  repository.
- `docs/reviews/` — verification records and design reviews.
- `docs/vision/` — in-flight multi-phase plans.
- `docs/security/` — security policies and audit trails.
- `docs/API.md`, `docs/operations.md`, `docs/user-guide.md`,
  `docs/verifier-guide.md`, `docs/schema-reference.md` — canonical
  reference docs. Update alongside the code they describe.
- `migrations/` — monotonic SQL migrations.
- `scripts/` — operator-facing scripts.
- `packaging/` — distribution artefacts.
- `stophammer-crawler/AGENTS.md`, `stophammer-parser/AGENTS.md` — the rules for
  those repositories. Read the one for the crate you change.

# Stophammer Agent Guidelines

Guidelines for any agent (human or AI) making changes in this repository
and its sibling crates. Treat as authoritative. When reality drifts,
update this file in the same change that drifts it.

## Where The Work Stands

Current priority - 2026-09-22: the MusicIndex API metadata contract.

v4vmm reported four defects in the read API. Stophammer verified each one and
found each one correct. The client document is
[the change request](docs/plans/v4vmm-musicindex-api-change-request.md). The
measurements are in
[the verification record](docs/reviews/v4vmm-musicindex-api-change-request-verification.md).

ADR 0042, ADR 0043 and ADR 0044 are Accepted on 2026-09-22. The work runs in
this sequence:

1. [ADR 0043](docs/adr/0043-feed-publication-date-records-its-source-element.md)
   in `stophammer-parser`, then in this crate. It stops `lastBuildDate` from
   supplying a release date. Today 94 percent of feeds hold a feed build time
   in `release_date`. It needs no migration and no protocol change.
2. The `FORCE_REINGEST` trickle over the affected feeds. It drains while the
   rest of the work continues.
3. [ADR 0042](docs/adr/0042-query-responses-name-the-field-owner.md). Artwork
   ownership first, then the search summary fields.
4. [ADR 0044](docs/adr/0044-api-contract-declares-its-fields.md). The response
   types declare their schemas.

[ADR 0045](docs/adr/0045-governance-model-and-contract-ownership.md) is
Proposed. The shared `project-baseline` skill and the two crate `AGENTS.md`
files follow it.

The node at `api.musicindex.org` serves the same OpenAPI document that commit
`a220f44` makes.

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
cargo test migration_tests       # Migration-specific tests (stophammer)
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
```

There are four modes: `feed`, `import`, `ndjson` and `gossip`. There is no
`podping` mode. The `gossip` mode consumes the stream that carries podping
notifications. `stophammer-crawler/AGENTS.md` holds the rules for that crate.

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
| `community`    | Community-node sync, push-receive, tracker register  |
| `db`           | SQLite schema init, core queries, pragmas            |
| `db_pool`      | WAL connection pool (1 writer, N readers)            |
| `event`        | Signed event envelope and serialisation              |
| `ingest`       | Crawler submission DTOs                              |
| `medium`       | Podcast `<podcast:medium>` handling                  |
| `model`        | Core domain types (`Feed`, `Track`, `Artist`, …)     |
| `openapi`      | OpenAPI schema (utoipa) for the HTTP surface         |
| `proof`        | Proof-of-possession challenge/token flow             |
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
4. Verify with `cargo test migration_tests`.

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

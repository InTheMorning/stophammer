# Stophammer Agent Guidelines

Guidelines for any agent (human or AI) making changes in this repository
and its sibling crates. Treat as authoritative. When reality drifts,
update this file in the same change that drifts it.

## Where The Work Stands

Release 0.16.0 is open. It holds ADR 0059 §6 and §7, and each `Cargo.toml`
gives its version. Release 0.15.0 of 2026-10-25 delivered ADR 0080, and items
1 to 7 of v4vmm request 10. Release 0.14.0 of 2026-10-24 delivered ADR 0078 and
ADR 0079.

### Deployed

Release 0.15.0 of 2026-10-25 is the latest release. The primary and the gossip
crawler run its GHCR images. `GET /node/info` gives the revision of the running
node. [ADR 0065](docs/adr/0065-a-release-promotes-its-candidate.md) gives how a
release promotes its candidate, and
[ADR 0066](docs/adr/0066-a-version-number-tells-what-to-upgrade.md) gives the
version number.

Each ADR in [the index](docs/adr/README.md) with the status Accepted is
deployed, except the items below. The primary sets
`SOURCE_GONE_HOSTS=wavlake.com` (ADR 0067).

### Complete And Not Deployed

ADR 0059 §6 and §7 answer musicindex.org requests 17 and 18. A copies row and
a live row give the image of their feed. A remote value time split gives the
summary of its feed and its track.
[The summary rows plan](docs/plans/summary-rows-phase-plan.md) gives the tasks.
Release 0.16.0 holds them.

### Open Items

- [ADR 0055](docs/adr/0055-the-primary-fetches-what-it-signs.md) stays
  Proposed until a crawler or a nominator runs outside the host of the
  primary, or a leak of `CRAWL_TOKEN` is suspected.
- ADR 0064: `CONFIRMING_RELAY_HOSTS` stays empty until the relay gives a lease.
- ADR 0070 §4a: the time of a build of the artist list after a change is not
  measured. Measure it when the gossip crawler sends changes.
- ADR 0076: the v4vmm operator checks its two visual criteria on release
  0.13.0.
- ADR 0062: no long replay of the podping archive has run on the crawler of
  release 0.3.0 or later. After the next stop of the gossip crawler, examine
  its memory during the catch-up.
- The open question of ADR 0050 stays open.
  [The measurement record](docs/reviews/adr-0050-second-pass-measurement.md)
  gives the reason.
- Podcast-namespace PR #793 is open. ADR 0069 §1a follows it.
  [The guide](docs/publisher-links-guide.md) tells a feed author how to write
  the links.
- [The remaining accepted work plan](docs/plans/remaining-accepted-work-plan.md)
  gives the other open items: the four pending GUID changes of ADR 0052, and
  two research items.
- [The research request of 2026-10-23](docs/plans/research-search-index-request.md)
  gives a list of search changes. ADR 0078 answers its four defects and the
  phrase change. No decision is recorded for the other changes.
- ADR 0080 §6: after a restart of the primary, the first search with no `q`
  can be slow, because the database pages are not in memory. On 2026-10-25,
  the first `type=track&host=wavlake.com` took 14.5 s, and the next ones took
  approximately 195 ms. Such a track search sorts each matching row with no
  index. Measure it again when a client reports a slow search.
- A restart of the gossip crawler reads many cold pages of the podping archive
  in its first seconds. This costs only disk reads, and no decision is open.

### Operating Facts

- The primary keys its rate limit by `X-Real-IP` with `TRUST_PROXY=true`, and
  uses the defaults of 50 requests a second and a burst of 100 for each client.
  `docker-compose.yml` publishes 8008 and 8089 on `127.0.0.1` only. CI runs
  `cargo deny check advisories`, and `deny.toml` gives the reason for each
  ignore.
- Each Compose service drops each Linux capability, and `primary` and `gossip`
  have a read-only root file system. A `docker compose run --user root` of
  `primary` adds `CHOWN`, `DAC_OVERRIDE` and `FOWNER` with `--cap-add`.
- `.env` on the host pins the podping listener image by digest. Each GitHub
  Action is pinned by commit SHA.
- Wavlake sends no podping for its feeds. A Wavlake record moves to its self
  link mostly through a crawl of the music URL form, or through the cache
  replay with `export-feed-cache-ndjson.py --self-links` (ADR 0052).
- A cached body replays with `export-feed-cache-ndjson.py` and the `ndjson`
  mode, with no fetch. Use `--copies` for the copy rows, and the default for
  the source URLs.

### Client Requests

The requests of v4vmm and musicindex.org are in
[the v4vmm requests](docs/plans/v4vmm-open-requests.md) and
[the musicindex.org requests](docs/plans/musicindex-open-requests.md). v4vmm
requests 1 to 9, items 1 to 7 of v4vmm request 10 (ADR 0078 and ADR 0080), and
musicindex.org requests 1 to 15 are deployed. musicindex.org request 16 is
declined, and requests 19 and 20 are closed. Sections 6 and 7 of ADR 0059 answer
musicindex.org requests 17 and 18, which are not deployed. v4vmm request 11 is a draft.
The v4vmm operator examines it before Stophammer answers.

### Governance

[ADR 0045](docs/adr/0045-governance-model-and-contract-ownership.md) is
Accepted and complete. `docs/adr/archive/` holds each superseded ADR, and
each ADR whose rules a test enforces. `tests/adr0045_governance_guards_tests.rs`
checks that the ADR files agree with
[the index](docs/adr/README.md). The shared `project-baseline` skill and the
two crate `AGENTS.md` files follow it.

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

`feed`, `refresh` and `gossip` follow a publisher link, and the albums of a
`musicL` list. The tests of the archived ADR 0049 section 2 own the publisher
rule. The tests of the archived ADR 0060 sections 5 and 6 own the list rule. `feed` and `refresh` follow in three waves: the input feeds,
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

- `allow_attributes_without_reason`, `as_pointer_underscore`,
  `clone_on_ref_ptr`, `deref_by_slicing`, `disallowed_script_idents`,
  `empty_drop`, `empty_enum_variants_with_brackets`,
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
| `community`    | Community-node sync, push-receive, primary register  |
| `db`           | SQLite schema init, core queries, pragmas            |
| `db_pool`      | WAL connection pool (1 writer, N readers)            |
| `event`        | Signed event envelope and serialisation              |
| `fetch_guard`  | SSRF guard: URL validation, DNS-pinned fetches       |
| `ingest`       | Crawler submission DTOs                              |
| `live`         | Pure ingest rules for a live item (ADR 0064)         |
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
2. Migrations are additive and idempotent. Add the line `<file> <sha256>`
   of the new file at the end of `tests/fixtures/migration_sha256.txt`.
   `tests/adr0023_migration_hash_tests.rs` fails when a committed migration
   changes, and its message gives the line to add (ADR 0023).
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
3. If the response is a JSON type:
   - Add `#[derive(ToSchema)]` to the response struct.
   - Register the type in `response_schemas()` in `src/query.rs` or the
     module that owns the response struct.
4. Add the path and response to `spec_value()` in `src/openapi.rs`. Point the
   response schema at the registered type with `$ref`.
5. Check the document with `cargo run --bin gen_openapi`. Then run
   `cargo test`. The guards of ADR 0044 fail when the path or the schema is
   missing. Add each new property or enum value to
   `tests/fixtures/v1-schema-fields.json`, so that its removal fails
   `tests/adr0044_field_removal_tests.rs`.
6. Add an integration test in `tests/`.
7. Document in `docs/API.md`.

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
- When a phase plan in `docs/plans/` is in flight, follow its sequence.
- No "black box" inference: behaviours not described in code, ADR, or
  plan must not appear silently in the implementation.

### 2. Governance & CI Compliance

- **ADR-first for architecture.** New subsystems, protocol changes,
  storage-shape changes, or cross-crate boundaries require a short ADR
  in `docs/adr/NNNN-<slug>.md` before the implementing PR.
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
  `docs/adr/archive/` holds each superseded ADR, and each ADR whose rules a
  test enforces, for research only.
- `docs/plans/` — in-flight plans, and requests received from a client
  repository. Delete a plan when its work is complete. First move each
  measurement that an ADR needs to `docs/reviews/`.
- `docs/tasks/` — task packets of work that is not deployed. Delete a packet
  when its work is deployed. Git holds the packet, and the ADR and the tests
  hold the result.
- `docs/reviews/` — verification records, measurements and design reviews.
- `docs/security/` — security policies and audit trails.
- `docs/API.md`, `docs/operations.md`, `docs/user-guide.md`,
  `docs/verifier-guide.md`, `docs/schema-reference.md` — canonical
  reference docs. Update alongside the code they describe.
- `migrations/` — monotonic SQL migrations.
- `scripts/` — operator-facing scripts.
- `packaging/` — distribution artefacts.
- `stophammer-crawler/AGENTS.md`, `stophammer-parser/AGENTS.md` — the rules for
  those repositories. Read the one for the crate you change.

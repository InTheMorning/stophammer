# Stophammer Agent Guidelines

Guidelines for each agent, human or AI, that changes this repository and its
two crate repositories. When a fact here becomes false, correct this file in
the same change.

## Where The Work Stands

Release 0.21.1 is the latest release. `GET /node/info` gives the revision of
the running node. [ADR 0065](docs/adr/0065-a-release-promotes-its-candidate.md)
gives how a release promotes its candidate, and
[ADR 0066](docs/adr/0066-a-version-number-tells-what-to-upgrade.md) gives the
version number. Each Accepted ADR in [the index](docs/adr/README.md) is in a
release, except the work in the next list.

### Work That No Release Holds

- [ADR 0105](docs/adr/0105-a-release-publishes-a-debian-package-for-each-role.md):
  the release workflow makes a `.deb` for each role. Its "Open Checks" give the
  image build and the install check of the first candidate.
- musicindex ADR 0083: `GET /node/info` and `stophammer --version` give the
  build values.
- `stophammer-parser` holds musicindex ADR 0093 R-FEED-2 to R-FEED-4. Its
  `AGENTS.md` gives the state.
- [ADR 0095](docs/adr/0095-a-release-on-two-platforms-is-two-feeds.md) changes
  no code, so no release holds it.
- [ADR 0057 task 003](docs/tasks/adr-0057-task-003-a-block-skips-the-verifier-chain.md):
  a blocking body of a known feed skips the verifier chain, and the gossip
  crawler does not skip a refused publisher feed. Its "Open Checks" give the
  check after the deploy.

### Open Checks

Each line gives the owner, the condition and the next action.

- ADR 0081 §10: on 2026-11-28, the 28-day result decides the schedule of
  `import`. [Task 011](docs/tasks/adr-0081-task-011-import-gap-decision.md)
  gives the steps.
- ADR 0098 §2: count the feeds with a recipient of `type="lightning"` in the
  first backup after 2026-11-24. The ADR status gives the first count.
- ADR 0105: the first candidate runs the new jobs and the two open checks.
- [ADR 0103](docs/adr/0103-a-marked-publisher-feed-is-a-person.md) is
  Proposed. No code is written.
- [ADR 0106](docs/adr/0106-a-feed-changes-its-guid-in-place.md): no release
  holds it. After the deploy, the next submission of each source URL with a
  new GUID changes its record in place.
- [ADR 0055](docs/adr/0055-the-primary-fetches-what-it-signs.md) stays
  Proposed until a crawler or a nominator runs outside the host of the
  primary, or a leak of `CRAWL_TOKEN` is suspected.
- ADR 0064: `CONFIRMING_RELAY_HOSTS` stays empty until the relay gives a lease.
- ADR 0062: after the next stop of the gossip crawler, examine its memory
  during the catch-up of the podping archive.
- ADR 0099 item 36: when the gossip crawler sends changes, measure the time of
  a build of the artist list after a change.
- ADR 0100 item 22: when a client reports a slow search, measure the first
  search after a restart of the primary again.
- ADR 0076: the HitIt operator checks its two visual criteria.
- ADR 0069 §1a follows podcast-namespace PR #793, which is open.
  [The guide](docs/publisher-links-guide.md) tells a feed author how to write
  the links.
- [The remaining accepted work plan](docs/plans/remaining-accepted-work-plan.md)
  gives the four Elijah Lied GUID changes, which wait for the release of ADR
  0106, and two research items.

### Operating Facts

- After each release deploy, check that `gossip` is up on the new image.
  [The operations guide](docs/operations.md), "Deploy a release with
  Compose", gives the check.
- A timer starts the `podcastindex` pass each 3 hours (ADR 0081 §1). A timer
  starts `refresh` on Sunday at 03:00 local time (ADR 0104 §1). The three
  crawler services use the image variable of the gossip crawler, and a release
  deploy pins that variable in `.env`.
- When `url_differences` is not the count that the ADR 0104 status gives,
  read the table (ADR 0104 §5).
- The primary sets `SOURCE_GONE_HOSTS=wavlake.com` (ADR 0067).
- The operations guide gives the rate limit, the published ports, the dropped
  Linux capabilities and the read-only root file systems.
- `.env` on the host pins the podping listener image by digest. CI pins each
  GitHub Action by commit SHA and runs `cargo deny check advisories`.
  `deny.toml` gives the reason for each ignore.
- Wavlake sends no podping. The `podcastindex` pass finds a new Wavlake feed
  of the PodcastIndex music list within 3 hours (ADR 0081). A Wavlake record
  moves to its self link mostly through a crawl of the music URL form, or
  through the cache replay with `--self-links` (ADR 0052).
- `stophammer-crawler/scripts/export-feed-cache-ndjson.py` and the `ndjson`
  mode replay a cached body with no fetch. `--copies` gives the copy rows.
  `--value-blocks` gives the refill pass of ADR 0098 §6. The default gives the
  source URLs.
- A restart of the gossip crawler reads many cold pages of the podping archive
  in its first seconds. This costs only disk reads.

### Client Requests

[The HitIt requests](docs/plans/hitit-open-requests.md) and
[the musicindex.org requests](docs/plans/musicindex-open-requests.md) give the
status of each request. In earlier documents of this repository, the name
v4vmm means HitIt (HitIt ADR 0096).

## Repository Layout

This repository holds three independent Cargo crates. It is not a Cargo
workspace.

| Path                        | Crate                 | Purpose                                             |
|-----------------------------|-----------------------|-----------------------------------------------------|
| `./`                        | `stophammer`          | Primary / community node, signed event log, API    |
| `./stophammer-crawler/`     | `stophammer-crawler`  | Feed crawler (gossip, podping, import, ndjson)     |
| `./stophammer-parser/`      | `stophammer-parser`   | Canonical RSS parser                                |

Each crate builds, tests and lints independently. Each is its own Git
repository with its own GitHub upstream under `InTheMorning`.
`stophammer-crawler/AGENTS.md` and `stophammer-parser/AGENTS.md` give the
rules of those repositories. Read the one for the crate that you change.

Operator choice 2026-11-23: the stophammer session owns the three
repositories. It commits in each repository separately, with the gate of
the `AGENTS.md` of that repository.

## Project Overview

Stophammer is a quality-gated V4V (Value-for-Value) music podcast index
built in Rust. Primary nodes ingest feeds, run verifiers and sign events.
Community nodes are read-only replicas that receive pushed events.

- **Edition**: Rust 2024
- **Database**: SQLite (WAL mode)
- **HTTP**: Axum 0.8 on Tokio
- **Identity**: Ed25519 signed events

## Commands

The `project-baseline` skill gives the gate. CI runs the same steps
(`.github/workflows/ci.yml`). Run the gate in each crate that you change.

```bash
cargo test --test migration_tests     # the migration tests
cargo run --release                   # a primary node
NODE_MODE=community cargo run --release
cargo run --bin gen_openapi           # prints the OpenAPI document
```

The crawler has six modes: `feed` (alias `crawl`), `import`, `ndjson`,
`gossip`, `refresh` and `podcastindex`. The `gossip` mode reads the stream
that carries podping notifications. `stophammer-crawler/AGENTS.md` gives the
environment of each mode. The `feed`, `refresh`, `gossip`, `import` and
`podcastindex` modes keep a fetch cache (ADR 0050).

`feed`, `refresh` and `gossip` follow a publisher link and the albums of a
`musicL` list. The `tests/adr0049_*` files and `stophammer-crawler/src/follow.rs`
guard the publisher rule of archived ADR 0049 §2. `tests/adr0060_list_items_tests.rs`
guards the list rule of archived ADR 0060 §5 and §6.

[ADR 0101](docs/adr/0101-a-publisher-feed-can-list-playlists.md) §2 and §3
change the two rules. `feed` and `refresh` follow in three waves. The waves
are the input feeds, the feeds that they name, and the album list of a
publisher that an album names. `gossip` follows in two levels, with its own throttle.

`cd stophammer-parser && cargo run --bin stophammer-parse < feed.xml` parses
one feed from standard input.

## Conventions

This section is advisory. No ADR owns these conventions. The `rust-dev` skill
owns naming, imports, error types and documentation. `[lints]` in
`Cargo.toml` gives the lint set of this crate.

- Wrap a blocking SQLite call in `tokio::task::spawn_blocking`. Use
  `db_pool::DbPool` for a read on the request path.
- Use `tracing` macros with structured fields, for example
  `tracing::warn!(feed_guid, "feed blocked")`. Do not use `println!` in
  library code.
- A tagged comment gives an ISO-8601 date: `// CRIT-0X ...` for a critical
  security note, `// FG-0X ...` for a feature gate, `// Finding-N ...` for an
  audit finding and `// Issue-<NAME>` for an issue.
- An enum on the wire uses `#[serde(rename_all = "snake_case")]`. A tagged
  union uses `#[serde(tag = "type", content = "data")]`.
- Compare a token, a signature or a value from a secret with
  `subtle::ConstantTimeEq`. `src/api.rs` and `src/verifiers/crawl_token.rs`
  do this.
- Validate external input at the ingest boundary: the verifiers and the
  handlers.
- Integration tests use the helpers of `tests/common/mod.rs`. A test of an
  internal API needs the `test-util` feature. A production build does not
  enable `test-util`.
- `api.html` is a hand-written page. Commit a change to it with the change of
  the API surface that it shows.
- Do not commit `signing.key`, a `*.env` file or a local database file.

## Module Structure (`stophammer` crate)

`src/lib.rs` gives the list of modules.

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

`src/bin/gen_openapi.rs` prints the OpenAPI document to standard output.

## Adding New Code

### New Verifier

1. Create `src/verifiers/<name>.rs` that implements the `Verifier` trait.
2. Register the module in `src/verifiers/mod.rs`.
3. Add a match arm to `build_chain()` in `src/verify.rs`.
4. Add the name to the `VERIFIER_CHAIN` text of `docs/verifier-guide.md`.
5. Add an integration test in `tests/`.

### New Database Migration

1. Create `migrations/000N_<description>.sql` with the next number.
2. Add the line `<file> <sha256>` to `tests/fixtures/migration_sha256.txt`.
   `tests/adr0023_migration_hash_tests.rs` guards each committed migration.
3. Change `src/schema.sql`. `tests/adr0046_schema_sql_tests.rs` guards it.
4. A migration version is its position in the `MIGRATIONS` array (ADR 0046).
   `tests/adr0046_runner_report_tests.rs` guards the report of a skipped
   migration. ADR 0046 gives the repair.

### New API Endpoint

1. Add the handler to `src/api.rs`, or to `src/query.rs` for a read.
2. Register the route in `build_router()`.
3. Add `#[derive(ToSchema)]` to a JSON response type, and register the type in
   `response_schemas()`.
4. Add the path to `spec_value()` in `src/openapi.rs`, with a `$ref` to the
   type. Add each new property or enum value to
   `tests/fixtures/v1-schema-fields.json`.
5. Add an integration test in `tests/`, and document the route in
   `docs/API.md`.

These tests guard the steps: `tests/adr0044_schema_refs_tests.rs`,
`tests/adr0044_contract_guard_tests.rs` and
`tests/adr0044_field_removal_tests.rs`.

## Feature Flags

| Flag        | Crate          | Purpose                                  |
|-------------|----------------|------------------------------------------|
| `test-util` | `stophammer`   | Exposes test helpers to integration tests|

## What Binds You

[`docs/adr/README.md`](docs/adr/README.md) gives each current ADR with its
scope and status. It is the answer to what binds a change today. ADR 0045 owns
that index.

This file owns how an agent works in this repository. An ADR owns how the code
is shaped. This file names the ADR of a code rule and does not state the rule.

These ADRs own rules that touch each change:

- [ADR 0025](docs/adr/0025-source-claims-and-canonical-music-layers.md), the
  source claims: keep each raw source value. Do not discard one because a
  heuristic prefers another.
- [ADR 0004](docs/adr/0004-signed-event-log.md): the primary signs each event,
  and a community node verifies the signature before it applies the event.
  [ADR 0036](docs/adr/0036-sign-event-sequence-numbers.md) owns the signed
  sequence numbers.

`tests/adr0045_governance_guards_tests.rs` checks that the ADR files agree
with the index. It also runs the ADR link guard with `--bar-from 0101`, so
each ADR from 0101 has a `- Bar:` line. The `project-baseline` skill owns the
bar rule.

## Directory Reference

- `docs/adr/`: the decision records. `docs/adr/README.md` is the index of
  current decisions. `docs/adr/archive/` holds each superseded ADR, and each
  ADR whose rules a test enforces, for research only.
- `docs/plans/`: plans in progress, and the requests of a client repository.
  Delete a plan when its work is complete. First move each measurement that an
  ADR needs to `docs/reviews/`.
- `docs/tasks/`: the task packets of work that is not deployed. Delete a packet
  when its work is deployed.
- `docs/reviews/`: verification records, measurements and design reviews.
- `docs/security/`: security policies and audit records.
- `docs/API.md`, `docs/operations.md`, `docs/user-guide.md`,
  `docs/verifier-guide.md` and `docs/schema-reference.md`: the reference
  documents. Change each one with the code that it describes.
- `migrations/`: SQL migrations, in sequence.
- `scripts/`: operator scripts.
- `packaging/`: distribution files.

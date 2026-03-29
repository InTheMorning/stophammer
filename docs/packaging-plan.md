# Packaging Plan

## Purpose

This document is a decision aid, not a frozen implementation spec.

It captures:

- packaging decisions already made
- packaging decisions that still need explicit operator choice
- the recommended first release shape

The goal is to make packaging decisions easy to review one at a time instead of
mixing settled structure with speculative details.

This plan extends [ADR 0010](adr/0010-distribution-and-deployment.md) rather
than replacing it.

## Already Decided

These decisions are already the current direction and should be treated as
settled unless there is a strong reason to undo them.

### 1. Package by operator role

The product should be packaged as:

- `stophammer-indexer`
- `stophammer-node`
- `stophammer-crawler`

Reason:

- this matches how the software is actually used
- it keeps the community-node install small
- it avoids the fuzzy `admin` vs `analysis` split

### 2. Resolver binaries must use stophammer-prefixed names

Installed binary names:

- `stophammer-resolverd`
- `stophammer-resolverctl`

Reason:

- avoids collisions with generic system "resolver" tooling
- makes logs, shell history, service names, and package contents clearer

Status: already landed. `Cargo.toml` defines `stophammer-resolverd` and
`stophammer-resolverctl` as `[[bin]]` names. Man pages are
`man/stophammer-resolverd.1` and `man/stophammer-resolverctl.1`. The
`RESOLVER_WORKER_ID` default is `stophammer-resolverd-<pid>`.

Backfill and review binaries (`backfill_canonical`, `backfill_artist_identity`,
`backfill_wallets`, `review_artist_identity`, etc.) intentionally keep their
unprefixed underscore-separated names. They are operator-invoked maintenance tools,
not long-running services, and do not risk namespace collisions in `/usr/bin`.

### 3. Community node package must stay minimal

`stophammer-node` should include:

- `stophammer`
- community-node env example
- `stophammer-community.service`

It should not include:

- `stophammer-resolverd`
- `stophammer-resolverctl`
- backfill binaries
- review binaries

Reason:

- the node-runner role is replication and read API serving, not indexing

### 4. systemd is first-class

First-party packaging should include systemd units for the main long-running
roles:

- `stophammer-primary.service`
- `stophammer-community.service`
- `stophammer-resolverd.service`
- `stophammer-gossip.service`

Reason:

- this is the most realistic production deployment target right now

### 5. `stophammer-indexer` and `stophammer-node` are mutually exclusive

Both packages install `/usr/bin/stophammer`. They declare mutual conflicts:

- `stophammer-indexer` → `conflicts=('stophammer-node')`
- `stophammer-node` → `conflicts=('stophammer-indexer')`

`stophammer-indexer` carries `optdepends=('stophammer-crawler: crawl RSS feeds for
ingestion')`. Crawlers are useful for primary operators now but may not be needed by
everyone in the future.

Operators who need both an indexer and a node on the same machine use containers.

Reason:

- simplest packaging rule
- matches the operator-role split; these are distinct deployment targets
- avoids inventing wrapper or replace= complexity

### 6. Default service files ship with each package

Each package ships complete, usable service files so operators can spin up a role
without writing unit files from scratch.

Service types:

- Always-on daemons (`Type=simple`, `Restart=on-failure`):
  - `stophammer-primary.service`
  - `stophammer-community.service`
  - `stophammer-resolverd.service`
  - `stophammer-gossip.service`
- Periodic / one-shot (`Type=oneshot` + example timer units):
  - `stophammer-import.service` + `stophammer-import.timer`
  - `stophammer-crawl.service` + `stophammer-crawl.timer`

The one-shot units ship as examples, not installed defaults. Operators enable the
timer or invoke them manually depending on their schedule.

Reason:

- each package should feel complete and runnable without operator boilerplate
- mixing daemon and oneshot units in the same "always enable" story would confuse
  new operators about what needs to run continuously

### 7. Container images are the primary deploy surface; Arch is the first distro target after that

Static musl builds remain the binary primitive. The release strategy proceeds in order:

1. Multi-arch static musl builds (already the direction)
2. Container images (Docker/OCI) with opinionated defaults
3. Persistent data layout, port/interface contracts, and upgrade semantics documented
4. Generated manifests and scripts (compose files, systemd units) derived from that base
5. Distro-specific packaging (Arch, deb, rpm) automated against the above — later

Arch PKGBUILD work is deferred, not dropped. It is the first distro-specific target
after the common container/manifests base is in place. The first milestone covers
steps 1–4; the next packaging milestone should be Arch.

Reason:

- establishes a portable, automatable base before committing to distro-specific layout
- container images give operators a working deploy path faster than per-distro packaging
- generated manifests are more maintainable than hand-maintained distro files

### 8. `install.sh` is deprecated

With container images and eventual packages as the primary install path, `install.sh`
has no clear future role. It should be explicitly marked as deprecated, not extended.

Reason:

- a role-selecting installer designed now would conflict with packaging decisions not
  yet finalized; better to let the actual packages become the install path

### 9. Dedicated service users; `podping` group is conditional, not packaged

Create two dedicated service users via `sysusers.d`:

- `stophammer` / `stophammer` group — for the main node (primary and community)
- `stophammer-crawler` / `stophammer-crawler` group — for the crawler

The `podping` supplemental group grants `stophammer-crawler` read access to the
`podping-alpha-gossip-listener` archive DB at runtime. It is handled at the service
level, not in `sysusers.d`:

- `stophammer-gossip.service` lists `SupplementaryGroups=podping`
- This only takes effect if the `podping` group exists at service start time
- `sysusers.d` must not attempt to add `stophammer-crawler` to `podping` — that
  group is owned by `podping-alpha-gossip-listener` and may not be present at
  package install time

Default gossip env file has `GOSSIP_ARCHIVE_DB=` commented out. When an operator
enables archive-backed mode they must also ensure:

1. `podping-alpha-gossip-listener` is installed (creates the `podping` group)
2. The `podping` group exists before starting `stophammer-gossip.service`

Post-install notes (`.install` for Arch, or equivalent) should surface this clearly.

## Recommended Package Shapes

### `stophammer-indexer`

For primary/index operators.

Recommended contents:

- `stophammer`
- `stophammer-resolverd`
- `stophammer-resolverctl`
- `backfill_canonical`
- `backfill_artist_identity`
- `backfill_wallets`
- `review_artist_identity`
- `review_artist_identity_tui`
- `review_wallet_identity`
- `review_wallet_identity_tui`
- `review_source_claims_tui`

Recommended assets:

- `stophammer-primary.service`
- `stophammer-resolverd.service`
- `/etc/stophammer/primary.env`
- `/etc/stophammer/stophammer-resolverd.env`

Package relationships:

- `conflicts=('stophammer-node')`
- `optdepends=('stophammer-crawler: crawl RSS feeds for ingestion')`

### `stophammer-node`

For community-node runners only.

Recommended contents:

- `stophammer`

Recommended assets:

- `stophammer-community.service`
- `/etc/stophammer/community.env`

Package relationships:

- `conflicts=('stophammer-indexer')`

### `stophammer-crawler`

For crawler operators.

Recommended contents:

- `stophammer-crawler`

Recommended assets:

- `stophammer-gossip.service`
- `/etc/stophammer/crawler-gossip.env`
- optional example env files:
  - `crawler-import.env`
  - `crawler-crawl.env`

## Filesystem Layout

### Binaries

- `/usr/bin/stophammer`
- `/usr/bin/stophammer-resolverd`
- `/usr/bin/stophammer-resolverctl`
- `/usr/bin/stophammer-crawler`

### Service units

- `/usr/lib/systemd/system/stophammer-primary.service`
- `/usr/lib/systemd/system/stophammer-community.service`
- `/usr/lib/systemd/system/stophammer-resolverd.service`
- `/usr/lib/systemd/system/stophammer-gossip.service`

### Runtime state

Indexer / community node:

- `/var/lib/stophammer/stophammer.db`
- `/var/lib/stophammer/signing.key`

Crawler:

- `/var/lib/stophammer-crawler/gossip_state.db`
- `/var/lib/stophammer-crawler/import_state.db`
- `/var/lib/stophammer-crawler/feed_skip.db`
- `/var/lib/stophammer-crawler/podcastindex_feeds.db`

### Config

- `/etc/stophammer/primary.env`
- `/etc/stophammer/community.env`
- `/etc/stophammer/stophammer-resolverd.env`
- `/etc/stophammer/crawler-gossip.env`
- optional:
  - `/etc/stophammer/crawler-import.env`
  - `/etc/stophammer/crawler-crawl.env`

## systemd Unit Shape

These are the intended roles, not final unit file contents.

### Configuration convention

Every service file ships with an `EnvironmentFile=` directive pointing to a file
under `/etc/stophammer/`. That file is installed with commented-out defaults and
inline documentation for every variable. Operators edit the env file; the service
file itself never needs to change.

```
# /etc/stophammer/primary.env
#
# DB_PATH=/var/lib/stophammer/stophammer.db
# BIND=0.0.0.0:8008
# RUST_LOG=stophammer=info
#
# Required:
# CRAWL_TOKEN=<secret>
# SYNC_TOKEN=<secret>
```

This means routine configuration changes (log level, bind address, batch sizes) do not
require `systemctl edit` overrides or manual service file patching.

### Always-on daemons (`Type=simple`, `Restart=on-failure`)

#### `stophammer-primary.service`

- runs `stophammer`
- primary/indexer mode
- writes `/var/lib/stophammer`

#### `stophammer-community.service`

- runs `stophammer`
- `NODE_MODE=community`
- writes `/var/lib/stophammer`
- no resolver worker

#### `stophammer-resolverd.service`

- runs `stophammer-resolverd`
- primary-only
- shares `/var/lib/stophammer`

See Decisions Still Needed, Decision 2 for the `BindsTo` vs. `After=` question.

#### `stophammer-gossip.service`

- runs `stophammer-crawler gossip`
- shares `/var/lib/stophammer-crawler`
- includes `SupplementaryGroups=podping` in the unit file
- `GOSSIP_ARCHIVE_DB=` is commented out in the default env file (live-only mode)
- operator sets `GOSSIP_ARCHIVE_DB=/var/lib/podping-alpha-gossip-listener/archive.db`
  to enable archive-backed mode; requires the `podping` group to exist at service start

### Periodic / one-shot examples (`Type=oneshot` + timer)

These ship as example files, not installed defaults. Operators enable the timer or
invoke the service manually.

#### `stophammer-import.service` + `stophammer-import.timer`

- runs a one-shot bulk import against the indexer database
- timer period is site-specific

#### `stophammer-crawl.service` + `stophammer-crawl.timer`

- runs a one-shot crawl pass
- timer period is site-specific

## Container Image Design

### Images

Two images, matching the two Dockerfiles already in the repo:

- `stophammer` — for primary nodes, community nodes, and the resolverd worker.
  Contains all binaries from the main workspace: `stophammer`,
  `stophammer-resolverd`, `stophammer-resolverctl`, plus maintenance tools
  (`backfill_canonical`, `backfill_artist_identity`, `backfill_wallets`,
  `review_artist_identity`, `review_wallet_identity`, etc.).
  The operator selects the role by choosing the container's command.
- `stophammer-crawler` — for the crawler tier. Contains `stophammer-crawler` only.

Container contract:

- binaries live in `/usr/local/bin`
- runtime working directory is `/data`
- the `stophammer` image defaults to `CMD ["stophammer"]`
- the `stophammer-crawler` image defaults to `CMD ["stophammer-crawler", "gossip"]`
- alternate roles are selected by overriding the container `command`
- both runtime images install `ca-certificates` so HTTPS fetches and sync work out
  of the box

### Base image

Alpine-based multi-stage (already the pattern). Builder stage uses `rust:1.87-alpine`;
runtime stage uses `alpine:3.20`. Non-root user.

### Persistent data

- `stophammer` image: `/data` volume
  - `stophammer.db` — SQLite database
  - `signing.key` — Ed25519 signing key (auto-generated if missing, 0600 perms)
- `stophammer-crawler` image: `/data` volume
  - `gossip_state.db`, `import_state.db`, `feed_skip.db`

### Ports and health

- Default port: `8008` (configurable via `BIND`)
- Health check: `GET /health` → `"ok"` (200 OK)
- Health check in compose: `wget -qO- http://127.0.0.1:8008/health`

### Interface contract

- API: versioned under `/v1/` — see [API.md](API.md)
- Ingest: `POST /ingest/feed` (primary only, requires `CRAWL_TOKEN`)
- Sync: `/sync/*` endpoints (requires `SYNC_TOKEN`)
- Resolver status: `GET /v1/resolver/status`

### Reference production compose

Ship a `docker-compose.yml` alongside the existing `docker-compose.e2e.yml`. The
production compose should show:

- `primary` — runs `stophammer` (primary mode)
- `resolverd` — runs `stophammer-resolverd` from the same image
- `gossip` — runs `stophammer-crawler gossip` from the crawler image
- `community` — optional, runs `stophammer` with `NODE_MODE=community`

Each service uses `env_file:` pointing to a role-specific `.env` file. Named volumes
for `/data`. Health check dependencies so resolverd waits for primary.

## Upgrade Semantics

### Database migrations

Automatic. `stophammer` runs `run_migrations()` on every startup. There are currently
21 SQL migrations in `migrations/`. Each runs inside a transaction; if it fails, the
transaction rolls back and the binary exits. No manual migration step is needed.

### Signing key

Preserved across upgrades. `NodeSigner::load_or_create()` only generates a new key if
the file is missing. Operators must back up the signing key separately — losing it
means the node can no longer prove authorship of its existing events.

### Downgrade safety

Not guaranteed. Migrations are forward-only with no rollback support. Operators should
back up the database (`cp stophammer.db stophammer.db.bak`) before upgrading. See the
Backup and Restore section in [operations.md](operations.md) for the recommended
procedure.

### Rolling upgrade order (primary + community network)

1. Stop `stophammer-resolverd` on the primary
2. Upgrade and restart the primary (`stophammer`)
3. Upgrade and restart `stophammer-resolverd`
4. Upgrade community nodes in any order

Community nodes tolerate a primary that is one version ahead. The sync protocol uses
signed events with explicit schemas; a community node that doesn't understand a new
event type logs a warning and skips it.

## Migration From Today

### What has already changed

- `resolverd` → `stophammer-resolverd` (binary name, man page, worker ID default)
- `resolverctl` → `stophammer-resolverctl` (binary name, man page)
- Both renames are landed in `Cargo.toml` and the man pages

### What changes when the first milestone ships

- `install.sh` is deprecated; container images and release tarballs are the
  recommended install path
- Production-ready `docker-compose.yml` and systemd unit files ship with the release
- Existing databases are auto-migrated on startup (no manual step)
- Existing signing keys are preserved (no action needed)
- Operators using custom systemd units should update `ExecStart` paths from
  `resolverd` / `resolverctl` to `stophammer-resolverd` / `stophammer-resolverctl`

## Versioning

Semver. Version bumps are tagged as `v<major>.<minor>.<patch>` in git and trigger
release builds. Database compatibility is forward-only: a newer binary can read an
older database (and will auto-migrate it), but the reverse is not supported.

Versioning policy details (when to bump major, API stability guarantees) are outside
the scope of this packaging plan.

## Arch Packaging Direction (Deferred)

Arch split-package support is deferred until after container images and generated
manifests are in place. It is still the first distro-specific packaging target,
not an indefinite "later maybe".

The Arch package should be a split package:

- `stophammer-indexer`
- `stophammer-node`
- `stophammer-crawler`

Recommended repository paths:

- `packaging/arch/PKGBUILD`
- `packaging/arch/stophammer.install`
- `packaging/arch/stophammer.sysusers`
- `packaging/arch/stophammer.tmpfiles`
- `packaging/arch/stophammer-crawler.sysusers`
- `packaging/arch/stophammer-crawler.tmpfiles`
- `packaging/systemd/stophammer-primary.service`
- `packaging/systemd/stophammer-community.service`
- `packaging/systemd/stophammer-resolverd.service`
- `packaging/systemd/stophammer-gossip.service`

## Decisions Still Needed

### Decision 1: Do crawler analysis binaries get packaged at all?

Examples:

- `feed_audit`
- `audit_analyzer`
- `audit_import`
- `audit_expand_publishers`
- `musicl_backfill`

Options:

1. Keep them source-built only.
2. Add a fourth package later.
3. Fold them into `stophammer-crawler`.

Recommendation:

- keep them source-built only for now

Reason:

- they are not part of the main operational surface
- packaging them now muddies the role split

### Decision 2: Relationship between `stophammer-resolverd.service` and `stophammer-primary.service`

Settled:

- `After=stophammer-primary.service`
- `PartOf=stophammer-primary.service`

Reason:

- `After=` gives sane startup ordering
- `PartOf=` means stopping or restarting the primary also carries the resolver
- this is a better fit than `BindsTo=` for two services that share one host and DB

## Recommended First Packaging Milestone

The binary rename (prefixed resolver names, man pages, worker ID default) is already
landed. The first milestone should deliver:

- Updated `Dockerfile` including all indexer-role binaries (resolverd, resolverctl,
  backfill, review tools) in the `stophammer` image
- Reference `docker-compose.yml` for production use
- Multi-arch static musl release tarballs (continuing existing pattern)
- `packaging/systemd/` unit files with `EnvironmentFile=` convention
- Commented-default env files for each role
- `sysusers.d` / `tmpfiles.d` for `stophammer` and `stophammer-crawler` users
- Documented upgrade semantics and migration notes
- Deprecation notice on `install.sh`

It should not include:

- Arch PKGBUILD (next milestone, after the common container/manifests base)
- A fourth analysis package
- One-shot import/crawl timer units as installed defaults

## Implementation Phases

The work should proceed in small, operator-usable slices.

### Phase 0: Naming and role model

Status: done

Delivered:

- prefixed resolver binaries
- prefixed resolver man pages
- role split decided:
  - `stophammer-indexer`
  - `stophammer-node`
  - `stophammer-crawler`

### Phase 1: Common packaging base

Goal:

- establish the shared, non-distro-specific deployment base

Scope:

- update the main [Dockerfile](/home/citizen/build/stophammer/Dockerfile) so the
  `stophammer` image includes the full indexer-role binary set:
  - `stophammer`
  - `stophammer-resolverd`
  - `stophammer-resolverctl`
  - backfill/review binaries
- keep `stophammer-crawler` in its own image
- add a production-oriented `docker-compose.yml`
- add versioned unit files under `packaging/systemd/`
- add commented env examples under `packaging/env/`
- add `sysusers.d` / `tmpfiles.d` source files
- document upgrade semantics and migration notes alongside these assets
- mark `install.sh` as deprecated

Exit criteria:

- a new operator can run primary + resolver + crawler from versioned assets
  without writing service files from scratch
- the repository contains the canonical systemd/env manifests from which later
  distro packaging can be derived

### Phase 2: Local systemd-first operation

Goal:

- make the project easy for you to run directly on your own Arch machines before
  formal distro packaging exists

Scope:

- install and validate the packaged systemd units manually on your machines
- validate:
  - `stophammer-primary.service`
  - `stophammer-community.service`
  - `stophammer-resolverd.service`
  - `stophammer-gossip.service`
- validate `PartOf=` + `After=` behavior between primary and resolver
- validate the env-file convention under `/etc/stophammer/`
- validate `SupplementaryGroups=podping` behavior for archive-backed gossip
- refine hardening, state paths, and defaults based on actual operator use

Exit criteria:

- the versioned unit files and env examples are proven usable in your own setup
- no local-only overrides are required except secrets and site-specific addresses

### Phase 3: Arch split packaging

Goal:

- turn the phase-1/2 assets into first-class Arch packages

Scope:

- add `packaging/arch/PKGBUILD`
- add `.install` messaging
- add Arch install targets for:
  - `stophammer-indexer`
  - `stophammer-node`
  - `stophammer-crawler`
- encode:
  - `conflicts=('stophammer-node')` / `conflicts=('stophammer-indexer')`
  - `optdepends=('stophammer-crawler: crawl RSS feeds for ingestion')`
- install the already-versioned systemd/env/sysusers/tmpfiles assets
- verify package install, upgrade, and removal behavior on Arch

Exit criteria:

- `makepkg -si` produces working packages for all three roles
- package contents match the role split exactly
- service enablement messages are clear and correct

### Phase 4: Release automation

Goal:

- make packaging and release repeatable

Scope:

- publish multi-arch static musl tarballs for:
  - `stophammer-indexer`
  - `stophammer-node`
  - `stophammer-crawler`
- publish OCI images
- automate release assembly from the same versioned packaging assets
- optionally add Arch package build automation after the local PKGBUILD is stable

Exit criteria:

- tagged releases produce the same artifacts every time
- release assets match the documented package roles

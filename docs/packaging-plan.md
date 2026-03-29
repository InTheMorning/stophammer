# Packaging Plan

This document describes the current packaging shape for Stophammer and the
remaining rollout work. It is intentionally narrower than the earlier planning
doc: the goal now is to record the decisions that survived implementation, not
to preserve every intermediate branch.

It extends [ADR 0010](adr/0010-distribution-and-deployment.md).

## Product Roles

Packaging is by operator role:

- `stophammer-indexer`
- `stophammer-node`
- `stophammer-crawler`

This is the settled model.

Why:

- it matches how the software is actually operated
- it keeps the node-runner install small
- it avoids fuzzy package names like `admin` or `analysis`

`stophammer-parser` is not an end-user package. It is a crawler-internal library
dependency.

## Current State

These pieces are already implemented in the repo.

### Binary naming

Installed resolver binaries are prefixed:

- `stophammer-resolverd`
- `stophammer-resolverctl`

This is already landed in:

- [Cargo.toml](/home/citizen/build/stophammer/Cargo.toml)
- [man/stophammer-resolverd.1](/home/citizen/build/stophammer/man/stophammer-resolverd.1)
- [man/stophammer-resolverctl.1](/home/citizen/build/stophammer/man/stophammer-resolverctl.1)

### Versioned deployment assets

The repo already ships versioned assets under [packaging](/home/citizen/build/stophammer/packaging):

- systemd units
- env examples
- `sysusers.d`
- `tmpfiles.d`
- release manifests

These are the source of truth for later distro packaging.

### Container/runtime contract

The common deployment base now exists:

- [Dockerfile](/home/citizen/build/stophammer/Dockerfile)
- [stophammer-crawler/Dockerfile](/home/citizen/build/stophammer/stophammer-crawler/Dockerfile)
- [docker-compose.yml](/home/citizen/build/stophammer/docker-compose.yml)

Current contract:

- `stophammer` image:
  - binaries in `/usr/local/bin`
  - working directory `/data`
  - default command `stophammer`
  - alternate role by overriding command, e.g. `stophammer-resolverd`
- `stophammer-crawler` image:
  - binary in `/usr/local/bin`
  - working directory `/data`
  - default command `stophammer-crawler gossip`

Tagged releases now also publish multi-arch GHCR images for:

- `stophammer-indexer`
- `stophammer-node`
- `stophammer-crawler`

`stophammer-indexer` and `stophammer-node` are separate release targets built
from the same root Dockerfile.

### Role tarball assembly

Release tarball assembly is already implemented:

- [packaging/releases](/home/citizen/build/stophammer/packaging/releases)
- [scripts/assemble-release.sh](/home/citizen/build/stophammer/scripts/assemble-release.sh)
- [scripts/publish-release.sh](/home/citizen/build/stophammer/scripts/publish-release.sh)
- [scripts/verify-release.sh](/home/citizen/build/stophammer/scripts/verify-release.sh)
- [.github/workflows/release.yml](/home/citizen/build/stophammer/.github/workflows/release.yml)

Tagged releases now:

1. assemble role tarballs
2. generate checksums
3. verify tarball contents and basic executability
4. publish only after verification passes

### Arch packaging

Phase 2 is now implemented in:

- [packaging/arch/PKGBUILD](/home/citizen/build/stophammer/packaging/arch/PKGBUILD)
- [packaging/arch/stophammer-indexer.install](/home/citizen/build/stophammer/packaging/arch/stophammer-indexer.install)
- [packaging/arch/stophammer-node.install](/home/citizen/build/stophammer/packaging/arch/stophammer-node.install)
- [packaging/arch/stophammer-crawler.install](/home/citizen/build/stophammer/packaging/arch/stophammer-crawler.install)
- [packaging/arch/README.md](/home/citizen/build/stophammer/packaging/arch/README.md)

Current contract:

- `makepkg` produces exactly:
  - `stophammer-indexer`
  - `stophammer-node`
  - `stophammer-crawler`
- env files are installed as real config under `/etc/stophammer`
- package conflicts and optional dependencies follow the role model in this
  document
- install notes provide role-specific post-install guidance

## Package Definitions

### `stophammer-indexer`

For primary/index operators.

Contents:

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

Assets:

- `stophammer-primary.service`
- `stophammer-resolverd.service`
- `primary.env.example`
- `stophammer-resolverd.env.example`
- `stophammer.conf` `sysusers.d` and `tmpfiles.d` entries

### `stophammer-node`

For community-node runners only.

Contents:

- `stophammer`

Assets:

- `stophammer-community.service`
- `community.env.example`
- `stophammer.conf` `sysusers.d` and `tmpfiles.d` entries

### `stophammer-crawler`

For crawler operators.

Contents:

- `stophammer-crawler`

Assets:

- `stophammer-gossip.service`
- example one-shot units:
  - `stophammer-import.service`
  - `stophammer-import.timer`
  - `stophammer-crawl.service`
  - `stophammer-crawl.timer`
- env examples:
  - `crawler-gossip.env.example`
  - `crawler-import.env.example`
  - `crawler-crawl.env.example`
- `stophammer-crawler.conf` `sysusers.d` and `tmpfiles.d` entries

## Package Relationships

These are settled:

- `stophammer-indexer` conflicts with `stophammer-node`
- `stophammer-node` conflicts with `stophammer-indexer`
- `stophammer-indexer` may declare `stophammer-crawler` as an optional dependency

We are not designing for “indexer and node in one package”. If someone wants both
roles on one machine, containers are the cleaner answer.

## Installed Paths

Intended packaged paths:

### Binaries

- `/usr/bin/stophammer`
- `/usr/bin/stophammer-resolverd`
- `/usr/bin/stophammer-resolverctl`
- `/usr/bin/stophammer-crawler`

### systemd units

- `/usr/lib/systemd/system/stophammer-primary.service`
- `/usr/lib/systemd/system/stophammer-community.service`
- `/usr/lib/systemd/system/stophammer-resolverd.service`
- `/usr/lib/systemd/system/stophammer-gossip.service`

### Configuration

- `/etc/stophammer/primary.env`
- `/etc/stophammer/community.env`
- `/etc/stophammer/stophammer-resolverd.env`
- `/etc/stophammer/crawler-gossip.env`
- optional examples:
  - `/etc/stophammer/crawler-import.env`
  - `/etc/stophammer/crawler-crawl.env`

### State

Node/indexer:

- `/var/lib/stophammer/stophammer.db`
- `/var/lib/stophammer/signing.key`

Crawler:

- `/var/lib/stophammer-crawler/gossip_state.db`
- `/var/lib/stophammer-crawler/import_state.db`
- `/var/lib/stophammer-crawler/feed_skip.db`
- `/var/lib/stophammer-crawler/podcastindex_feeds.db`

## systemd Conventions

These are also settled.

- service units use `EnvironmentFile=` under `/etc/stophammer/`
- operators edit env files, not unit files
- `stophammer-resolverd.service` uses:
  - `After=stophammer-primary.service`
  - `PartOf=stophammer-primary.service`
- `stophammer-gossip.service` uses `SupplementaryGroups=podping`
  when archive-backed gossip is desired

The `podping` group is conditional runtime integration, not something we create
ourselves in `sysusers.d`.

## Release Artifacts

The release layout is now role-accurate.

Tarballs:

- `stophammer-indexer-<version>.tar.gz`
- `stophammer-node-<version>.tar.gz`
- `stophammer-crawler-<version>.tar.gz`

Each tarball contains:

- `bin/`
- `env/`
- `systemd/`
- `sysusers.d/`
- `tmpfiles.d/`
- package-specific `README.md`
- `LICENSE`

The manifests in [packaging/releases](/home/citizen/build/stophammer/packaging/releases)
are the source of truth for these bundles.

## What We Are Not Packaging

For now, these stay source-built only:

- crawler analysis binaries such as `feed_audit`, `audit_import`, and
  `musicl_backfill`

That is intentional. They are useful expert tools, but they are not part of the
main operator install surface.

## Remaining Phases

We are done with the common packaging base. The remaining work is narrower now.

### Phase 1: Common base

Status: done

Delivered:

- role split
- prefixed resolver binaries
- versioned packaging assets
- container/runtime contract
- role tarball assembly
- publish + verify release workflow

### Phase 2: Arch packaging

Status: done

Delivered:

- split-package [PKGBUILD](/home/citizen/build/stophammer/packaging/arch/PKGBUILD)
- role-specific Arch `.install` notes
- `makepkg` buildability for:
  - `stophammer-indexer`
  - `stophammer-node`
  - `stophammer-crawler`
- packaged install paths that match this document
- clear post-install guidance per role

### Phase 3: Broader release automation

Status: in progress

Delivered so far:

- tagged-release OCI image publishing to GHCR
- multi-arch image builds for:
  - `stophammer-indexer`
  - `stophammer-node`
  - `stophammer-crawler`

Remaining goal:

- extend the now-working tarball release flow into the next artifact layers

Remaining scope:

- later, Arch package build automation after the local `PKGBUILD` stabilizes

This phase is about automation polish, not package shape discovery.

## Deprecated Path

[install.sh](/home/citizen/build/stophammer/install.sh) is now legacy. It still
exists, but it is no longer the direction for packaging or release work.

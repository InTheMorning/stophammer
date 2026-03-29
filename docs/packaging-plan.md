# Packaging Plan

## Summary

Stophammer should be packaged by operator role:

- `stophammer-indexer`
- `stophammer-node`
- `stophammer-crawler`

This is simpler than splitting by "admin" versus "analysis" tooling and matches
how the product is actually used:

- indexers ingest, resolve, backfill, and review
- node runners replicate and serve read APIs
- crawlers are the separate untrusted fetch tier

This plan extends [ADR 0010](adr/0010-distribution-and-deployment.md):

- static musl binaries remain the release primitive
- systemd remains the first-class service target
- Arch Linux gets a first-party `PKGBUILD` split-package layout

## Goals

- Keep the community-node install small and obvious.
- Ship a full indexer package without making operators assemble binaries by hand.
- Give the resolver binaries stophammer-prefixed names so they do not collide
  with generic system tooling.
- Ship first-class systemd units and Arch packaging assets.

## Non-Goals

- Supporting every init system in-tree.
- Shipping one package that installs every binary by default.
- Turning analysis binaries into managed daemons.

## Installed Binary Names

The installed resolver binaries should use prefixed names:

- `stophammer-resolverd`
- `stophammer-resolverctl`

This avoids collisions with generic "resolver" tooling on Linux systems and
keeps service names and shell history unambiguous.

Internal Rust module names can remain `resolver` without affecting the operator
surface.

## Package Boundaries

### 1. `stophammer-indexer`

For primary/index operators.

Included binaries:

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

Included docs/assets:

- node/runtime docs
- maintenance manpages
- systemd units:
  - `stophammer-primary.service`
  - `stophammer-resolverd.service`
- env examples
- `sysusers.d` and `tmpfiles.d`

This is the "full primary/indexing stack" package.

### 2. `stophammer-node`

For community-node runners only.

Included binaries:

- `stophammer`

Included docs/assets:

- node/runtime docs
- systemd unit:
  - `stophammer-community.service`
- env examples
- `sysusers.d` and `tmpfiles.d`

Explicitly not included:

- `stophammer-resolverd`
- `stophammer-resolverctl`
- backfill binaries
- review binaries

This package should stay small and read-only in spirit.

### 3. `stophammer-crawler`

For crawler operators.

Included binaries:

- `stophammer-crawler`

Included docs/assets:

- crawler ops docs
- systemd unit:
  - `stophammer-gossip.service`
- optional example timer/service pairs for import or one-shot crawl
- env examples
- `sysusers.d` and `tmpfiles.d`

Optional analysis binaries can remain source-built for now. If they become
important enough for packaging later, add a fourth package then.

## Release Artifacts

Keep the current static-musl release model, but publish role-oriented tarballs
instead of exposing only raw binaries.

Recommended GitHub Release assets per architecture:

- `stophammer-indexer-linux-x86_64.tar.gz`
- `stophammer-indexer-linux-aarch64.tar.gz`
- `stophammer-node-linux-x86_64.tar.gz`
- `stophammer-node-linux-aarch64.tar.gz`
- `stophammer-crawler-linux-x86_64.tar.gz`
- `stophammer-crawler-linux-aarch64.tar.gz`

Each tarball should contain:

- `bin/`
- `share/man/man1/`
- `share/doc/stophammer*/`
- `lib/systemd/system/` when relevant
- `usr/lib/sysusers.d/`
- `usr/lib/tmpfiles.d/`
- `etc/stophammer/examples/`

Raw standalone binaries can still be published for debugging and scripting, but
the tarballs should be the operator-facing release surface.

## Filesystem Layout

### Common

- binaries: `/usr/bin`
- manpages: `/usr/share/man/man1`
- docs/examples: `/usr/share/doc/<pkgname>`
- service units: `/usr/lib/systemd/system`
- sysusers: `/usr/lib/sysusers.d`
- tmpfiles: `/usr/lib/tmpfiles.d`
- packaged env examples: `/usr/share/stophammer/examples`

### Indexer runtime state

- database: `/var/lib/stophammer/stophammer.db`
- signing key: `/var/lib/stophammer/signing.key`
- working directory: `/var/lib/stophammer`

### Community node runtime state

- database: `/var/lib/stophammer/stophammer.db`
- optional signing key path remains present but unused in normal community mode
- working directory: `/var/lib/stophammer`

### Crawler runtime state

- gossip state: `/var/lib/stophammer-crawler/gossip_state.db`
- import state: `/var/lib/stophammer-crawler/import_state.db`
- skip db: `/var/lib/stophammer-crawler/feed_skip.db`
- optional PodcastIndex snapshot: `/var/lib/stophammer-crawler/podcastindex_feeds.db`

### Configuration

- indexer:
  - `/etc/stophammer/primary.env`
  - `/etc/stophammer/stophammer-resolverd.env`
- community node:
  - `/etc/stophammer/community.env`
- crawler:
  - `/etc/stophammer/crawler-gossip.env`
  - optional:
    - `/etc/stophammer/crawler-import.env`
    - `/etc/stophammer/crawler-crawl.env`

This is more explicit than a single env file and prevents primary/community
packaging from implying the same operational role.

## System Users and Groups

Create dedicated users:

- `stophammer`
- `stophammer-crawler`

Recommended groups:

- `stophammer`
- `stophammer-crawler`
- `podping` as a supplemental group for the crawler when archive-backed gossip
  reads `gossip-listener`'s archive DB

Ship:

- `stophammer.sysusers`
- `stophammer.tmpfiles`
- `stophammer-crawler.sysusers`
- `stophammer-crawler.tmpfiles`

## systemd Services

### 1. `stophammer-primary.service`

Purpose:

- primary/indexer runtime

Core unit shape:

- `User=stophammer`
- `Group=stophammer`
- `EnvironmentFile=/etc/stophammer/primary.env`
- `ExecStart=/usr/bin/stophammer`
- `WorkingDirectory=/var/lib/stophammer`
- `Restart=on-failure`

Hardening:

- `NoNewPrivileges=true`
- `ProtectSystem=strict`
- `ProtectHome=true`
- `PrivateTmp=true`
- `PrivateDevices=true`
- `ProtectControlGroups=true`
- `ProtectKernelTunables=true`
- `ProtectKernelModules=true`
- `LockPersonality=true`
- `ReadWritePaths=/var/lib/stophammer`
- `StateDirectory=stophammer`

### 2. `stophammer-community.service`

Purpose:

- read-only community node runtime

Core unit shape:

- `User=stophammer`
- `Group=stophammer`
- `EnvironmentFile=/etc/stophammer/community.env`
- `Environment=NODE_MODE=community`
- `ExecStart=/usr/bin/stophammer`
- `WorkingDirectory=/var/lib/stophammer`
- `Restart=on-failure`

Hardening should match `stophammer-primary.service`.

### 3. `stophammer-resolverd.service`

Purpose:

- primary-only resolver worker

Core unit shape:

- `User=stophammer`
- `Group=stophammer`
- `EnvironmentFile=/etc/stophammer/stophammer-resolverd.env`
- `ExecStart=/usr/bin/stophammer-resolverd`
- `WorkingDirectory=/var/lib/stophammer`
- `Restart=on-failure`

Hardening should match `stophammer-primary.service`.

### 4. `stophammer-gossip.service`

Purpose:

- long-running archive-backed gossip ingest

Core unit shape:

- `User=stophammer-crawler`
- `Group=stophammer-crawler`
- `SupplementaryGroups=podping`
- `EnvironmentFile=/etc/stophammer/crawler-gossip.env`
- `ExecStart=/usr/bin/stophammer-crawler gossip --archive-db /var/lib/podping-alpha-gossip-listener/archive.db --skip-db /var/lib/stophammer-crawler/feed_skip.db --skip-known-non-music`
- `WorkingDirectory=/var/lib/stophammer-crawler`
- `Restart=on-failure`

Hardening:

- `NoNewPrivileges=true`
- `ProtectSystem=strict`
- `ProtectHome=true`
- `PrivateTmp=true`
- `PrivateDevices=true`
- `ProtectControlGroups=true`
- `ProtectKernelTunables=true`
- `ProtectKernelModules=true`
- `LockPersonality=true`
- `ReadWritePaths=/var/lib/stophammer-crawler`
- `ReadOnlyPaths=/var/lib/podping-alpha-gossip-listener`
- `StateDirectory=stophammer-crawler`

### Optional scheduled crawler units

Do not enable these by default, but ship examples:

- `stophammer-import.service`
- `stophammer-import.timer`
- `stophammer-crawl.service`

The only always-on crawler service should be gossip mode.

## Arch Linux Packaging

### Strategy

Use one source tree with split packages:

- `stophammer-indexer`
- `stophammer-node`
- `stophammer-crawler`

This keeps builds single-source while giving Arch users role-oriented installs.

### `PKGBUILD` structure

Recommended split-package outline:

```bash
pkgbase=stophammer
pkgname=(
  stophammer-indexer
  stophammer-node
  stophammer-crawler
)
pkgver=0.1.0
pkgrel=1
arch=('x86_64' 'aarch64')
license=('AGPL3')
url='https://github.com/v4v-tools/stophammer'
makedepends=('cargo' 'clang' 'pkgconf' 'systemd')
source=("stophammer-$pkgver.tar.gz::https://github.com/v4v-tools/stophammer/archive/refs/tags/v$pkgver.tar.gz")
sha256sums=('SKIP')

build() {
  cd "$srcdir/stophammer-$pkgver"
  cargo build --release --bins
  cargo build --manifest-path stophammer-crawler/Cargo.toml --release --bins
}
```

Then install by role:

- `package_stophammer-indexer()`
  - install:
    - `target/release/stophammer`
    - `target/release/stophammer-resolverd`
    - `target/release/stophammer-resolverctl`
    - backfill binaries
    - review binaries
  - install manpages
  - install:
    - `stophammer-primary.service`
    - `stophammer-resolverd.service`
  - install env examples
  - install `sysusers.d` and `tmpfiles.d`
- `package_stophammer-node()`
  - install:
    - `target/release/stophammer`
  - install:
    - `stophammer-community.service`
  - install env examples
  - install `sysusers.d` and `tmpfiles.d`
- `package_stophammer-crawler()`
  - install:
    - `stophammer-crawler/target/release/stophammer-crawler`
  - install:
    - `stophammer-gossip.service`
  - install crawler env examples
  - install `sysusers.d` and `tmpfiles.d`

### Arch package relationships

Recommended dependencies:

- `stophammer-indexer`
  - `depends=('systemd-libs')`
  - `provides=('stophammer-primary')`
- `stophammer-node`
  - `depends=('systemd-libs')`
  - `provides=('stophammer-community')`
- `stophammer-crawler`
  - `depends=('systemd-libs')`
  - `optdepends=('podping-alpha-gossip-listener: archive-backed gossip source')`

`stophammer-indexer` should not depend on `stophammer-node`; it already carries
the node binary and is a different operator role, not an extension package.

### Arch service enablement

Do not auto-enable services in `post_install`.

Use `.install` messages to print next steps instead:

- indexer:
  - `systemctl enable --now stophammer-primary.service`
  - `systemctl enable --now stophammer-resolverd.service`
- community node:
  - `systemctl enable --now stophammer-community.service`
- crawler:
  - `systemctl enable --now stophammer-gossip.service`

### Arch packaging files to ship

Add these repository paths:

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

## Recommended First Packaging Milestone

The first milestone should ship all three operator packages:

- `stophammer-indexer`
- `stophammer-node`
- `stophammer-crawler`

with:

- static release tarballs
- Arch split packages
- systemd units
- env examples

This is enough to make the product feel packaged without prematurely freezing
every ancillary tool.

## Migration From Today

### Current state

- operators mostly build from source or use `install.sh`
- service management is manual
- community-node and primary/index installs are not packaged separately
- resolver binaries use generic names

### Target state

- role-oriented packages are published per release
- community-node installs remain minimal
- indexers get the full primary/resolver/tool stack
- resolver binaries and services use stophammer-prefixed names
- Arch packages install only the correct role subset

### Phased rollout

1. Land prefixed resolver binary names and operator docs.
2. Add versioned systemd units and env examples to the repository.
3. Add release assembly scripts for role-based tarballs.
4. Add `packaging/arch/PKGBUILD`.
5. Update install docs so role-based packages become the default deployment path.

## Open Questions

- Whether `install.sh` should remain node-only or become a role-selecting installer.
- Whether crawler analysis binaries should ever get their own package.
- Whether example timer-based import units belong in-tree or only in docs.

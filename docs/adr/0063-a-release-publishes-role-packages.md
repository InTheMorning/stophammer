# ADR 0063: A Release Publishes Role Packages

## Status
Accepted

Supersedes [ADR 0010](0010-distribution-and-deployment.md).

## Date
2026-09-26

## Context
ADR 0010 describes a release with these parts:

- One static musl binary for each architecture, built with `cross` for
  aarch64.
- Two release assets, `stophammer-linux-x86_64` and
  `stophammer-linux-aarch64`.
- The script `install.sh`, which an operator runs with `curl … | sh`.
- One systemd unit, `stophammer.service`, with the env file
  `/etc/stophammer/env`.

On 2026-09-26 the release does not agree with ADR 0010:

1. `.github/workflows/release.yml` publishes no asset with the name
   `stophammer-linux-x86_64` or `stophammer-linux-aarch64`. `install.sh`
   downloads from the repository `stophammer/stophammer`. That repository is
   not available. Thus the script cannot install a node.
2. The workflow builds each tarball with `cargo build --release` on
   `ubuntu-latest`. The binary links to glibc, and it is for x86_64 only. No
   job uses `cross` or a musl target. Only the images build on Alpine.
3. `packaging/systemd/` holds one unit for each role. Each unit reads the env
   file of its role, for example `/etc/stophammer/primary.env`.
4. `docs/operations.md` calls `install.sh` "the legacy direct-binary path".

The first release candidate, `v0.1.0-rc.1`, ran this workflow on 2026-09-26.
On the same day the operator decided to remove `install.sh`.

## Decision

### 1. A release is one tag in each of the three repositories

A release is a `v*` tag with the same name in `stophammer-parser`,
`stophammer-crawler` and `stophammer`. The tag of `stophammer` starts the
release workflow. The workflow checks out the crawler and the parser at the
same tag. When a repository has no tag of that name, the workflow stops.

### 2. A release gives three operator roles

The roles are `stophammer-indexer`, `stophammer-node` and
`stophammer-crawler`. The file `packaging/releases/<role>.manifest` gives the
contents of the tarball of each role.

### 3. The release assets

- A tarball for each role, for Linux on x86_64, and
  `SHA256SUMS-<tag>.txt`.
- An Arch package for each role, for x86_64, and its checksum file.
- A GHCR image for each role, for `linux/amd64` and `linux/arm64`.

### 4. A tag with a hyphen is a candidate

A tag such as `v0.1.0-rc.1` gives a GitHub pre-release. It does not become
the latest release, and its images get no `latest` tag.

### 5. There is no install script

An operator installs a role from its tarball, from its Arch package, or from
its image. The repository holds no `curl … | sh` installer.

### 6. The systemd units

`packaging/systemd/` holds the units. Each unit runs as a dedicated
service user, reads the env file of its role from `/etc/stophammer/`, and
has these settings:

- `NoNewPrivileges=true`
- `ProtectSystem=strict`
- `ReadWritePaths` names only the data directory of the role.

## Consequences

- A tarball runs on an x86_64 Linux host with a glibc version that is the
  same as the runner of the workflow or newer. An aarch64 host uses the image.
- The release depends on three tags. The operator gives the tag in the
  parser, then the crawler, then `stophammer`.
- An operator with an init system that is not systemd writes a service
  wrapper. This ADR does not include that.
- `scripts/verify-release.sh` and `scripts/verify-arch-packages.sh` check the
  tarballs and the Arch packages in the workflow. No test checks section 6.
  The operator examines a changed unit before its commit.

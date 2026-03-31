## Packaging Assets

This directory holds the shared deployment assets that Phase 1 packaging work
builds on.

Layout:

- `env/`
  - `*.example`: host/systemd `EnvironmentFile=` payloads
  - `*.compose.env`: Docker Compose env files with container-internal `/data` paths
- `systemd/`
  - long-running service units for primary, community, resolver, and gossip
  - example one-shot import/crawl units and timers
- `arch/`
  - Arch Linux split-package metadata and install notes
  - built on top of the versioned assets in this directory
- `sysusers.d/`
  - service users and groups for packaged installs
- `tmpfiles.d/`
  - state-directory ownership and creation rules

Current intent:

- `docker-compose.yml` is the common container-first reference stack
- `systemd/`, `sysusers.d/`, and `tmpfiles.d/` are the first-party assets for
  later Arch packaging
- `arch/` turns those assets into real Arch packages for:
  - `stophammer-indexer`
  - `stophammer-node`
  - `stophammer-crawler`
- `releases/` defines the package-to-tarball layout for role-specific release
  bundles
- `env/*.example` files are copied to `/etc/stophammer/` and edited per host
- `env/*.compose.env` files are edited in-place for the reference Docker stack;
  they are intentionally Docker-specific and should not reuse host `/var/lib/...`
  path defaults

These files are versioned deployment inputs, not generated artifacts.

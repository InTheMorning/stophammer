## Packaging Assets

This directory holds the shared deployment assets that Phase 1 packaging work
builds on.

Layout:

- `env/`
  - `*.example`: host/systemd `EnvironmentFile=` payloads
  - `*.compose.env.example`: tracked Docker Compose templates
  - `*.compose.env`: local ignored Docker Compose env files with container-internal `/data` paths
- `systemd/`
  - long-running service units for primary, community, and gossip
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
- `env/*.compose.env.example` files are copied to `env/*.compose.env` for the
  reference Docker stack
- the resulting `env/*.compose.env` files are intentionally Docker-specific,
  ignored by Git, and should not reuse host `/var/lib/...` path defaults
- extra tracked compose templates also exist for custom `community` and
  one-shot `crawl` services, even though the root `docker-compose.yml` does not
  instantiate those roles by default

These files are versioned deployment inputs, not generated artifacts.

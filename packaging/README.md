## Packaging Assets

This directory holds the shared deployment assets that Phase 1 packaging work
builds on.

Layout:

- `env/`
  - example `EnvironmentFile=` payloads for systemd installs
  - runnable compose env files for the reference container stack
- `systemd/`
  - long-running service units for primary, community, resolver, and gossip
  - example one-shot import/crawl units and timers
- `sysusers.d/`
  - service users and groups for packaged installs
- `tmpfiles.d/`
  - state-directory ownership and creation rules

Current intent:

- `docker-compose.yml` is the common container-first reference stack
- `systemd/`, `sysusers.d/`, and `tmpfiles.d/` are the first-party assets for
  later Arch packaging
- `env/*.example` files are copied to `/etc/stophammer/` and edited per host

These files are versioned deployment inputs, not generated artifacts.

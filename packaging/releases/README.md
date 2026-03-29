## Release Assembly

This directory defines the release-tarball layout for the three operator-role
packages:

- `stophammer-indexer`
- `stophammer-node`
- `stophammer-crawler`

Each `*.manifest` file maps a built artifact or versioned packaging asset into
the tarball-relative path that should contain it.

The assembly entrypoint is:

```bash
./scripts/assemble-release.sh
```

To build the tarballs and emit a checksum file for publishing:

```bash
./scripts/publish-release.sh
```

To smoke-check the produced tarballs locally:

```bash
./scripts/verify-release.sh
```

By default it creates versioned tarballs under `dist/` using:

- the main workspace release binaries from `target/release`
- the crawler binary from `stophammer-crawler/target/release`

Expected tarball structure:

```text
stophammer-indexer-<version>/
  bin/
  env/
  systemd/
  sysusers.d/
  tmpfiles.d/
  README.md
  LICENSE
```

These manifests are the shared source of truth for later distro packaging and
release automation.

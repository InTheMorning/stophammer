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

To build the Arch split packages from the committed `PKGBUILD`:

```bash
./scripts/build-arch-packages.sh
```

To verify the built Arch packages and their checksums:

```bash
./scripts/verify-arch-packages.sh
```

Tagged releases also publish GHCR images for the same three operator roles:

- `stophammer-indexer`
- `stophammer-node`
- `stophammer-crawler`

The release workflow also builds and uploads the matching Arch packages as
release assets, after a separate verification step.

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

These manifests are the shared source of truth for tarball packaging, while the
release workflow now layers OCI image publishing on top of the same role model.

## Arch Packaging

This directory contains the first distro-specific packaging target from
[packaging/README.md](/home/citizen/build/stophammer/packaging/README.md).

Current shape:

- split packages:
  - `stophammer-indexer`
  - `stophammer-node`
  - `stophammer-crawler`
- the `PKGBUILD` reuses the existing role-tarball assembly flow by calling:
  - [scripts/publish-release.sh](/home/citizen/build/stophammer/scripts/publish-release.sh)
- install paths match the packaging asset layout:
  - binaries under `/usr/bin`
  - units under `/usr/lib/systemd/system`
  - env files under `/etc/stophammer`
  - `sysusers.d` and `tmpfiles.d` data under `/usr/lib`

Notes:

- `stophammer-indexer` and `stophammer-node` conflict intentionally.
- `stophammer-crawler` is independent and can be installed alongside the
  indexer package.
- the env files are installed as real config files, not only as examples:
  - `primary.env`
  - `community.env`
  - `crawler-gossip.env`
  - `crawler-import.env`
  - `crawler-feed.env`

Local build:

```bash
cd packaging/arch
makepkg -f
```

This `PKGBUILD` assumes the current workspace layout:

- root repo at `../../`
- crawler repo available at `../../stophammer-crawler`

That matches the release assembly scripts already used by this repo.

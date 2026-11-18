# ADR 0105: A Release Publishes A Debian Package For Each Role

## Status
Accepted on 2026-11-18 by the operator. The orchestrator session relayed the
rules of musicindex ADR 0084 and the decision of the operator. "Open Checks" gives the two
checks of the first candidate.

- Bar: crosses repositories. musicindex ADR 0084 R-PACKAGE-5 asks each
  repository for its own packaging record. The release workflow also reads
  `stophammer-crawler`.
- Cites: musicindex ADR 0084, `docs/adr/0084-the-linux-packages-of-each-suite-app.md`
  in the `musicindex` repository, commit `2518515`. That record owns the
  rules R-PACKAGE-1 to R-PACKAGE-5. This record does not write them again.
- Changes: [ADR 0063](0063-a-release-publishes-role-packages.md) §3. A release
  also has a Debian package for each role.
- Operator choice, 2026-11-18: the `Maintainer` field is
  `HeyCitizen <4637290+InTheMorning@users.noreply.github.com>`, the line of
  the other suite packages. The orchestrator session relayed the choice.

## Date
2026-11-18

## Context

musicindex ADR 0084 R-PACKAGE-2 tells stophammer to ship an Arch PKGBUILD, a
Debian `.deb` and a Docker image. The `.deb` and the PKGBUILD install a
systemd unit. On 2026-11-18 stophammer has the PKGBUILD and the images, but no
`.deb`.

The release of ADR 0063 makes three roles: `stophammer-indexer`,
`stophammer-node` and `stophammer-crawler`. These parts make a release today:

| Part | What it does |
| --- | --- |
| `packaging/releases/<role>.manifest` | Gives the files of each role tarball. |
| `scripts/publish-release.sh` | Builds each binary one time and makes the role tarballs. |
| `packaging/arch/PKGBUILD` | Reads the role tarballs and puts each file at its path on the host. |
| `scripts/verify-arch-packages.sh` | Reads each Arch package and finds the files of its role. |
| `scripts/promote-release.sh` | Copies the files of a candidate to its release with no build (ADR 0065). |
| `packaging/systemd/`, `sysusers.d/`, `tmpfiles.d/` | The units, the service users and the data folders. |

The crawler is a different repository, but it ships as its own service. It
has its own user `stophammer-crawler`, its own data folder and its own units.
The release workflow checks out the crawler at the tag of the release.
This repository makes the crawler tarball, the crawler Arch package and the
crawler image. The crawler repository has no release workflow.

Terms:

- A **`.deb`** is a Debian binary package. This is an established Debian term.
- **`dpkg-deb`** is the Debian tool that makes and reads a `.deb`. This is an
  established Debian term.
- **cargo-deb** is a Cargo command that makes a `.deb` from metadata in
  `Cargo.toml`. This is an established term.
- A **conffile** is a file in `/etc` that the package manager keeps when the
  operator changed it. This is an established Debian term.

## Decision

### 1. A release has three Debian packages

A release has one `.deb` for each role of ADR 0063 §2:
`stophammer-indexer`, `stophammer-node` and `stophammer-crawler`.

The crawler gets its own package, because it is its own service with its own
user. This repository builds the crawler package, as it builds the crawler
Arch package. The crawler repository builds no package.

### 2. `dpkg-deb` makes each package from the role tarball

`scripts/build-deb-packages.sh` reads the role tarballs of
`scripts/publish-release.sh`. It puts each file at the path that
`packaging/arch/PKGBUILD` gives it, and it makes the package with `dpkg-deb`.
No step builds the binaries again.

The workflow does not use cargo-deb, for these reasons:

1. cargo-deb reads the file list from `Cargo.toml`. The role manifests give
   the same list, so cargo-deb needs a second copy of each manifest.
2. The crawler package needs metadata in the crawler repository. The crawler
   repository then owns a part of the stophammer release.
3. cargo-deb builds the binary again. The `.deb` then has different bytes from
   the binary of the role tarball.

### 3. The contents of each package

- The paths are the paths of the PKGBUILD: `/usr/bin`, `/etc/stophammer`,
  `/usr/lib/systemd/system`, `/usr/lib/sysusers.d`, `/usr/lib/tmpfiles.d` and
  `/usr/share/doc/<package>`. `LICENSE` goes to `copyright`.
  `etc/stophammer/crawler-podcastindex.env` has mode `0600` (ADR 0081 §2).
- Each file in `/etc/stophammer` is a conffile.
- `stophammer-indexer` and `stophammer-node` install `stophammer.1`.
- `Depends` has `systemd` and the packages of the shared libraries of the
  binary. The script reads the libraries from the binary. A library with no
  known package stops the build.
- `stophammer-indexer` and `stophammer-node` name each other in `Conflicts`,
  as in the PKGBUILD.
  `Suggests` gives the `optdepends` of the PKGBUILD.
- The version is the `Cargo.toml` version of the crate, with the Debian
  revision `1`. A Cargo pre-release such as `0.1.0-rc.2` becomes
  `0.1.0~rc.2-1`. Thus a candidate and its release have the same package.
- The architecture is `amd64`, because the role tarballs are for x86_64 only.

### 4. The package scripts

- `postinst` runs `systemd-sysusers` and `systemd-tmpfiles --create` with the
  file of the role. Then it tells systemd to read the units again. It does not
  run `systemctl enable` or `systemctl start`. The operator edits the env file first, as with the
  Arch package.
- `prerm` stops the units of the role before a remove. Thus a removed binary
  does not keep the database or the port.
- `postrm` tells systemd to read the units again after a remove.
- The service user and the data folder stay after a remove and after a purge,
  because the data folder holds the database.

The three scripts are in `packaging/debian/`.

### 5. The release workflow builds and checks the packages

`.github/workflows/release.yml` builds the packages of a candidate after
`verify-tarballs`:

1. `build-deb-packages` runs `scripts/build-deb-packages.sh`.
2. `verify-deb-packages` runs `scripts/verify-deb-packages.sh`. Then it runs
   `scripts/check-deb-install.sh`. That script installs each package on the
   runner, finds the user, the data folder, the binary and the units, and
   removes the package.
3. `release-deb-packages` attaches the packages and
   `SHA256SUMS-deb-<tag>.txt` to the pre-release.

`scripts/promote-release.sh` copies the packages of the candidate to the
release with no change, and checks them again.

### 6. The deploy does not change

The VPS keeps Docker Compose. The packages are for an operator who installs
a role on a Debian or Ubuntu host.

## Consequences

- A release has three more assets and one more checksum file.
- A candidate that has no `.deb` cannot promote. The first release after this
  record needs a new candidate.
- A host must have the glibc version of the runner or a newer one. `Depends`
  states that version.
- An aarch64 host uses the image, as for the tarball and the Arch package.
- A local build needs `dpkg-deb`. An Arch host gets it from the package
  `dpkg`.

## Checks

Mechanical:

- `scripts/verify-deb-packages.sh` finds the fields and the files of each role
  (§1 and §3). The release workflow and `scripts/promote-release.sh` run it.
- `scripts/check-deb-install.sh` installs and removes each package on the
  runner (§4). The release workflow runs it.

No `cargo test` examines this record. The checks need `dpkg-deb` and a host
with systemd. The release workflow has the two.

## Open Checks

These checks stay open until the first candidate after this record runs the
release workflow:

1. The image of each role builds on GitHub. Since musicindex ADR 0083, the
   build gets the crate `musicindex-build-info` from GitHub. No host builds
   the images.
2. `scripts/check-deb-install.sh` passes on the runner.

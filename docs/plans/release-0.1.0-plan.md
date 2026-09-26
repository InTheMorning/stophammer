# Release 0.1.0 Plan

Date: 2026-09-26. Owner of the release mechanism:
[ADR 0010](../adr/0010-distribution-and-deployment.md). This plan states no
rule.

## Goal

The first public release, version 0.1.0, of the three repositories:
`stophammer`, `stophammer-crawler` and `stophammer-parser`. Each has the tag
`v0.1.0`. The GitHub release of `stophammer` holds the role tarballs, the
checksums and the Arch packages, and GHCR holds the three images.

## Preconditions

The operator decided on 2026-09-26 that 0.1.0 waits until ADR 0044 and ADR
0057 are complete. Complete means that each task is merged, deployed and
checked on production:

1. ADR 0044 tasks 002 and 003 are merged and deployed. The visual gate of
   task 002 is done: `/api` on the node and `api.html` on the site show each
   example.
2. ADR 0057 tasks 001 to 003 are merged and deployed. The operator check of
   the ADR 0057 phase plan is done: a test feed with a `yes` block leaves the
   index, and returns when the tag is removed.
3. `AGENTS.md` records ADR 0044 and ADR 0057 as complete.
4. The release workflow is corrected. See "The Release Workflow Fails Today".
5. A release candidate, `v0.1.0-rc.1`, runs the workflow to the end.

## Current State

- Each `Cargo.toml` gives `version = "0.1.0"`. The OpenAPI document gives
  `info.version` from `CARGO_PKG_VERSION`, so it gives `0.1.0` today.
- No repository has a tag. `stophammer` has no GitHub release. The release
  workflow has never run.
- The three repositories are public. `stophammer` ignores the directories
  `stophammer-crawler/` and `stophammer-parser/` in `.gitignore`.
- The CI workflow of `stophammer` passes. The two crate repositories have no
  workflow.

## The Release Workflow Fails Today

`.github/workflows/release.yml` runs on a `v*` tag of `stophammer`. It checks
out only `stophammer`. Two jobs need the crawler:

- `scripts/assemble-release.sh` builds
  `stophammer-crawler/Cargo.toml`. The directory is not in the checkout, so
  the job fails, and each later job does not run.
- The image job of `stophammer-crawler` uses the build context
  `./stophammer-crawler`. The directory is not in the checkout.

The crawler also needs the parser. `stophammer-crawler/Cargo.toml` gives
`stophammer-parser = { path = "../stophammer-parser" }`. The crawler
`Dockerfile` clones the parser at `STOPHAMMER_PARSER_REF`, and the default is
`main`, not a tag.

## Defects Found In A Local Run

On 2026-09-26 the release scripts ran on the machine of the operator, with the
tag name `v0.1.0-local`. The run found three more defects. Each is corrected
in the working tree:

| Defect | Correction |
|---|---|
| The crawler manifest, `stophammer-crawl.service`, `PKGBUILD` and `.SRCINFO` name `crawler-crawl.env`. Commit `99ef400` renamed the file to `crawler-feed.env.example` | Each file names `crawler-feed.env` |
| `verify-release.sh`, `verify-arch-packages.sh` and `.SRCINFO` expect the retired resolver binaries | The checks are removed |
| `verify-arch-packages.sh` names each package with the tag. `makepkg` names it with `pkgver`, so no tag can pass | The script reads `pkgver` and `pkgrel` from the `PKGBUILD` |

Also, `Cargo.toml`, `PKGBUILD` and `.SRCINFO` gave the repository URL
`https://github.com/v4v-tools/stophammer`, which answers `404`. Each now gives
`https://github.com/InTheMorning/stophammer`.

After the corrections, `publish-release.sh` and `verify-release.sh` pass for
the three tarballs. The Arch build did not run on this machine, because it
has no `/etc/makepkg.conf`. The release candidate is its first real run.

## A Decision Before The Release

`install.sh` downloads `stophammer-linux-x86_64` from the repository
`stophammer/stophammer`. Neither exists. The workflow publishes role tarballs,
Arch packages and images. `docs/operations.md` calls the script "the legacy
direct-binary path". ADR 0010 §2 and §3 still describe these assets and this
script. The operator decides one of these:

- Delete `install.sh`, and supersede ADR 0010 §2 and §3 with a record of the
  present assets.
- Correct `install.sh` to download a role tarball, and amend ADR 0010.

## Tasks

### R1: Correct the release workflow

In `.github/workflows/release.yml`:

- In each job that builds the crawler, add two `actions/checkout@v5` steps
  after the first one. One checks out `InTheMorning/stophammer-crawler` to
  `stophammer-crawler`, and one checks out `InTheMorning/stophammer-parser` to
  `stophammer-parser`. Each uses `ref: ${{ github.ref_name }}`, so the three
  repositories release the same tag.
- The image job of `stophammer-crawler` gives the build argument
  `STOPHAMMER_PARSER_REF=${{ github.ref_name }}`.
- When a crate repository has no tag of that name, the checkout fails. That
  is correct: a release needs the tag in each repository.

Check: `actionlint` on the file, when it is installed. The release candidate
is the real check.

### R2: A release candidate

The operator gives the tag `v0.1.0-rc.1` in the parser, then the crawler,
then `stophammer`. The tag of `stophammer` starts the workflow. A tag with
`-` gives no `latest` image tag, because of the rule in the image job.

The candidate passes when these three conditions are true:

- Each job ends green.
- The GitHub release holds three tarballs, `SHA256SUMS-v0.1.0-rc.1.txt` and
  the Arch packages.
- GHCR holds the three images with the tag `0.1.0-rc.1`.

A failed candidate is corrected, and the next one is `v0.1.0-rc.2`. A tag is
never moved.

### R3: The release

The operator gives the tag `v0.1.0` in the parser, then the crawler, then
`stophammer`, at the commits of the candidate that passed. The release notes
name the ADRs that the release holds.

### R4: After the release

- `AGENTS.md` names the release.
- The two client request files tell v4vmm and musicindex.org that 0.1.0 is
  the first release, and that `info.version` of `/openapi.json` gives it.

## Open Decisions

1. **The next version.** Each `Cargo.toml` stays `0.1.0` until the next
   release. The operator decides when a deploy is a release. A rule for the
   version number needs an ADR, because it binds each later release. ADR 0044
   already makes a breaking change of `v1` need a new path version.
2. **A workflow for the crate repositories.** They have no CI. A crate commit
   is checked only on the machine of the operator.

## ADR 0061

ADR 0061 is not in this release. It stays Proposed. The operator takes the
publisher role to the namespace, to MSP-2.0 and to Wavlake, and the index
collects the evidence at each step. The
[adoption plan](publisher-rel-adoption-plan.md) gives the steps.

## Commands For The Operator

The tags are git write commands. The operator runs them.

```bash
# Release candidate, after R1 is committed and pushed
cd /home/citizen/build/stophammer/stophammer-parser && git tag -a v0.1.0-rc.1 -m "0.1.0 release candidate 1" && git push origin v0.1.0-rc.1
cd /home/citizen/build/stophammer/stophammer-crawler && git tag -a v0.1.0-rc.1 -m "0.1.0 release candidate 1" && git push origin v0.1.0-rc.1
cd /home/citizen/build/stophammer && git tag -a v0.1.0-rc.1 -m "0.1.0 release candidate 1" && git push origin v0.1.0-rc.1
```

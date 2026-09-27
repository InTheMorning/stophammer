# ADR 0065: A Release Promotes Its Candidate

## Status
Proposed

## Date
2026-09-26

## Context
[ADR 0063](0063-a-release-publishes-role-packages.md) gives the release
assets and the tags. On 2026-09-26 each tag builds all assets again. The run
of `v0.1.0-rc.2` gave these times:

| Job | Time |
|---|---|
| Image `stophammer-node` | 3 h 05 min |
| Image `stophammer-indexer` | 1 h 50 min |
| Image `stophammer-crawler` | 59 min |
| Arch packages | 5 min 28 s |
| Tarballs | 5 min 16 s |

The run took 3 h 11 min. Three facts cause most of this time:

1. The runner is x86_64. Each image job compiles its `arm64` half under QEMU.
2. The indexer and node images hold the same binary, but two jobs compile it
   with two cache scopes.
3. A release tag on the commits of a passed candidate compiles again. The
   release binaries are then not the bytes that passed.

## Decision

### 1. Only a candidate builds

A tag with a hyphen, for example `v0.1.0-rc.3`, is a candidate. It builds and
checks each asset of ADR 0063 section 3.

### 2. A release promotes its candidate

A tag with no hyphen, for example `v0.1.0`, builds nothing. The workflow finds
the newest candidate `<tag>-rc.N` on the same commit in each of the three
repositories. It then publishes the files of that candidate under the release
tag:

- The tarballs get the name and the top directory of the release tag. Each
  file in a tarball stays the same.
- The Arch packages stay the same files.
- Each image gets the release tags from the candidate image, with no build.

The workflow runs `verify-release.sh` and `verify-arch-packages.sh` on the
candidate files and on the promoted files.

### 3. A release with no candidate stops

The workflow stops before it publishes a file in two conditions. No
candidate is on the commit, or the crawler or the parser has the release tag on
a different commit. The operator then gives a new candidate on the correct commits.

## Consequences

- A release is the bytes that passed as a candidate.
- A release run takes minutes. A candidate builds each image platform on a
  native runner, and builds the indexer and node images in one job.
- A release needs a candidate that passed on the same commits. A tag is never
  moved, so a correction needs a new candidate.
- The first release with this workflow is also the first test of the
  promotion on GitHub. If it fails, it stops before it publishes a file.

## Checks

Mechanical:

- `scripts/promote-release.sh` gives the release assets from the candidate
  assets and checks the two sets. It was run on 2026-09-26 on the assets of
  `v0.1.0-rc.2`. Each binary and each Arch package was the same bytes.
- `actionlint` with `shellcheck` finds no error in
  `.github/workflows/release.yml`.

Manual, because a test cannot give a tag to GitHub:

- The operator checks that the release page holds each asset, and that each
  image has the release tags.

#!/usr/bin/env bash
set -euo pipefail

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"

version="${1:-${STOPHAMMER_RELEASE_VERSION:-${GITHUB_REF_NAME:-}}}"
if [[ -z "$version" ]]; then
  version="$(git -C "$repo_root" describe --tags --always --dirty 2>/dev/null || echo dev)"
fi

export STOPHAMMER_RELEASE_VERSION="$version"

bash "$repo_root/scripts/assemble-release.sh"

dist_dir="${DIST_DIR:-"$repo_root/dist"}"
checksums_path="$dist_dir/SHA256SUMS-${version}.txt"

(
  cd "$dist_dir"
  sha256sum \
    "stophammer-indexer-${version}.tar.gz" \
    "stophammer-node-${version}.tar.gz" \
    "stophammer-crawler-${version}.tar.gz" \
    > "$checksums_path"
)

printf 'published release artifacts under %s\n' "$dist_dir"
printf 'checksums: %s\n' "$checksums_path"

#!/usr/bin/env bash
# Make the assets of a release from the assets of its candidate, with no
# build. The binaries and the Arch packages stay the same bytes. Only the
# tarball names and their top directory change to the release tag.
#
# Usage: promote-release.sh <candidate-tag> <release-tag> <candidate-dir> <out-dir>
set -euo pipefail

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"

if [[ $# -ne 4 ]]; then
  printf 'usage: %s <candidate-tag> <release-tag> <candidate-dir> <out-dir>\n' "$0" >&2
  exit 2
fi

candidate="$1"
release="$2"
candidate_dir="$(cd "$3" && pwd)"
out_dir="$4"

packages=(
  "stophammer-indexer"
  "stophammer-node"
  "stophammer-crawler"
)

work_dir="$(mktemp -d)"
trap 'rm -rf "$work_dir"' EXIT

# The candidate must pass its own checks before anything is promoted.
DIST_DIR="$candidate_dir" bash "$repo_root/scripts/verify-release.sh" "$candidate"
DIST_DIR="$candidate_dir" bash "$repo_root/scripts/verify-arch-packages.sh" "$candidate"

mkdir -p "$out_dir"
out_dir="$(cd "$out_dir" && pwd)"

for package_name in "${packages[@]}"; do
  extract_root="$work_dir/$package_name"
  mkdir -p "$extract_root"
  tar -xpzf "$candidate_dir/${package_name}-${candidate}.tar.gz" -C "$extract_root"
  mv "$extract_root/${package_name}-${candidate}" "$extract_root/${package_name}-${release}"
  tar -C "$extract_root" -czf "$out_dir/${package_name}-${release}.tar.gz" "${package_name}-${release}"
done

(
  cd "$out_dir"
  sha256sum \
    "stophammer-indexer-${release}.tar.gz" \
    "stophammer-node-${release}.tar.gz" \
    "stophammer-crawler-${release}.tar.gz" \
    > "SHA256SUMS-${release}.txt"
)

cp "$candidate_dir"/*.pkg.tar.zst "$out_dir/"
(
  cd "$out_dir"
  sha256sum ./*.pkg.tar.zst > "SHA256SUMS-arch-${release}.txt"
)

DIST_DIR="$out_dir" bash "$repo_root/scripts/verify-release.sh" "$release"
DIST_DIR="$out_dir" bash "$repo_root/scripts/verify-arch-packages.sh" "$release"

printf 'promoted %s to %s under %s\n' "$candidate" "$release" "$out_dir"

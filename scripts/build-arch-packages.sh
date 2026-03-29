#!/usr/bin/env bash
set -euo pipefail

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
dist_dir="${DIST_DIR:-"$repo_root/dist"}"
version="${1:-${STOPHAMMER_RELEASE_VERSION:-${GITHUB_REF_NAME:-}}}"

if [[ -z "$version" ]]; then
  version="$(git -C "$repo_root" describe --tags --always --dirty 2>/dev/null || echo dev)"
fi

arch_dir="$repo_root/packaging/arch"
arch_dist_dir="$dist_dir/arch"

mkdir -p "$arch_dist_dir"

(
  cd "$arch_dir"
  STOPHAMMER_RELEASE_VERSION="$version" makepkg -f --cleanbuild
)

find "$arch_dir" -maxdepth 1 -type f \( -name '*.pkg.tar.zst' -o -name '*.pkg.tar.zst.sig' \) -exec cp {} "$arch_dist_dir/" \;

(
  cd "$arch_dist_dir"
  sha256sum ./*.pkg.tar.zst > "SHA256SUMS-arch-${version}.txt"
)

printf 'built Arch packages under %s\n' "$arch_dist_dir"

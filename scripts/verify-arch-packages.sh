#!/usr/bin/env bash
set -euo pipefail

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
dist_dir="${DIST_DIR:-"$repo_root/dist/arch"}"
version="${1:-${STOPHAMMER_RELEASE_VERSION:-${GITHUB_REF_NAME:-}}}"

if [[ -z "$version" ]]; then
  version="$(git -C "$repo_root" describe --tags --always --dirty 2>/dev/null || echo dev)"
fi

check_file() {
  local path="$1"
  if [[ ! -f "$path" ]]; then
    printf 'missing expected file: %s\n' "$path" >&2
    exit 1
  fi
}

package_path() {
  local package_name="$1"
  printf '%s/%s-%s-1-x86_64.pkg.tar.zst\n' "$dist_dir" "$package_name" "$version"
}

check_pkginfo_field() {
  local pkg="$1"
  local expected="$2"

  if ! bsdtar -xOf "$pkg" .PKGINFO | grep -Fqx "$expected"; then
    printf 'missing package metadata in %s: %s\n' "$pkg" "$expected" >&2
    exit 1
  fi
}

check_archive_member() {
  local pkg="$1"
  local member="$2"

  if ! bsdtar -tf "$pkg" | grep -Fqx "$member"; then
    printf 'missing package member in %s: %s\n' "$pkg" "$member" >&2
    exit 1
  fi
}

verify_package() {
  local package_name="$1"
  local pkg
  pkg="$(package_path "$package_name")"
  check_file "$pkg"

  case "$package_name" in
    stophammer-indexer)
      check_pkginfo_field "$pkg" 'pkgname = stophammer-indexer'
      check_pkginfo_field "$pkg" 'conflict = stophammer-node'
      check_archive_member "$pkg" 'usr/bin/stophammer'
      check_archive_member "$pkg" 'usr/bin/stophammer-resolverd'
      check_archive_member "$pkg" 'usr/bin/stophammer-resolverctl'
      check_archive_member "$pkg" 'usr/lib/systemd/system/stophammer-primary.service'
      check_archive_member "$pkg" 'etc/stophammer/primary.env'
      ;;
    stophammer-node)
      check_pkginfo_field "$pkg" 'pkgname = stophammer-node'
      check_pkginfo_field "$pkg" 'conflict = stophammer-indexer'
      check_archive_member "$pkg" 'usr/bin/stophammer'
      check_archive_member "$pkg" 'usr/lib/systemd/system/stophammer-community.service'
      check_archive_member "$pkg" 'etc/stophammer/community.env'
      ;;
    stophammer-crawler)
      check_pkginfo_field "$pkg" 'pkgname = stophammer-crawler'
      check_archive_member "$pkg" 'usr/bin/stophammer-crawler'
      check_archive_member "$pkg" 'usr/lib/systemd/system/stophammer-gossip.service'
      check_archive_member "$pkg" 'usr/lib/systemd/system/stophammer-import.service'
      check_archive_member "$pkg" 'usr/lib/systemd/system/stophammer-crawl.service'
      check_archive_member "$pkg" 'etc/stophammer/crawler-gossip.env'
      ;;
    *)
      printf 'unknown package: %s\n' "$package_name" >&2
      exit 1
      ;;
  esac
}

check_file "$dist_dir/SHA256SUMS-arch-${version}.txt"

(
  cd "$dist_dir"
  sha256sum --check "SHA256SUMS-arch-${version}.txt"
)

verify_package 'stophammer-indexer'
verify_package 'stophammer-node'
verify_package 'stophammer-crawler'

printf 'verified Arch packages for %s\n' "$version"

#!/usr/bin/env bash
set -euo pipefail

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
dist_dir="${DIST_DIR:-"$repo_root/dist"}"
version="${1:-${STOPHAMMER_RELEASE_VERSION:-${GITHUB_REF_NAME:-}}}"

if [[ -z "$version" ]]; then
  version="$(git -C "$repo_root" describe --tags --always --dirty 2>/dev/null || echo dev)"
fi

work_dir="$(mktemp -d)"
trap 'rm -rf "$work_dir"' EXIT

check_file() {
  local path="$1"
  if [[ ! -f "$path" ]]; then
    printf 'missing expected file: %s\n' "$path" >&2
    exit 1
  fi
}

check_executable() {
  local path="$1"
  check_file "$path"
  if [[ ! -x "$path" ]]; then
    printf 'expected executable: %s\n' "$path" >&2
    exit 1
  fi
}

verify_package() {
  local package_name="$1"
  local tarball="$dist_dir/${package_name}-${version}.tar.gz"
  local extract_root="$work_dir/${package_name}"
  local package_root="$extract_root/${package_name}-${version}"

  check_file "$tarball"
  mkdir -p "$extract_root"
  tar -xzf "$tarball" -C "$extract_root"

  check_file "$package_root/README.md"
  check_file "$package_root/LICENSE"

  case "$package_name" in
    stophammer-indexer)
      check_executable "$package_root/bin/stophammer"
      check_executable "$package_root/bin/stophammer-resolverd"
      check_executable "$package_root/bin/stophammer-resolverctl"
      check_file "$package_root/systemd/stophammer-primary.service"
      check_file "$package_root/systemd/stophammer-resolverd.service"
      "$package_root/bin/stophammer-resolverctl" --help >/dev/null
      ;;
    stophammer-node)
      check_executable "$package_root/bin/stophammer"
      check_file "$package_root/systemd/stophammer-community.service"
      ;;
    stophammer-crawler)
      check_executable "$package_root/bin/stophammer-crawler"
      check_file "$package_root/systemd/stophammer-gossip.service"
      check_file "$package_root/systemd/stophammer-import.service"
      check_file "$package_root/systemd/stophammer-crawl.service"
      "$package_root/bin/stophammer-crawler" --help >/dev/null
      ;;
    *)
      printf 'unknown package: %s\n' "$package_name" >&2
      exit 1
      ;;
  esac
}

check_file "$dist_dir/SHA256SUMS-${version}.txt"

(
  cd "$dist_dir"
  sha256sum --check "SHA256SUMS-${version}.txt"
)

verify_package stophammer-indexer
verify_package stophammer-node
verify_package stophammer-crawler

printf 'verified release tarballs for %s\n' "$version"

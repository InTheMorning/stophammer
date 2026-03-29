#!/usr/bin/env bash
set -euo pipefail

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
dist_dir="${DIST_DIR:-"$repo_root/dist"}"
version="${STOPHAMMER_RELEASE_VERSION:-$(git -C "$repo_root" describe --tags --always --dirty 2>/dev/null || echo dev)}"
root_release_dir="${ROOT_RELEASE_DIR:-"$repo_root/target/release"}"
crawler_release_dir="${CRAWLER_RELEASE_DIR:-"$repo_root/stophammer-crawler/target/release"}"

packages=(
  "stophammer-indexer"
  "stophammer-node"
  "stophammer-crawler"
)

resolve_source() {
  local raw="$1"
  raw="${raw//@ROOT@/$repo_root}"
  raw="${raw//@ROOT_RELEASE@/$root_release_dir}"
  raw="${raw//@CRAWLER_RELEASE@/$crawler_release_dir}"
  printf '%s\n' "$raw"
}

ensure_root_release_binaries() {
  cargo build --release --bins
}

ensure_crawler_release_binary() {
  cargo build --manifest-path "$repo_root/stophammer-crawler/Cargo.toml" --release --bin stophammer-crawler
}

assemble_package() {
  local package_name="$1"
  local manifest="$repo_root/packaging/releases/${package_name}.manifest"
  local staging_root="$dist_dir/${package_name}-${version}"
  local tarball="$dist_dir/${package_name}-${version}.tar.gz"

  rm -rf "$staging_root"
  mkdir -p "$staging_root"

  while IFS='|' read -r source target; do
    [[ -z "$source" ]] && continue
    local resolved_source
    resolved_source="$(resolve_source "$source")"
    if [[ ! -f "$resolved_source" ]]; then
      printf 'missing source for %s: %s\n' "$package_name" "$resolved_source" >&2
      exit 1
    fi
    mkdir -p "$staging_root/$(dirname "$target")"
    install -m 0644 "$resolved_source" "$staging_root/$target"
    case "$target" in
      bin/*)
        chmod 0755 "$staging_root/$target"
        ;;
    esac
  done < "$manifest"

  tar -C "$dist_dir" -czf "$tarball" "${package_name}-${version}"
  printf 'assembled %s\n' "$tarball"
}

mkdir -p "$dist_dir"

ensure_root_release_binaries
ensure_crawler_release_binary

for package_name in "${packages[@]}"; do
  assemble_package "$package_name"
done

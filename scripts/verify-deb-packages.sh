#!/usr/bin/env bash
# Check the Debian packages of build-deb-packages.sh (ADR 0105). The check
# reads each package with dpkg-deb and does not install it.
set -euo pipefail

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
dist_dir="${DIST_DIR:-"$repo_root/dist/deb"}"
version="${1:-${STOPHAMMER_RELEASE_VERSION:-${GITHUB_REF_NAME:-}}}"

if [[ -z "$version" ]]; then
  version="$(git -C "$repo_root" describe --tags --always --dirty 2>/dev/null || echo dev)"
fi

check_file() {
  local path="$1"
  if [[ ! -f "$path" ]]; then
    printf 'ADR 0105: missing expected file: %s\n' "$path" >&2
    exit 1
  fi
}

package_path() {
  local package_name="$1"
  local matches=("$dist_dir/${package_name}"_*_amd64.deb)
  if [[ "${#matches[@]}" -ne 1 || ! -f "${matches[0]}" ]]; then
    printf 'ADR 0105: expected one package of %s in %s\n' "$package_name" "$dist_dir" >&2
    exit 1
  fi
  printf '%s\n' "${matches[0]}"
}

check_field() {
  local deb="$1"
  local field="$2"
  local expected="$3"
  local actual
  actual="$(dpkg-deb --field "$deb" "$field")"
  if [[ "$actual" != *"$expected"* ]]; then
    printf 'ADR 0105: field %s of %s is "%s". It must contain "%s".\n' \
      "$field" "$deb" "$actual" "$expected" >&2
    exit 1
  fi
}

check_member() {
  local deb="$1"
  local member="$2"
  if ! dpkg-deb --contents "$deb" | awk '{print $6}' | grep -Fqx "./$member"; then
    printf 'ADR 0105: missing package member in %s: %s\n' "$deb" "$member" >&2
    exit 1
  fi
}

check_control_member() {
  local deb="$1"
  local member="$2"
  local control_dir
  control_dir="$(mktemp -d)"
  dpkg-deb --control "$deb" "$control_dir"
  if [[ ! -f "$control_dir/$member" ]]; then
    rm -rf "$control_dir"
    printf 'ADR 0105: missing control file in %s: %s\n' "$deb" "$member" >&2
    exit 1
  fi
  rm -rf "$control_dir"
}

verify_package() {
  local package_name="$1"
  local deb
  deb="$(package_path "$package_name")"

  check_field "$deb" Package "$package_name"
  check_field "$deb" Depends 'systemd'
  check_control_member "$deb" postinst
  check_control_member "$deb" prerm
  check_control_member "$deb" postrm
  check_control_member "$deb" conffiles

  case "$package_name" in
    stophammer-indexer)
      check_field "$deb" Conflicts 'stophammer-node'
      check_member "$deb" 'usr/bin/stophammer'
      check_member "$deb" 'usr/lib/systemd/system/stophammer-primary.service'
      check_member "$deb" 'usr/lib/sysusers.d/stophammer.conf'
      check_member "$deb" 'usr/lib/tmpfiles.d/stophammer.conf'
      check_member "$deb" 'etc/stophammer/primary.env'
      ;;
    stophammer-node)
      check_field "$deb" Conflicts 'stophammer-indexer'
      check_member "$deb" 'usr/bin/stophammer'
      check_member "$deb" 'usr/lib/systemd/system/stophammer-community.service'
      check_member "$deb" 'usr/lib/sysusers.d/stophammer.conf'
      check_member "$deb" 'usr/lib/tmpfiles.d/stophammer.conf'
      check_member "$deb" 'etc/stophammer/community.env'
      ;;
    stophammer-crawler)
      check_member "$deb" 'usr/bin/stophammer-crawler'
      check_member "$deb" 'usr/lib/systemd/system/stophammer-gossip.service'
      check_member "$deb" 'usr/lib/systemd/system/stophammer-import.service'
      check_member "$deb" 'usr/lib/systemd/system/stophammer-crawl.service'
      check_member "$deb" 'usr/lib/sysusers.d/stophammer-crawler.conf'
      check_member "$deb" 'usr/lib/tmpfiles.d/stophammer-crawler.conf'
      check_member "$deb" 'etc/stophammer/crawler-gossip.env'
      ;;
    *)
      printf 'ADR 0105: unknown package: %s\n' "$package_name" >&2
      exit 1
      ;;
  esac
}

check_file "$dist_dir/SHA256SUMS-deb-${version}.txt"

(
  cd "$dist_dir"
  sha256sum --check "SHA256SUMS-deb-${version}.txt"
)

verify_package 'stophammer-indexer'
verify_package 'stophammer-node'
verify_package 'stophammer-crawler'

printf 'verified Debian packages for %s\n' "$version"

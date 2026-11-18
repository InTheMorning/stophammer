#!/usr/bin/env bash
# Install each Debian package of build-deb-packages.sh, check the service
# user, the data folder and the units, and remove the package (ADR 0105).
# The script needs root and a host with systemd, such as a CI runner.
set -euo pipefail

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
dist_dir="${DIST_DIR:-"$repo_root/dist/deb"}"

fail() {
  printf 'ADR 0105: %s\n' "$1" >&2
  exit 1
}

check_install() {
  local package_name="$1"
  local user="$2"
  local data_dir="$3"
  local binary="$4"
  shift 4
  local units=("$@")

  local debs=("$dist_dir/${package_name}"_*_amd64.deb)
  apt-get install -y "${debs[0]}"

  getent passwd "$user" > /dev/null || fail "$package_name did not make the user $user"
  [[ -d "$data_dir" ]] || fail "$package_name did not make $data_dir"
  [[ "$(stat -c %U "$data_dir")" == "$user" ]] || fail "$data_dir does not belong to $user"
  command -v "$binary" > /dev/null || fail "$package_name did not install $binary"
  if [[ "$binary" == "stophammer" ]]; then
    # The binary must start on this host (musicindex ADR 0083 R-VERSION-2).
    stophammer --version || fail "stophammer --version failed after the install of $package_name"
  fi
  local unit
  for unit in "${units[@]}"; do
    systemctl cat "$unit" > /dev/null || fail "systemd does not find $unit of $package_name"
  done

  apt-get remove -y "$package_name"
  printf 'install check passed for %s\n' "$package_name"
}

check_install stophammer-indexer stophammer /var/lib/stophammer stophammer \
  stophammer-primary.service
check_install stophammer-node stophammer /var/lib/stophammer stophammer \
  stophammer-community.service
check_install stophammer-crawler stophammer-crawler /var/lib/stophammer-crawler stophammer-crawler \
  stophammer-gossip.service stophammer-import.service stophammer-crawl.service

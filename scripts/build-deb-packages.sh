#!/usr/bin/env bash
# Build one Debian package for each role from the role tarballs (ADR 0105).
#
# The input is the tarballs of publish-release.sh under dist/. Each file goes
# to the path that packaging/arch/PKGBUILD gives it. The tool is dpkg-deb.
#
# Usage: build-deb-packages.sh [<tag>]
set -euo pipefail

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
dist_dir="${DIST_DIR:-"$repo_root/dist"}"
version="${1:-${STOPHAMMER_RELEASE_VERSION:-${GITHUB_REF_NAME:-}}}"

if [[ -z "$version" ]]; then
  version="$(git -C "$repo_root" describe --tags --always --dirty 2>/dev/null || echo dev)"
fi

deb_dist_dir="$dist_dir/deb"
debian_dir="$repo_root/packaging/debian"
deb_revision="1"
deb_arch="amd64"
maintainer="stophammer maintainers <https://github.com/InTheMorning/stophammer/issues>"
homepage="https://github.com/InTheMorning/stophammer"

work_dir="$(mktemp -d)"
trap 'rm -rf "$work_dir"' EXIT

# The Debian version of a crate. A Cargo pre-release such as 0.1.0-rc.2
# becomes 0.1.0~rc.2, so that it sorts before 0.1.0.
crate_deb_version() {
  local manifest="$1"
  local cargo_version
  cargo_version="$(sed -n 's/^version *= *"\(.*\)"/\1/p' "$manifest" | head -n 1)"
  if [[ -z "$cargo_version" ]]; then
    printf 'ADR 0105: no version in %s\n' "$manifest" >&2
    exit 1
  fi
  printf '%s-%s\n' "${cargo_version/-/\~}" "$deb_revision"
}

# The Debian packages of the shared libraries of one binary. A library with
# no entry here stops the build, so that no dependency is missing.
library_depends() {
  local binary="$1"
  local library glibc
  local -a depends=()
  while read -r library; do
    case "$library" in
      libc.so.6 | libm.so.6 | ld-linux-x86-64.so.2 | libdl.so.2 | libpthread.so.0 | librt.so.1)
        ;;
      libgcc_s.so.1)
        depends+=("libgcc-s1")
        ;;
      *)
        printf 'ADR 0105: %s needs %s, and build-deb-packages.sh has no Debian package for it. Add the library to library_depends.\n' \
          "$binary" "$library" >&2
        exit 1
        ;;
    esac
  done < <(readelf -d "$binary" | sed -n 's/.*(NEEDED).*\[\(.*\)\]/\1/p')

  glibc="$(objdump -T "$binary" | grep -o 'GLIBC_[0-9.]*' | sed 's/GLIBC_//' | sort -uV | tail -n 1)"
  printf 'libc6 (>= %s)' "$glibc"
  local depend
  for depend in "${depends[@]}"; do
    printf ', %s' "$depend"
  done
  printf '\n'
}

# Copy the files of one role tarball to the package tree. The paths are the
# paths of _install_bundle_tree in packaging/arch/PKGBUILD.
install_bundle_tree() {
  local package_name="$1"
  local bundle_dir="$2"
  local pkg_root="$3"

  local path rel target
  while IFS= read -r -d '' path; do
    rel="${path#"${bundle_dir}"/}"
    case "$rel" in
      bin/*)
        install -Dm755 "$path" "$pkg_root/usr/bin/${rel#bin/}"
        ;;
      env/crawler-podcastindex.env.example)
        # ADR 0081 §2: the file holds the PodcastIndex API secret.
        target="/etc/stophammer/${rel#env/}"
        install -Dm600 "$path" "$pkg_root${target%.example}"
        ;;
      env/*.env.example)
        target="/etc/stophammer/${rel#env/}"
        install -Dm644 "$path" "$pkg_root${target%.example}"
        ;;
      systemd/*)
        install -Dm644 "$path" "$pkg_root/usr/lib/systemd/system/${rel#systemd/}"
        ;;
      sysusers.d/*)
        install -Dm644 "$path" "$pkg_root/usr/lib/sysusers.d/${rel#sysusers.d/}"
        ;;
      tmpfiles.d/*)
        install -Dm644 "$path" "$pkg_root/usr/lib/tmpfiles.d/${rel#tmpfiles.d/}"
        ;;
      README.md)
        install -Dm644 "$path" "$pkg_root/usr/share/doc/$package_name/README.md"
        ;;
      LICENSE)
        install -Dm644 "$path" "$pkg_root/usr/share/doc/$package_name/copyright"
        ;;
    esac
  done < <(find "$bundle_dir" -type f -print0)
}

build_package() {
  local package_name="$1"
  local deb_version="$2"
  local binary_name="$3"
  local relations="$4"
  local description="$5"

  local tarball="$dist_dir/${package_name}-${version}.tar.gz"
  if [[ ! -f "$tarball" ]]; then
    printf 'ADR 0105: missing role tarball %s. Run publish-release.sh first.\n' "$tarball" >&2
    exit 1
  fi

  local extract_root="$work_dir/extract/$package_name"
  local pkg_root="$work_dir/root/$package_name"
  mkdir -p "$extract_root" "$pkg_root/DEBIAN"
  tar -xzf "$tarball" -C "$extract_root"
  install_bundle_tree "$package_name" "$extract_root/${package_name}-${version}" "$pkg_root"

  if [[ "$binary_name" == "stophammer" ]]; then
    install -d "$pkg_root/usr/share/man/man1"
    gzip -9n < "$repo_root/man/stophammer.1" > "$pkg_root/usr/share/man/man1/stophammer.1.gz"
    chmod 644 "$pkg_root/usr/share/man/man1/stophammer.1.gz"
  fi

  local sysusers_conf
  sysusers_conf="$(basename "$(find "$pkg_root/usr/lib/sysusers.d" -name '*.conf' | head -n 1)")"
  sed "s/@CONF@/${sysusers_conf}/g" "$debian_dir/postinst.in" > "$pkg_root/DEBIAN/postinst"
  local units
  units="$(cd "$pkg_root/usr/lib/systemd/system" && find . -type f \( -name '*.service' -o -name '*.timer' \) -printf '%f\n' | sort | tr '\n' ' ')"
  sed "s/@UNITS@/${units% }/g" "$debian_dir/prerm.in" > "$pkg_root/DEBIAN/prerm"
  install -m755 "$debian_dir/postrm" "$pkg_root/DEBIAN/postrm"
  chmod 755 "$pkg_root/DEBIAN/postinst" "$pkg_root/DEBIAN/prerm"

  (cd "$pkg_root" && find etc -type f | sort | sed 's|^|/|') > "$pkg_root/DEBIAN/conffiles"

  local installed_size
  installed_size="$(du -sk --exclude=DEBIAN "$pkg_root" | cut -f1)"

  {
    printf 'Package: %s\n' "$package_name"
    printf 'Version: %s\n' "$deb_version"
    printf 'Architecture: %s\n' "$deb_arch"
    printf 'Maintainer: %s\n' "$maintainer"
    printf 'Installed-Size: %s\n' "$installed_size"
    printf 'Depends: %s, systemd\n' "$(library_depends "$pkg_root/usr/bin/$binary_name")"
    if [[ -n "$relations" ]]; then
      printf '%s\n' "$relations"
    fi
    printf 'Section: net\n'
    printf 'Priority: optional\n'
    printf 'Homepage: %s\n' "$homepage"
    printf 'Description: %s\n' "$description"
  } > "$pkg_root/DEBIAN/control"

  mkdir -p "$deb_dist_dir"
  dpkg-deb --root-owner-group -Zxz --build "$pkg_root" \
    "$deb_dist_dir/${package_name}_${deb_version}_${deb_arch}.deb"
}

stophammer_version="$(crate_deb_version "$repo_root/Cargo.toml")"
crawler_version="$(crate_deb_version "$repo_root/stophammer-crawler/Cargo.toml")"

rm -rf "$deb_dist_dir"

build_package 'stophammer-indexer' "$stophammer_version" 'stophammer' \
  $'Conflicts: stophammer-node\nSuggests: stophammer-crawler' \
  'Primary and indexer role of the Stophammer music index'
build_package 'stophammer-node' "$stophammer_version" 'stophammer' \
  'Conflicts: stophammer-indexer' \
  'Community node role of the Stophammer music index'
build_package 'stophammer-crawler' "$crawler_version" 'stophammer-crawler' \
  'Suggests: stophammer-indexer' \
  'Crawler role of the Stophammer music index'

(
  cd "$deb_dist_dir"
  sha256sum ./*.deb > "SHA256SUMS-deb-${version}.txt"
)

printf 'built Debian packages under %s\n' "$deb_dist_dir"

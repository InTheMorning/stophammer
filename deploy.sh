#!/usr/bin/env bash
#
# Build stophammer locally and deploy to VPS.
# Rsync binaries + slim Dockerfile, build image on VPS (~1s).
#
# Usage:
#   ./deploy.sh              # build and deploy everything
#   ./deploy.sh indexer      # only stophammer-indexer
#   ./deploy.sh crawler      # only stophammer-crawler
#   ./deploy.sh --rebuild    # deploy and rebuild only the primary SQLite DB
#   ./deploy.sh --backup     # copy the primary SQLite DB to ./backups/ first
#   ./deploy.sh --no-cache   # build without Docker layer cache (crawler only)
#   ./deploy.sh --release 0.9.0-rc.1 --backup
#                            # deploy the GHCR images of a release, no build
#   ./deploy.sh --release 0.9.0-rc.1 --create --backup
#                            # make that candidate or release first, then deploy
#
set -euo pipefail

LOCAL_DIR="$(cd "$(dirname "$0")" && pwd)"
# The repository holds no value of one deployment. A value in the environment
# has priority over the ignored file deploy.env.
VPS="${VPS:-}"
VPS_DIR="${VPS_DIR:-}"
NODE_URL="${NODE_URL:-}"
GHCR_PREFIX="${GHCR_PREFIX:-}"
if [[ -f "${LOCAL_DIR}/deploy.env" ]]; then
    env_vps="${VPS}"
    env_vps_dir="${VPS_DIR}"
    env_node_url="${NODE_URL}"
    env_ghcr_prefix="${GHCR_PREFIX}"
    # shellcheck source=/dev/null
    source "${LOCAL_DIR}/deploy.env"
    VPS="${env_vps:-${VPS:-}}"
    VPS_DIR="${env_vps_dir:-${VPS_DIR:-}}"
    NODE_URL="${env_node_url:-${NODE_URL:-}}"
    GHCR_PREFIX="${env_ghcr_prefix:-${GHCR_PREFIX:-}}"
fi

INDEXER_IMAGE="stophammer-indexer"
CRAWLER_IMAGE="stophammer-crawler"
MUSL_TARGET="x86_64-unknown-linux-musl"
# The release workflow publishes each image under the owner of the repository.
if [[ -z "${GHCR_PREFIX}" ]]; then
    origin_owner="$(git -C "${LOCAL_DIR}" config --get remote.origin.url | sed -E 's#^.*github\.com[:/]##; s#/.*$##')"
    GHCR_PREFIX="ghcr.io/${origin_owner,,}"
fi

usage() {
    cat >&2 <<'EOF'
usage: deploy.sh [all|indexer|crawler] [--rebuild] [--backup] [--no-cache]
       deploy.sh [all|indexer|crawler] --release VERSION [--create [--yes]] [--backup]

  all         build and deploy both images (default)
  indexer     build and deploy only the primary image
  crawler     build and deploy only the crawler image
  --rebuild   remove only the primary SQLite database on the VPS before restart
              while preserving other persistent data such as signing.key
  --backup    stop the primary, then copy its SQLite database and the -wal and
              -shm files to backups/ next to the database. The copy runs
              before --rebuild. The deploy stops if the copy fails. After the
              restart, the script downloads the copy to ./backups/ in this
              repository, compares SHA-256 sums, and then deletes the copy on
              the VPS. If the download or the comparison fails, the VPS copy
              stays.
  --no-cache  pass --no-cache to docker buildx build (crawler only)
  --release VERSION
              build nothing. Deploy the GHCR images of VERSION, for example
              0.9.0-rc.1 (ADR 0065). The VPS pulls the images first, and stops
              no service when a pull fails. Then it stops gossip before
              primary (ADR 0062 section 8), pins VERSION in .env, and starts
              primary before gossip. The local tag vVERSION must exist. After
              the deploy, the script compares git_revision of /node/info with
              the commit of that tag. --rebuild and --no-cache are refused.
  --create    with --release: run scripts/create-release.sh VERSION first. It
              sets the version, pushes main, waits for CI, pushes the tags and
              waits for the release workflow in the three repositories. A
              VERSION with no hyphen promotes its newest candidate.
  --yes       with --create: do not ask before the first change.

The script reads these values from the environment, or from deploy.env. Git
ignores deploy.env. deploy.env.example shows the format.

  VPS          the SSH target
  VPS_DIR      the Stophammer folder on the VPS
  NODE_URL     the public address of the primary. --release needs it to check
               /node/info, unless the deploy holds only the crawler
  GHCR_PREFIX  the image registry path. The default comes from the owner of the
               origin remote
EOF
}

what="all"
rebuild=0
backup=0
no_cache=0
target_set=0
release=""
create=0
assume_yes=0

while [[ $# -gt 0 ]]; do
    arg="$1"
    shift
    case "$arg" in
        all|indexer|crawler)
            if [[ "${target_set}" -eq 1 ]]; then
                usage
                exit 1
            fi
            what="$arg"
            target_set=1
            ;;
        --rebuild)
            rebuild=1
            ;;
        --backup)
            backup=1
            ;;
        --no-cache)
            no_cache=1
            ;;
        --release)
            if [[ $# -eq 0 ]]; then
                usage
                exit 1
            fi
            release="$1"
            shift
            ;;
        --create)
            create=1
            ;;
        --yes)
            assume_yes=1
            ;;
        -h|--help)
            usage
            exit 0
            ;;
        *)
            usage
            exit 1
            ;;
    esac
done

if [[ -z "${VPS}" || -z "${VPS_DIR}" ]]; then
    echo >&2 "error: set VPS and VPS_DIR in deploy.env or in the environment."
    echo >&2 "       Copy deploy.env.example to deploy.env to start."
    exit 1
fi

cache_flag=()
[[ "${no_cache}" -eq 1 ]] && cache_flag=(--no-cache)

deploy_indexer=0
deploy_crawler=0
[[ "$what" == "all" || "$what" == "indexer" ]] && deploy_indexer=1
[[ "$what" == "all" || "$what" == "crawler" ]] && deploy_crawler=1

# --- Release mode: stop before each change when a value is missing ---

if [[ -n "${release}" && "${deploy_indexer}" -eq 1 && -z "${NODE_URL}" ]]; then
    echo >&2 "error: --release needs NODE_URL to check /node/info after the deploy."
    echo >&2 "       Set it in deploy.env or in the environment."
    exit 1
fi

# --- Release mode: make the release when asked ---

if [[ "${create}" -eq 1 || "${assume_yes}" -eq 1 ]]; then
    if [[ -z "${release}" ]]; then
        echo >&2 "error: --create and --yes need --release VERSION."
        exit 1
    fi
    if [[ "${create}" -eq 0 ]]; then
        echo >&2 "error: --yes needs --create."
        exit 1
    fi
    yes_flag=()
    [[ "${assume_yes}" -eq 1 ]] && yes_flag=(--yes)
    "${LOCAL_DIR}/scripts/create-release.sh" "${release}" "${yes_flag[@]}"
fi

# --- Release mode: check the version and its local tag ---

expected_revision=""
if [[ -n "${release}" ]]; then
    if [[ ! "${release}" =~ ^[0-9]+\.[0-9]+\.[0-9]+(-rc\.[0-9]+)?$ ]]; then
        echo >&2 "error: --release needs a version such as 0.9.0 or 0.9.0-rc.1, not ${release}"
        exit 1
    fi
    if [[ "${rebuild}" -eq 1 || "${no_cache}" -eq 1 ]]; then
        echo >&2 "error: --release builds nothing and keeps the database."
        echo >&2 "       Do not give --rebuild or --no-cache with it."
        exit 1
    fi
    if ! expected_revision="$(git -C "${LOCAL_DIR}" rev-parse --short "v${release}^{}" 2>/dev/null)"; then
        echo >&2 "error: the local tag v${release} does not exist. Tag the release first."
        exit 1
    fi
    echo ":: Release ${release}, commit ${expected_revision}. No local build."
fi

# --- Build indexer (local cargo) ---

if [[ "${deploy_indexer}" -eq 1 && -z "${release}" ]]; then
    BINARY_DIR="${LOCAL_DIR}/target/${MUSL_TARGET}/release"

    echo ":: Building Rust binaries (local cargo)"
    export STOPHAMMER_GIT_REVISION="$(git rev-parse --short HEAD)$(git status --porcelain --untracked-files=no | grep -q . && echo -dirty || true)"
    export STOPHAMMER_BUILT_AT="$(date -u +%Y-%m-%dT%H:%M:%SZ)"
    cargo build --release --target "${MUSL_TARGET}" --bin stophammer --bin gen_openapi

    echo ":: Syncing binaries + Dockerfile to VPS"
    ssh "${VPS}" "mkdir -p /tmp/stophammer-deploy"
    rsync -z --info=progress2 \
        "${BINARY_DIR}/stophammer" \
        "${BINARY_DIR}/gen_openapi" \
        "${LOCAL_DIR}/Dockerfile.deploy" \
        "${VPS}:/tmp/stophammer-deploy/"
fi

# --- Build crawler (Docker, separate repo) ---

if [[ "${deploy_crawler}" -eq 1 && -z "${release}" ]]; then
    if [[ ! -d "${LOCAL_DIR}/stophammer-crawler" ]]; then
        echo >&2 "error: stophammer-crawler/ not found — clone it first"
        exit 1
    fi
    echo ":: Building ${CRAWLER_IMAGE}"
    (
        mkdir -p "${LOCAL_DIR}/target"
        build_dir="$(mktemp -d "${LOCAL_DIR}/target/deploy-build.XXXXXX")"
        trap 'rm -rf "${build_dir}"' EXIT

        # Exclude Cargo output and local data before the copy to the Docker context.
        # The crawler .dockerignore also excludes these files from direct Docker builds.
        stage_flags=(
            -a
            --exclude=.git
            --exclude=target/
            --exclude=analysis/
            --exclude=.env
            --exclude='.env.*'
            --exclude='*.db'
            --exclude='*.db-shm'
            --exclude='*.db-wal'
            --exclude='*.sqlite*'
            --exclude='*.ndjson'
            --exclude='*.bak'
            --exclude='*.log'
            --exclude=signing.key
            --exclude=tls/
        )
        # Use the parser source files from the sibling directory.
        rsync "${stage_flags[@]}" --exclude=/stophammer-parser/ \
            "${LOCAL_DIR}/stophammer-crawler/" "${build_dir}/stophammer-crawler/"
        rsync "${stage_flags[@]}" \
            "${LOCAL_DIR}/stophammer-parser/" "${build_dir}/stophammer-crawler/stophammer-parser/"

        echo ":: Crawler build context: $(du -sh "${build_dir}/stophammer-crawler" | cut -f1)"
        docker build \
            "${cache_flag[@]}" \
            -t "${CRAWLER_IMAGE}:latest" \
            -f "${build_dir}/stophammer-crawler/Dockerfile" \
            "${build_dir}/stophammer-crawler"
    )

    echo ":: Shipping crawler image to VPS"
    tarball=$(mktemp -d)/${CRAWLER_IMAGE}.tar.gz
    docker save "${CRAWLER_IMAGE}:latest" | gzip > "${tarball}"
    echo "   $(du -h "${tarball}" | cut -f1)"
    scp "${tarball}" "${VPS}:/tmp/"
    rm -rf "$(dirname "${tarball}")"
fi

if [[ "${deploy_indexer}" -eq 0 && "${deploy_crawler}" -eq 0 ]]; then
    usage
    exit 1
fi

# --- Remote: build image + restart ---

backup_stamp="$(date -u +%Y%m%dT%H%M%SZ)"

echo ":: Deploying on VPS"
ssh "${VPS}" \
    REMOTE_REBUILD="${rebuild}" \
    REMOTE_BACKUP="${backup}" \
    BACKUP_STAMP="${backup_stamp}" \
    VPS_DIR="${VPS_DIR}" \
    DEPLOY_INDEXER="${deploy_indexer}" \
    DEPLOY_CRAWLER="${deploy_crawler}" \
    RELEASE="${release}" \
    GHCR_PREFIX="${GHCR_PREFIX}" \
    bash <<'REMOTE'
set -euo pipefail
cd "${VPS_DIR}"

# .env can pin a release image from GHCR (docs/operations.md, Install a
# release). This deploy ships images built from the local tree, so it names
# them for its own compose commands. A variable of the shell overrides the
# same variable in .env, and .env does not change. A role that this deploy
# does not ship keeps the image that it runs now, so the blanket `up -d`
# below does not move it.
running_image() {
    local id
    id="$(docker compose ps -q "$1" 2>/dev/null | head -n1)"
    [[ -n "${id}" ]] && docker inspect -f '{{.Config.Image}}' "${id}"
}
if [[ -n "${RELEASE}" ]]; then
    # Release mode: pull before anything stops, so a missing image stops
    # the deploy with each service still running.
    release_images=()
    [[ "${DEPLOY_INDEXER}" == "1" ]] && release_images+=("${GHCR_PREFIX}/stophammer-indexer:${RELEASE}")
    [[ "${DEPLOY_CRAWLER}" == "1" ]] && release_images+=("${GHCR_PREFIX}/stophammer-crawler:${RELEASE}")
    for image in "${release_images[@]}"; do
        echo "Pulling ${image}..."
        docker pull "${image}"
    done
fi

if [[ -n "${RELEASE}" && "${DEPLOY_INDEXER}" == "1" ]]; then
    export STOPHAMMER_INDEXER_IMAGE="${GHCR_PREFIX}/stophammer-indexer:${RELEASE}"
elif [[ "${DEPLOY_INDEXER}" == "1" ]]; then
    export STOPHAMMER_INDEXER_IMAGE="stophammer-indexer:latest"
elif current="$(running_image primary)" && [[ -n "${current}" ]]; then
    export STOPHAMMER_INDEXER_IMAGE="${current}"
fi
if [[ -n "${RELEASE}" && "${DEPLOY_CRAWLER}" == "1" ]]; then
    export STOPHAMMER_CRAWLER_IMAGE="${GHCR_PREFIX}/stophammer-crawler:${RELEASE}"
elif [[ "${DEPLOY_CRAWLER}" == "1" ]]; then
    export STOPHAMMER_CRAWLER_IMAGE="stophammer-crawler:latest"
elif current="$(running_image gossip)" && [[ -n "${current}" ]]; then
    export STOPHAMMER_CRAWLER_IMAGE="${current}"
fi
echo "Images for this deploy: primary=${STOPHAMMER_INDEXER_IMAGE:-.env} crawler=${STOPHAMMER_CRAWLER_IMAGE:-.env}"

# Load crawler image if shipped
if [[ -z "${RELEASE}" && "${DEPLOY_CRAWLER}" == "1" ]] && [[ -f /tmp/stophammer-crawler.tar.gz ]]; then
    echo "Loading crawler image..."
    docker load < /tmp/stophammer-crawler.tar.gz
    rm /tmp/stophammer-crawler.tar.gz
fi

# Build indexer image on VPS from synced binaries (~1s)
if [[ -z "${RELEASE}" && "${DEPLOY_INDEXER}" == "1" ]] && [[ -d /tmp/stophammer-deploy ]]; then
    echo "Building indexer image from synced binaries..."
    docker build \
        --target stophammer-indexer \
        -t stophammer-indexer:latest \
        -f /tmp/stophammer-deploy/Dockerfile.deploy \
        /tmp/stophammer-deploy
    rm -rf /tmp/stophammer-deploy
fi

# deploy-podping-listener.sh builds the gossip watcher on this host and pins the
# resulting tag in .env. That tag exists nowhere but here, so pulling it fails
# with "pull access denied" and takes the whole deploy down before anything is
# restarted. Only pull when the service is still on its registry default.
podping_image=""
if [[ -f .env ]]; then
    podping_image="$(sed -n 's/^STOPHAMMER_PODPING_IMAGE=//p' .env | tail -n1)"
fi

if [[ -n "${podping_image}" ]]; then
    echo "Skipping podping pull: pinned to locally built ${podping_image}"
    if ! docker image inspect "${podping_image}" >/dev/null 2>&1; then
        echo >&2 "error: ${podping_image} is pinned in .env but is not present on this host"
        echo >&2 "       rebuild it with ./deploy-podping-listener.sh, or remove the"
        echo >&2 "       STOPHAMMER_PODPING_IMAGE line from .env to use the registry image"
        exit 1
    fi
else
    echo "Pulling latest podping image..."
    docker compose pull podping-listener
fi

# live-relay is built locally by deploy-live-relay.sh and never published to a
# registry. The blanket `docker compose up -d` below would otherwise try to pull
# it from Docker Hub and fail the same way. Fail here with something actionable.
relay_image="musicindex-live-relay"
if [[ -f .env ]]; then
    relay_pinned="$(sed -n 's/^STOPHAMMER_LIVE_RELAY_IMAGE=//p' .env | tail -n1)"
    [[ -n "${relay_pinned}" ]] && relay_image="${relay_pinned}"
fi

if ! docker image inspect "${relay_image}" >/dev/null 2>&1; then
    echo >&2 "error: live-relay image ${relay_image} is not present on this host"
    echo >&2 "       run ./deploy-live-relay.sh once to build and start the relay"
    exit 1
fi

if [[ -n "${RELEASE}" ]]; then
    # ADR 0062 section 8: a deploy stops gossip before primary.
    echo "Stopping gossip, then primary..."
    docker compose stop gossip || true
    docker compose stop primary || true
fi

if [[ "${REMOTE_BACKUP}" == "1" ]]; then
    # The primary must be stopped, so that the database and its WAL agree
    # (docs/operations.md, SQLite Backup Procedure, option 1).
    echo "Stopping primary before backup..."
    docker compose stop primary || true

    echo "Copying database files..."
    # -T and </dev/null: this script reaches bash on standard input. Without
    # them, docker compose run reads the rest of the script, and the restart
    # below never runs. That occurred on 2026-09-24 and left primary stopped.
    # --user root: an older backup made backups/ as root. The copy then
    # gives the directory and the new files to the owner of the database, so
    # the download and the delete below can run as the image user.
    # docker-compose.yml drops each capability of primary. Root needs these
    # three to write in /data, which the image user owns, and to chown the copy.
    docker compose run -T --rm --no-deps --user root -e BACKUP_STAMP="${BACKUP_STAMP}" \
        --cap-add CHOWN --cap-add DAC_OVERRIDE --cap-add FOWNER \
        --entrypoint /bin/sh primary -c '
        set -eu
        db_path="${DB_PATH:-/data/stophammer.db}"
        backup_dir="$(dirname "$db_path")/backups"
        backup_base="$backup_dir/$(basename "$db_path" .db)-${BACKUP_STAMP}.db"

        if [ ! -f "$db_path" ]; then
            echo "error: no database at $db_path; nothing to back up" >&2
            exit 1
        fi

        mkdir -p "$backup_dir"

        # The copy needs the size of the three files, and a margin of 64 MiB.
        need_kib=$(du -ck "$db_path" "${db_path}-wal" "${db_path}-shm" 2>/dev/null | tail -n1 | cut -f1)
        need_kib=$((need_kib + 65536))
        free_kib=$(df -Pk "$backup_dir" | tail -n1 | tr -s " " | cut -d" " -f4)
        if [ "$free_kib" -lt "$need_kib" ]; then
            echo "error: $backup_dir has ${free_kib} KiB free, and the copy needs ${need_kib} KiB" >&2
            exit 1
        fi

        cp "$db_path" "$backup_base"
        for suffix in -wal -shm; do
            if [ -f "${db_path}${suffix}" ]; then
                cp "${db_path}${suffix}" "${backup_base}${suffix}"
            fi
        done

        owner="$(stat -c "%u:%g" "$db_path")"
        chown "$owner" "$backup_dir" "$backup_base"*

        src_size=$(wc -c < "$db_path")
        dst_size=$(wc -c < "$backup_base")
        if [ "$src_size" -ne "$dst_size" ]; then
            echo "error: backup size $dst_size is not equal to database size $src_size" >&2
            exit 1
        fi

        echo "Backup written: $backup_base ($dst_size bytes)"
        ls -l "$backup_dir" | tail -n 5
    ' </dev/null
fi

if [[ "${REMOTE_REBUILD}" == "1" ]]; then
    echo "Stopping primary before fresh database..."
    docker compose stop primary || true

    echo "Removing database files..."
    # -T and </dev/null: see the backup step above.
    docker compose run -T --rm --no-deps --entrypoint /bin/sh primary -lc '
        set -euo pipefail
        db_path="${DB_PATH:-/data/stophammer.db}"
        key_path="${KEY_PATH:-/data/signing.key}"

        rm -f "$db_path" "${db_path}-wal" "${db_path}-shm"
        echo "Removed database files: $db_path ${db_path}-wal ${db_path}-shm"

        if [ -e "$key_path" ]; then
            echo "Preserved signing key: $key_path"
        else
            echo "Signing key not found at $key_path; nothing touched outside the DB files"
        fi
    ' </dev/null
fi

if [[ -n "${RELEASE}" ]]; then
    # Pin the release in .env, so that a plain `docker compose up -d` keeps it.
    touch .env
    for var in STOPHAMMER_INDEXER_IMAGE STOPHAMMER_CRAWLER_IMAGE; do
        value="${!var:-}"
        [[ -z "${value}" || "${value}" != "${GHCR_PREFIX}/"* ]] && continue
        if grep -q "^${var}=" .env; then
            sed -i "s#^${var}=.*#${var}=${value}#" .env
        else
            echo "${var}=${value}" >> .env
        fi
    done
    echo "Pinned in .env:"
    grep -n "^STOPHAMMER_.*_IMAGE" .env || true

    echo "Starting primary, then gossip..."
    [[ "${DEPLOY_INDEXER}" == "1" ]] && docker compose up -d --force-recreate --no-build primary
    [[ "${DEPLOY_CRAWLER}" == "1" ]] && docker compose up -d --force-recreate --no-build gossip
fi

echo "Restarting services with orphan cleanup..."
to_recreate=()
if [[ -z "${RELEASE}" ]]; then
    [[ "${DEPLOY_INDEXER}"  == "1" ]] && to_recreate+=(primary)
    [[ "${DEPLOY_CRAWLER}"  == "1" ]] && to_recreate+=(gossip)
fi
if [[ ${#to_recreate[@]} -gt 0 ]]; then
    docker compose up -d --remove-orphans --force-recreate --no-build "${to_recreate[@]}"
fi
docker compose up -d --remove-orphans --no-build

# The override above lasts for this deploy only. Name each pin that a plain
# `docker compose up -d` would bring back.
for var in STOPHAMMER_INDEXER_IMAGE STOPHAMMER_CRAWLER_IMAGE; do
    pinned=""
    [[ -f .env ]] && pinned="$(sed -n "s/^${var}=//p" .env | tail -n1)"
    if [[ -n "${pinned}" && "${pinned}" != "${!var:-}" ]]; then
        echo "note: .env pins ${var}=${pinned}. This deploy runs ${!var}."
        echo "      A plain 'docker compose up -d' or 'docker compose run' uses the pin."
    fi
done

if [[ -n "${RELEASE}" && "${DEPLOY_INDEXER}" == "1" ]]; then
    echo "Primary log lines with error, panic, migration or repair, last 5 minutes:"
    docker compose logs --no-color --since 5m primary \
        | grep -iE "error|panic|migrat|filled|rebuilt|repair" | tail -n 20 || true
fi
REMOTE

# --- Backup: download to this repository, verify, then delete on the VPS ---

# Runs a /bin/sh script in the primary image on the VPS, with the primary
# volume mounted. Standard output comes back unchanged, so a tar stream can
# pass through it. -T turns off the TTY, which would change binary data.
remote_primary_sh() {
    local script="$1"
    ssh "${VPS}" "VPS_DIR=$(printf %q "${VPS_DIR}") BACKUP_STAMP=$(printf %q "${backup_stamp}") SCRIPT=$(printf %q "${script}") bash -s" <<'REMOTE'
set -euo pipefail
cd "${VPS_DIR}"
docker compose run -T --rm --no-deps -e BACKUP_STAMP="${BACKUP_STAMP}" \
    --entrypoint /bin/sh primary -c "${SCRIPT}"
REMOTE
}

# Sets "$@" to the backup files of this deploy, and changes to their directory.
backup_files_sh='
set -eu
db_path="${DB_PATH:-/data/stophammer.db}"
backup_dir="$(dirname "$db_path")/backups"
base="$(basename "$db_path" .db)-${BACKUP_STAMP}.db"
cd "$backup_dir"
set --
for f in "$base" "$base-wal" "$base-shm"; do
    if [ -f "$f" ]; then set -- "$@" "$f"; fi
done
if [ "$#" -eq 0 ]; then
    echo "error: no backup named $base in $backup_dir" >&2
    exit 1
fi
'

if [[ "${backup}" -eq 1 ]]; then
    local_backup_dir="${LOCAL_DIR}/backups"
    mkdir -p "${local_backup_dir}"

    echo ":: Downloading backup ${backup_stamp} to ${local_backup_dir}"
    if ! remote_primary_sh "${backup_files_sh}"'tar -cf - "$@"' \
        | tar -C "${local_backup_dir}" -xf -; then
        echo >&2 "error: download failed. The backup stays on the VPS in the"
        echo >&2 "       primary volume, in backups/, with the stamp ${backup_stamp}."
        exit 1
    fi

    echo ":: Verifying the download"
    remote_sums="$(remote_primary_sh "${backup_files_sh}"'sha256sum "$@"')"
    if ! (cd "${local_backup_dir}" && printf '%s\n' "${remote_sums}" | sha256sum -c -); then
        echo >&2 "error: the local copy is not equal to the VPS copy. The backup"
        echo >&2 "       stays on the VPS, with the stamp ${backup_stamp}."
        exit 1
    fi

    echo ":: Deleting the backup on the VPS"
    remote_primary_sh "${backup_files_sh}"'rm -f "$@"; cd /; rmdir "$backup_dir" 2>/dev/null || true'

    echo ":: Backup saved:"
    ls -l "${local_backup_dir}" | grep -F "${backup_stamp}"
fi

if [[ -n "${release}" && "${deploy_indexer}" -eq 1 ]]; then
    echo ":: Checking ${NODE_URL}/node/info"
    actual_revision=""
    for _ in $(seq 1 30); do
        actual_revision="$(curl -fsS "${NODE_URL}/node/info" 2>/dev/null \
            | sed -n 's/.*"git_revision":"\([^"]*\)".*/\1/p')" || true
        [[ "${actual_revision}" == "${expected_revision}" ]] && break
        sleep 2
    done
    if [[ "${actual_revision}" != "${expected_revision}" ]]; then
        echo >&2 "error: /node/info gives git_revision '${actual_revision}', and v${release} is ${expected_revision}"
        exit 1
    fi
    echo "   git_revision ${actual_revision} is the commit of v${release}"
fi

echo ":: Done"

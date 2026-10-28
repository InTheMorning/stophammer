#!/usr/bin/env bash
#
# Build and deploy the Podcast Index podping gossip watcher on the VPS as
# the bundled Compose podping-listener service.
#
# Usage:
#   ./deploy-podping-listener.sh
#
# Optional overrides:
#   VPS=user@example.org ./deploy-podping-listener.sh
#   VPS_DIR=/srv/stophammer ./deploy-podping-listener.sh
#   PODPING_REF=v0.12.0 ./deploy-podping-listener.sh
#   PODPING_IMAGE=stophammer-podping-gossipwatcher:test ./deploy-podping-listener.sh
#
set -euo pipefail

LOCAL_DIR="$(cd "$(dirname "$0")" && pwd)"
# The repository holds no host and no folder of the VPS. A value in the
# environment has priority over the ignored file deploy.env.
VPS="${VPS:-}"
VPS_DIR="${VPS_DIR:-}"
if [[ -f "${LOCAL_DIR}/deploy.env" ]]; then
    env_vps="${VPS}"
    env_vps_dir="${VPS_DIR}"
    # shellcheck source=/dev/null
    source "${LOCAL_DIR}/deploy.env"
    VPS="${env_vps:-${VPS:-}}"
    VPS_DIR="${env_vps_dir:-${VPS_DIR:-}}"
fi
PODPING_REPO="${PODPING_REPO:-https://github.com/InTheMorning/podping-gossipwatcher.git}"
PODPING_REF="${PODPING_REF:-main}"
PODPING_SRC_DIR="${PODPING_SRC_DIR:-}"
PODPING_IMAGE="${PODPING_IMAGE:-stophammer-podping-gossipwatcher:local}"

usage() {
    cat >&2 <<'EOF'
usage: deploy-podping-listener.sh

Clone/update the podping gossip watcher source on the VPS, build a local
Docker image, set STOPHAMMER_PODPING_IMAGE in the remote Stophammer .env,
recreate podping-listener, and restart archive-backed gossip.

Environment overrides:
  VPS              SSH target
  VPS_DIR          remote Stophammer checkout
  PODPING_REPO     source repository
  PODPING_REF      branch, tag, or commit to build
  PODPING_SRC_DIR  remote source checkout; defaults beside VPS_DIR
  PODPING_IMAGE    local Docker image tag to build and run

Options:
  -h, --help   show this help
EOF
}

for arg in "$@"; do
    case "$arg" in
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

echo ":: Deploying podping listener to ${VPS}"
ssh "${VPS}" \
    VPS_DIR="${VPS_DIR}" \
    PODPING_REPO="${PODPING_REPO}" \
    PODPING_REF="${PODPING_REF}" \
    PODPING_SRC_DIR="${PODPING_SRC_DIR}" \
    PODPING_IMAGE="${PODPING_IMAGE}" \
    bash <<'REMOTE'
set -euo pipefail

if [[ ! -d "${VPS_DIR}" ]]; then
    echo >&2 "error: Stophammer directory not found: ${VPS_DIR}"
    exit 1
fi

if [[ -z "${PODPING_SRC_DIR}" ]]; then
    PODPING_SRC_DIR="$(dirname "${VPS_DIR}")/podping-gossipwatcher"
fi

echo "Syncing podping gossip watcher source..."
if [[ -d "${PODPING_SRC_DIR}/.git" ]]; then
    current_origin="$(git -C "${PODPING_SRC_DIR}" remote get-url origin || true)"
    if [[ "${current_origin}" != "${PODPING_REPO}" ]]; then
        git -C "${PODPING_SRC_DIR}" remote set-url origin "${PODPING_REPO}"
    fi
else
    if [[ -e "${PODPING_SRC_DIR}" ]]; then
        echo >&2 "error: ${PODPING_SRC_DIR} exists but is not a git checkout"
        exit 1
    fi
    git clone "${PODPING_REPO}" "${PODPING_SRC_DIR}"
fi

if [[ -n "$(git -C "${PODPING_SRC_DIR}" status --porcelain)" ]]; then
    echo >&2 "error: ${PODPING_SRC_DIR} has local changes; refusing to overwrite them"
    exit 1
fi

git -C "${PODPING_SRC_DIR}" fetch --tags --prune origin
if git -C "${PODPING_SRC_DIR}" rev-parse --verify --quiet "origin/${PODPING_REF}^{commit}" >/dev/null; then
    git -C "${PODPING_SRC_DIR}" checkout -B "${PODPING_REF}" "origin/${PODPING_REF}"
else
    git -C "${PODPING_SRC_DIR}" checkout "${PODPING_REF}"
fi
source_commit="$(git -C "${PODPING_SRC_DIR}" rev-parse HEAD)"

echo "Building ${PODPING_IMAGE} from ${PODPING_REPO}@${source_commit}..."
docker build \
    --label "org.opencontainers.image.source=${PODPING_REPO}" \
    --label "org.opencontainers.image.revision=${source_commit}" \
    -t "${PODPING_IMAGE}" \
    "${PODPING_SRC_DIR}"

echo "Updating Stophammer Compose image override..."
cd "${VPS_DIR}"
touch .env
tmp_env="$(mktemp)"
awk -v image_line="STOPHAMMER_PODPING_IMAGE=${PODPING_IMAGE}" '
    BEGIN { written = 0 }
    /^STOPHAMMER_PODPING_IMAGE=/ {
        if (written == 0) {
            print image_line
            written = 1
        }
        next
    }
    /^[[:space:]]*listener:latest$/ {
        next
    }
    {
        print
    }
    END {
        if (written == 0) {
            print image_line
        }
    }
' .env > "${tmp_env}"
mv "${tmp_env}" .env

echo "Ensuring crawler stays archive-backed..."
crawler_env="packaging/env/crawler-gossip.compose.env"
if [[ -f "${crawler_env}" ]]; then
    tmp_crawler_env="$(mktemp)"
    awk '
        BEGIN { written = 0 }
        /^[[:space:]]*#?[[:space:]]*GOSSIP_ARCHIVE_DB=/ {
            if (written == 0) {
                print "GOSSIP_ARCHIVE_DB=/podping/archive.db"
                written = 1
            }
            next
        }
        { print }
        END {
            if (written == 0) {
                print "GOSSIP_ARCHIVE_DB=/podping/archive.db"
            }
        }
    ' "${crawler_env}" > "${tmp_crawler_env}"
    mv "${tmp_crawler_env}" "${crawler_env}"
fi

echo "Stopping archive users before recreate..."
docker compose stop gossip podping-listener || true

echo "Recreating podping-listener..."
docker compose up -d --force-recreate podping-listener

echo "Starting archive-backed gossip crawler..."
docker compose up -d --force-recreate gossip

container_id="$(docker compose ps -q podping-listener || true)"
if [[ -n "${container_id}" ]]; then
    docker inspect "${container_id}" \
        --format 'podping-listener image={{.Config.Image}} image_id={{.Image}} created={{.Created}} source={{index .Config.Labels "org.opencontainers.image.source"}} revision={{index .Config.Labels "org.opencontainers.image.revision"}}'
fi

docker compose ps podping-listener gossip
REMOTE

echo ":: Done"

#!/usr/bin/env bash
#
# Build the MusicIndex live metadata relay locally as a static musl binary and
# deploy it to the VPS as the bundled Compose live-relay service.
#
# Mirrors deploy.sh: build locally, rsync the binary, build a tiny image on the
# VPS (~1s), recreate the service.
#
# Reserved live items (relay ADR 0001) persist in a SQLite state file at
# /var/lib/musicindex-live-relay inside the container. The image made here owns
# that directory as the relay user. The VPS must mount a named volume there,
# or every recreate deletes each reserved item and its broadcaster token. This
# script refuses to recreate the service when ADMIN_TOKEN is set and that
# volume is missing.
#
# Usage:
#   ./deploy-live-relay.sh
#
# Optional overrides:
#   VPS=user@example.org ./deploy-live-relay.sh
#   VPS_DIR=/srv/stophammer ./deploy-live-relay.sh
#   RELAY_SRC_DIR=~/src/Live-Metadata-Relay ./deploy-live-relay.sh
#   RELAY_IMAGE=musicindex-live-relay:test ./deploy-live-relay.sh
#   RELAY_SKIP_VERIFY=1 ./deploy-live-relay.sh
#   NODE_URL=https://node.example.org ./deploy-live-relay.sh
#
set -euo pipefail

LOCAL_DIR="$(cd "$(dirname "$0")" && pwd)"
# The repository holds no value of one deployment. A value in the environment
# has priority over the ignored file deploy.env.
VPS="${VPS:-}"
VPS_DIR="${VPS_DIR:-}"
NODE_URL="${NODE_URL:-}"
if [[ -f "${LOCAL_DIR}/deploy.env" ]]; then
    env_vps="${VPS}"
    env_vps_dir="${VPS_DIR}"
    env_node_url="${NODE_URL}"
    # shellcheck source=/dev/null
    source "${LOCAL_DIR}/deploy.env"
    VPS="${env_vps:-${VPS:-}}"
    VPS_DIR="${env_vps_dir:-${VPS_DIR:-}}"
    NODE_URL="${env_node_url:-${NODE_URL:-}}"
fi

RELAY_SRC_DIR="${RELAY_SRC_DIR:-$(dirname "${LOCAL_DIR}")/musicindex-live-relay}"
RELAY_IMAGE="${RELAY_IMAGE:-musicindex-live-relay:local}"
RELAY_REPO="${RELAY_REPO:-https://github.com/InTheMorning/Live-Metadata-Relay}"
PUBLIC_HEALTH_URL="${PUBLIC_HEALTH_URL-${NODE_URL:+${NODE_URL}/v1/liveitems/health}}"
MUSL_TARGET="x86_64-unknown-linux-musl"
RELAY_STATE_DIR="/var/lib/musicindex-live-relay"

usage() {
    cat >&2 <<'EOF'
usage: deploy-live-relay.sh [-h]

Build musicindex-live-relay from a local checkout as a static musl binary,
ship it to the VPS, build the Compose image there, set
STOPHAMMER_LIVE_RELAY_IMAGE in the remote .env, and recreate live-relay.

Environment overrides:
  VPS                SSH target
  VPS_DIR            remote Stophammer checkout
  RELAY_SRC_DIR      local Live-Metadata-Relay checkout
  RELAY_IMAGE        image tag to build and run
  NODE_URL           public address of the primary, also read from deploy.env
  PUBLIC_HEALTH_URL  URL checked after deploy, by default under NODE_URL; empty
                     or RELAY_SKIP_VERIFY=1 skips
  RELAY_SKIP_VERIFY  set to 1 to skip the post-deploy public health check

Options:
  -h, --help   show this help
EOF
}

for arg in "$@"; do
    case "$arg" in
        -h|--help) usage; exit 0 ;;
        *) usage; exit 1 ;;
    esac
done

if [[ -z "${VPS}" || -z "${VPS_DIR}" ]]; then
    echo >&2 "error: set VPS and VPS_DIR in deploy.env or in the environment."
    echo >&2 "       Copy deploy.env.example to deploy.env to start."
    exit 1
fi

if [[ ! -f "${RELAY_SRC_DIR}/Cargo.toml" ]]; then
    echo >&2 "error: no Live-Metadata-Relay checkout at ${RELAY_SRC_DIR}"
    echo >&2 "       clone ${RELAY_REPO} or set RELAY_SRC_DIR"
    exit 1
fi

if ! grep -q '^name = "musicindex-live-relay"' "${RELAY_SRC_DIR}/Cargo.toml"; then
    echo >&2 "error: ${RELAY_SRC_DIR} is not the musicindex-live-relay crate"
    exit 1
fi

# Record what is actually being shipped. The working tree is used as-is, so a
# dirty checkout is worth saying out loud rather than silently stamping a
# commit that does not match the binary.
source_commit="$(git -C "${RELAY_SRC_DIR}" rev-parse HEAD 2>/dev/null || echo unknown)"
source_dirty=""
if [[ -n "$(git -C "${RELAY_SRC_DIR}" status --porcelain 2>/dev/null)" ]]; then
    source_dirty=" (working tree dirty)"
fi

cat <<EOF
:: Relay source ${RELAY_SRC_DIR}
   commit ${source_commit}${source_dirty}

!! Recreating live-relay drops each ephemeral live item and its token.
!! A reserved live item survives in the state volume, but it has no snapshot
!! until its broadcaster publishes again. Display states and images clear.

EOF

echo ":: Building musicindex-live-relay (${MUSL_TARGET})"
cargo build --release --target "${MUSL_TARGET}" \
    --manifest-path "${RELAY_SRC_DIR}/Cargo.toml"

binary="${RELAY_SRC_DIR}/target/${MUSL_TARGET}/release/musicindex-live-relay"
if [[ ! -x "${binary}" ]]; then
    echo >&2 "error: expected binary not found: ${binary}"
    exit 1
fi

echo ":: Syncing binary + Dockerfile to VPS"
ssh "${VPS}" "mkdir -p /tmp/stophammer-live-relay-deploy"
rsync -z --info=progress2 \
    "${binary}" \
    "${LOCAL_DIR}/Dockerfile.deploy" \
    "${VPS}:/tmp/stophammer-live-relay-deploy/"

echo ":: Deploying on VPS"
ssh "${VPS}" \
    VPS_DIR="${VPS_DIR}" \
    RELAY_IMAGE="${RELAY_IMAGE}" \
    RELAY_STATE_DIR="${RELAY_STATE_DIR}" \
    RELAY_REPO="${RELAY_REPO}" \
    SOURCE_COMMIT="${source_commit}" \
    bash <<'REMOTE'
set -euo pipefail

if [[ ! -d "${VPS_DIR}" ]]; then
    echo >&2 "error: Stophammer directory not found: ${VPS_DIR}"
    exit 1
fi

cd "${VPS_DIR}"

if [[ ! -f packaging/env/live-relay.compose.env ]]; then
    if [[ -f packaging/env/live-relay.compose.env.example ]]; then
        echo "Seeding packaging/env/live-relay.compose.env from the example..."
        cp packaging/env/live-relay.compose.env.example \
           packaging/env/live-relay.compose.env
    else
        echo >&2 "error: packaging/env/live-relay.compose.env{,.example} missing on the VPS"
        echo >&2 "       run ./deploy.sh first, or rsync the repo, so Compose config is current"
        exit 1
    fi
fi

env_file=packaging/env/live-relay.compose.env
if grep -q '^ADMIN_TOKEN=.' "${env_file}"; then
    # Reserved items are on. Check the secret file mode and the state volume
    # before anything changes. The compose config holds the token, so it goes
    # only into grep and is never printed.
    env_mode="$(stat -c '%a' "${env_file}")"
    if [[ "${env_mode}" != "600" ]]; then
        echo >&2 "error: ${env_file} holds ADMIN_TOKEN and has mode ${env_mode}"
        echo >&2 "       run: chmod 600 ${VPS_DIR}/${env_file}"
        exit 1
    fi
    if ! docker compose config live-relay 2>/dev/null \
        | grep -q "target: ${RELAY_STATE_DIR}\$"; then
        echo >&2 "error: ADMIN_TOKEN is set, but live-relay mounts no volume at ${RELAY_STATE_DIR}"
        echo >&2 "       a recreate would delete every reserved live item and its token"
        echo >&2 "       add the volume in docker-compose.override.yml, then run this again"
        exit 1
    fi
    echo "Reserved live items are on. State volume at ${RELAY_STATE_DIR}."
else
    echo "ADMIN_TOKEN is not set. Reserved live items are off."
fi

echo "Building ${RELAY_IMAGE} from shipped binary..."
docker build \
    --target musicindex-live-relay \
    --label "org.opencontainers.image.source=${RELAY_REPO}" \
    --label "org.opencontainers.image.revision=${SOURCE_COMMIT}" \
    -t "${RELAY_IMAGE}" \
    -f /tmp/stophammer-live-relay-deploy/Dockerfile.deploy \
    /tmp/stophammer-live-relay-deploy
rm -rf /tmp/stophammer-live-relay-deploy

# The relay user must own the state directory. A new named volume takes the
# owner of the image directory at its first mount. Dockerfile.deploy is a
# tracked stophammer file, so this layer is added here.
echo "Adding the state directory ${RELAY_STATE_DIR} to ${RELAY_IMAGE}..."
docker build -t "${RELAY_IMAGE}" - <<DOCKERFILE
FROM ${RELAY_IMAGE}
USER root
RUN mkdir -p ${RELAY_STATE_DIR} && chown relay:relay ${RELAY_STATE_DIR} && chmod 0700 ${RELAY_STATE_DIR}
USER relay
DOCKERFILE

echo "Updating Stophammer Compose image override..."
touch .env
tmp_env="$(mktemp)"
awk -v image_line="STOPHAMMER_LIVE_RELAY_IMAGE=${RELAY_IMAGE}" '
    BEGIN { written = 0 }
    /^STOPHAMMER_LIVE_RELAY_IMAGE=/ {
        if (written == 0) { print image_line; written = 1 }
        next
    }
    { print }
    END { if (written == 0) print image_line }
' .env > "${tmp_env}"
mv "${tmp_env}" .env

echo "Recreating live-relay..."
docker compose up -d --force-recreate --no-build live-relay

container_id="$(docker compose ps -q live-relay || true)"
if [[ -n "${container_id}" ]]; then
    docker inspect "${container_id}" \
        --format 'live-relay image={{.Config.Image}} image_id={{.Image}} created={{.Created}} source={{index .Config.Labels "org.opencontainers.image.source"}} revision={{index .Config.Labels "org.opencontainers.image.revision"}}'
fi

echo "Waiting for the container to answer /health..."
for _ in $(seq 1 30); do
    if curl -fsS -o /dev/null http://127.0.0.1:8018/health; then
        echo "live-relay is healthy on 127.0.0.1:8018"
        break
    fi
    sleep 1
done

docker compose ps live-relay

# The startup line names the state file and the count of restored items. It
# holds no token.
docker compose logs --no-log-prefix --tail 50 live-relay 2>&1 \
    | grep -E 'reserved live items|relay stopped with an error' || true
REMOTE

if [[ "${RELAY_SKIP_VERIFY:-0}" != "1" && -n "${PUBLIC_HEALTH_URL}" ]]; then
    echo ":: Verifying public route ${PUBLIC_HEALTH_URL}"
    code="$(curl -sS -o /dev/null -w '%{http_code}' "${PUBLIC_HEALTH_URL}" || echo 000)"
    case "${code}" in
        200)
            echo "   HTTP 200 — nginx is reaching the relay"
            ;;
        502|503|504)
            echo >&2 "   HTTP ${code} — the container is up but nginx is not reaching it."
            echo >&2 "   Check that the nginx /v1/liveitems/* location proxies to 127.0.0.1:8018."
            exit 1
            ;;
        *)
            echo >&2 "   HTTP ${code} — unexpected; check the nginx config and relay logs."
            exit 1
            ;;
    esac
fi

echo ":: Done"

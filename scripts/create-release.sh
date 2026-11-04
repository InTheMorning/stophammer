#!/usr/bin/env bash
#
# Make a candidate or a release of the three Stophammer repositories
# (ADR 0063, ADR 0065 and ADR 0066).
#
# Usage:
#   scripts/create-release.sh VERSION [--yes]
#
# A VERSION with -rc.N is a candidate. The script does these steps:
#   1. It sets the version in Cargo.toml and Cargo.lock of each repository,
#      and commits only these two files. A repository that already gives the
#      version gets no commit.
#   2. It pushes main of each repository, and waits for CI on the stophammer
#      commit (ADR 0065 section 4).
#   3. It pushes the tag vVERSION in the parser, then the crawler, then
#      stophammer. The stophammer tag starts release.yml, and that workflow
#      reads the tag of the other two repositories.
#   4. It waits for release.yml on the tag.
#
# A VERSION with no hyphen promotes its newest candidate (ADR 0065 section 2).
# The script tags the commit of the candidate in each repository, and waits
# for release.yml on the tag. It makes no commit, and it does not push main.
#
# A second run with the same VERSION continues after the last completed step.
# The script does not deploy. ./deploy.sh --release VERSION --create runs this
# script, and then deploys.
#
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
# The tag of stophammer is last, because it starts the workflow.
REPOS=("${ROOT}/stophammer-parser" "${ROOT}/stophammer-crawler" "${ROOT}")

usage() {
    cat >&2 <<'USAGE'
usage: create-release.sh VERSION [--yes]

  VERSION  A candidate such as 0.18.1-rc.1, or a release such as 0.18.1. A
           release promotes the newest candidate of that version.
  --yes    Do not ask before the first change.

The script waits for GitHub Actions through the GitHub API. It uses `gh api`
when gh has a login. Otherwise it uses curl, with GITHUB_TOKEN when it is set.
With no token, it asks one time each minute, because GitHub permits 60 requests
in each hour with no token.
USAGE
}

die() {
    echo "create-release: $*" >&2
    exit 1
}

version=""
confirm=1
while [[ $# -gt 0 ]]; do
    case "$1" in
        --yes) confirm=0 ;;
        -h | --help)
            usage
            exit 0
            ;;
        -*)
            usage
            exit 1
            ;;
        *)
            [[ -z "${version}" ]] || { usage; exit 1; }
            version="$1"
            ;;
    esac
    shift
done
[[ -n "${version}" ]] || { usage; exit 1; }
[[ "${version}" =~ ^([0-9]+\.[0-9]+\.[0-9]+)(-rc\.[0-9]+)?$ ]] \
    || die "VERSION must be a version such as 0.18.1 or 0.18.1-rc.1, not ${version}."
base="${BASH_REMATCH[1]}"
candidate=0
[[ -n "${BASH_REMATCH[2]}" ]] && candidate=1
tag="v${version}"

for repo in "${REPOS[@]}"; do
    # A submodule has a .git file, not a directory, so only Git can answer.
    top="$(git -C "${repo}" rev-parse --show-toplevel 2>/dev/null || true)"
    [[ -n "${top}" && "${top}" == "$(cd "${repo}" && pwd -P)" ]] \
        || die "${repo} is not a Git repository. Clone it first."
done

github_repo="$(git -C "${ROOT}" config --get remote.origin.url | sed -E 's#^.*github\.com[:/]##; s#\.git$##')"
[[ "${github_repo}" == */* ]] || die "origin is not a GitHub repository."

if gh auth status >/dev/null 2>&1; then
    use_gh=1
    interval=30
else
    use_gh=0
    interval=$([[ -n "${GITHUB_TOKEN:-}" ]] && echo 30 || echo 60)
fi
interval="${CREATE_RELEASE_INTERVAL:-${interval}}"

# One GET request to the GitHub API. It prints the JSON body.
api() {
    if [[ "${use_gh}" == 1 ]]; then
        gh api "$1"
    else
        curl -fsSL -H 'Accept: application/vnd.github+json' \
            ${GITHUB_TOKEN:+-H "Authorization: Bearer ${GITHUB_TOKEN}"} \
            "https://api.github.com/$1"
    fi
}

# The status, the conclusion and the address of the newest run of workflow $1
# on commit $2 and ref $3. A candidate and its release have one commit, so only
# the ref tells their runs apart.
run_state() {
    api "repos/${github_repo}/actions/workflows/$1/runs?head_sha=$2&event=push&per_page=20" | REF="$3" python3 -c '
import json, os, sys
runs = [r for r in json.load(sys.stdin).get("workflow_runs") or [] if r.get("head_branch") == os.environ["REF"]]
run = runs[0] if runs else {}
print(run.get("status") or "none", run.get("conclusion") or "none", run.get("html_url") or "-")'
}

# Wait for workflow $1 on commit $2 and ref $3 for at most $4 minutes. Stop when
# the run does not pass. A failed request to the API counts as no result.
wait_for() {
    local workflow="$1" sha="$2" ref="$3" minutes="$4" last="" status conclusion url
    local end=$((SECONDS + minutes * 60))
    while ((SECONDS < end)); do
        read -r status conclusion url < <(run_state "${workflow}" "${sha}" "${ref}" || echo "none none -")
        if [[ "${status}" != "${last}" ]]; then
            echo "  ${workflow} on ${sha:0:7}: ${status} ${url}"
            last="${status}"
        fi
        if [[ "${status}" == completed ]]; then
            [[ "${conclusion}" == success ]] && return 0
            die "${workflow} on ${sha:0:7} ended with ${conclusion}: ${url}"
        fi
        sleep "${interval}"
    done
    die "${workflow} on ${sha:0:7} did not end within ${minutes} minutes. A commit that changes only documents starts no CI run (.github/workflows/ci.yml)."
}

# The version in the Cargo.toml of repository $1 at commit $2.
version_at() {
    git -C "$1" show "$2:Cargo.toml" | sed -nE 's/^version[[:space:]]*=[[:space:]]*"(.*)"$/\1/p' | head -n1
}

# The commit of tag $2 in repository $1, on this machine or on origin. It stops
# when the two disagree, because a tag never moves (ADR 0065).
tag_commit() {
    local repo="$1" name="$2" local_commit remote_commit
    local_commit="$(git -C "${repo}" rev-parse -q --verify "refs/tags/${name}^{commit}" || true)"
    remote_commit="$(git -C "${repo}" ls-remote --tags origin "refs/tags/${name}^{}" | cut -f1)"
    if [[ -n "${local_commit}" && -n "${remote_commit}" && "${local_commit}" != "${remote_commit}" ]]; then
        die "the tag ${name} of $(basename "${repo}") is on different commits here and on origin. Use a new version."
    fi
    echo "${local_commit:-${remote_commit}}"
}

for repo in "${REPOS[@]}"; do
    git -C "${repo}" fetch --quiet --tags origin main \
        || die "the fetch of $(basename "${repo}") failed. Examine the tags of that repository."
done

steps=()
declare -A target_commit=()

if [[ "${candidate}" == 1 ]]; then
    released="$(tag_commit "${ROOT}" "v${base}")"
    [[ -z "${released}" ]] || die "the release v${base} exists. A candidate needs a higher version (ADR 0066)."
    for repo in "${REPOS[@]}"; do
        name="$(basename "${repo}")"
        tagged="$(tag_commit "${repo}" "${tag}")"
        if [[ -n "${tagged}" ]]; then
            git -C "${repo}" merge-base --is-ancestor "${tagged}" origin/main \
                || die "the tag ${tag} of ${name} is on a commit that is not on origin/main. Push main first."
            [[ "$(version_at "${repo}" "${tagged}")" == "${base}" ]] \
                || die "the tag ${tag} of ${name} is on a commit with a different version. Use a new version."
            target_commit["${repo}"]="${tagged}"
            continue
        fi
        [[ "$(git -C "${repo}" symbolic-ref --short HEAD)" == main ]] || die "check out main in ${name} first."
        git -C "${repo}" merge-base --is-ancestor origin/main HEAD \
            || die "origin/main of ${name} has commits that main does not have. Pull first."
        count="$(git -C "${repo}" rev-list --count origin/main..HEAD)"
        current="$(version_at "${repo}" HEAD)"
        if [[ "${current}" != "${base}" ]]; then
            git -C "${repo}" diff --quiet HEAD -- Cargo.toml Cargo.lock \
                || die "Cargo.toml or Cargo.lock of ${name} has changes that are not committed."
            steps+=("${name}: set the version ${current} -> ${base}, and commit Cargo.toml and Cargo.lock.")
            count=$((count + 1))
        fi
        steps+=("${name}: push main (${count} new commits).")
    done
    steps+=("Wait for CI on the stophammer commit.")
else
    # The newest candidate of this version, by the number after -rc.
    from_tag="$(git -C "${ROOT}" tag --list "${tag}-rc.*" --sort=-version:refname | head -n1)"
    [[ -n "${from_tag}" ]] || die "no candidate ${tag}-rc.N exists. A release needs a candidate (ADR 0065 section 3)."
    for repo in "${REPOS[@]}"; do
        name="$(basename "${repo}")"
        commit="$(git -C "${repo}" rev-parse -q --verify "refs/tags/${from_tag}^{commit}" || true)"
        [[ -n "${commit}" ]] || die "${name} has no tag ${from_tag}. The three repositories release together (ADR 0066 section 3)."
        tagged="$(tag_commit "${repo}" "${tag}")"
        if [[ -n "${tagged}" && "${tagged}" != "${commit}" ]]; then
            die "the tag ${tag} of ${name} is not on the commit of ${from_tag} (ADR 0065 section 3). Use a new version."
        fi
        target_commit["${repo}"]="${commit}"
    done
    steps+=("Promote the candidate ${from_tag}.")
fi

for repo in "${REPOS[@]}"; do
    [[ -z "$(git -C "${repo}" ls-remote --tags origin "refs/tags/${tag}")" ]] \
        && steps+=("$(basename "${repo}"): push the tag ${tag}.")
done
steps+=("Wait for release.yml on the tag ${tag}.")

echo "${version} of ${github_repo}:"
printf '  - %s\n' "${steps[@]}"
if [[ "${confirm}" == 1 ]]; then
    read -r -p "Continue? [y/N] " answer
    [[ "${answer}" == [yY]* ]] || die "stopped. Nothing was changed."
fi

if [[ "${candidate}" == 1 && -z "${target_commit["${ROOT}"]:-}" ]]; then
    for repo in "${REPOS[@]}"; do
        [[ -z "${target_commit["${repo}"]:-}" ]] || continue
        name="$(basename "${repo}")"
        if [[ "$(version_at "${repo}" HEAD)" != "${base}" ]]; then
            echo "Set the version of ${name} to ${base}."
            sed -i -E "0,/^version([[:space:]]*)=.*/s//version\1= \"${base}\"/" "${repo}/Cargo.toml"
            # The crawler reads the parser by its path, so its lock file holds
            # the version of the parser too.
            (cd "${repo}" && cargo update --workspace --offline --quiet)
            git -C "${repo}" add Cargo.toml Cargo.lock
            if [[ "${repo}" == "${ROOT}" ]]; then
                reference="ADR 0066"
            else
                reference="stophammer ADR 0066"
            fi
            git -C "${repo}" commit --quiet -m "Set version ${base} for the next release candidate (${reference})" \
                -m "The tag ${tag} builds the candidate. The three repositories get one version."
        fi
        echo "Push main of ${name}."
        git -C "${repo}" push --quiet origin main
        target_commit["${repo}"]="$(git -C "${repo}" rev-parse HEAD)"
    done
    echo "Wait for CI (ADR 0065 section 4)."
    wait_for ci.yml "${target_commit["${ROOT}"]}" main 45
elif [[ "${candidate}" == 1 ]]; then
    # The stophammer tag exists, so its CI run passed. A repository with no tag
    # is impossible here, because stophammer gets its tag last. Stop if it occurs.
    for repo in "${REPOS[@]}"; do
        [[ -n "${target_commit["${repo}"]:-}" ]] || die "$(basename "${repo}") has no tag ${tag}, and stophammer has one. Use a new version."
    done
fi

for repo in "${REPOS[@]}"; do
    if ! git -C "${repo}" rev-parse -q --verify "refs/tags/${tag}" >/dev/null; then
        if [[ -n "$(git -C "${repo}" ls-remote --tags origin "refs/tags/${tag}")" ]]; then
            git -C "${repo}" fetch --quiet origin "refs/tags/${tag}:refs/tags/${tag}"
        else
            git -C "${repo}" tag -a "${tag}" "${target_commit["${repo}"]}" -m "${version}"
        fi
    fi
    if [[ -z "$(git -C "${repo}" ls-remote --tags origin "refs/tags/${tag}")" ]]; then
        echo "Push the tag ${tag} of $(basename "${repo}")."
        git -C "${repo}" push --quiet origin "refs/tags/${tag}"
    fi
done

echo "Wait for release.yml."
if [[ "${candidate}" == 1 ]]; then
    wait_for release.yml "${target_commit["${ROOT}"]}" "${tag}" 180
else
    wait_for release.yml "${target_commit["${ROOT}"]}" "${tag}" 30
fi

echo "Done. ${tag} has its release page and its images:"
echo "  https://github.com/${github_repo}/releases/tag/${tag}"
if [[ "${candidate}" == 0 ]]; then
    echo "Set the notes on that page. .idea/releases/${version}/ holds them."
fi

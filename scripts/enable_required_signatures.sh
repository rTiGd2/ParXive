#!/usr/bin/env bash
set -euo pipefail

# Usage: scripts/enable_required_signatures.sh [OWNER] [REPO] [BRANCH]

OWNER=${1:-}
REPO=${2:-}
BR=${3:-main}

if [[ -z "${OWNER}" || -z "${REPO}" ]]; then
  url=$(git remote get-url origin 2>/dev/null || true)
  if [[ -z "$url" ]]; then
    echo "Error: provide OWNER REPO or run in a git repo with an 'origin' remote." >&2
    exit 1
  fi
  owner_repo=$(sed -E 's#.*github.com[:/ ]([^/]+/[^/.]+)(\\.git)?$#\1#' <<<"$url")
  OWNER="${owner_repo%/*}"
  REPO="${owner_repo#*/}"
fi

gh api -X POST repos/$OWNER/$REPO/branches/$BR/protection/required_signatures >/dev/null
echo "Required signed commits enabled on $OWNER/$REPO:$BR"


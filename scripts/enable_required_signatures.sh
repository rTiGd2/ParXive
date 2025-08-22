#!/usr/bin/env bash
set -euo pipefail

# Enable required signed commits on a protected branch.
# Usage: ./scripts/enable_required_signatures.sh [OWNER] [REPO] [BRANCH]

OWNER=${1:-rTiGd2}
REPO=${2:-ParXive}
BR=${3:-main}

echo "Enabling required signed commits on $OWNER/$REPO:$BR"

# GitHub API: POST to enable required signatures for a protected branch
gh api \
  -X POST \
  -H "Accept: application/vnd.github+json" \
  repos/"$OWNER"/"$REPO"/branches/"$BR"/protection/required_signatures \
  >/dev/null

echo "Required signed commits enabled."


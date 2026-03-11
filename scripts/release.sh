#!/bin/bash
set -euo pipefail

TYPE="${1:-}"
VERSION="${2:-}"

if [ -z "$TYPE" ] || [ -z "$VERSION" ]; then
  echo "Usage: ./scripts/release.sh <type> <version>"
  echo ""
  echo "  ./scripts/release.sh app   0.5.0   # Release app + CLI (triggers release-app.yml)"
  echo "  ./scripts/release.sh agent 0.5.0   # Release agent (triggers release-agent.yml)"
  exit 1
fi

CURRENT_BRANCH=$(git branch --show-current)

case "$TYPE" in
  app)
    TAG="app-v${VERSION}"

    # Bump versions
    sed -i '' "s/\"version\": \"[^\"]*\"/\"version\": \"${VERSION}\"/" package.json
    sed -i '' "s/\"version\": \"[^\"]*\"/\"version\": \"${VERSION}\"/" src-tauri/tauri.conf.json
    sed -i '' "0,/^version = \"[^\"]*\"/s//version = \"${VERSION}\"/" src-tauri/Cargo.toml

    # Commit version bump on main (private)
    git add package.json src-tauri/tauri.conf.json src-tauri/Cargo.toml
    if ! git diff --cached --quiet; then
      git commit -m "Bump app version to ${VERSION}"
      git push origin main
    fi
    ;;
  agent)
    TAG="agent-v${VERSION}"

    # Bump agent version
    sed -i '' "0,/^version = \"[^\"]*\"/s//version = \"${VERSION}\"/" crates/berth-agent/Cargo.toml

    # Commit version bump on main (private)
    git add crates/berth-agent/Cargo.toml
    if ! git diff --cached --quiet; then
      git commit -m "Bump agent version to ${VERSION}"
      git push origin main
    fi
    ;;
  *)
    echo "Unknown type: $TYPE (use 'app' or 'agent')"
    exit 1
    ;;
esac

echo "Releasing ${TAG}..."

# Create orphan branch with all files
git checkout --orphan release
git add -A
git commit -m "v${VERSION}"
git tag "${TAG}"

# Push to public
git push public release:main --force
git push public "${TAG}"

# Return to main
git checkout "${CURRENT_BRANCH}"
git branch -D release

echo "Released ${TAG} to public repo"
echo "CI will build and publish automatically"

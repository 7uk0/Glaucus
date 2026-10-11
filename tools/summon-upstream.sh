#!/usr/bin/env bash
# summon-upstream.sh — fetch the upstream content Caput Mortuum runs on.
#
# Caput Mortuum is an engine + systems rewrite. The *content* (prototypes,
# maps, sprites, sounds, locale) is read directly from upstream checkouts and
# is NOT vendored into this repository: it is large and carries its own
# licenses (mostly CC-BY-SA 3.0; see each meta.json / attributions.yml).
#
# Layout produced:
#   upstream/frontier/        new-frontiers-14/frontier-station-14 (content fork)
#   upstream/RobustToolbox/   space-wizards/RobustToolbox at Frontier's pinned commit
#   upstream/ss14/            space-wizards/space-station-14 (reference only; --with-ss14)
#
# Commits are pinned in upstream.lock so everyone sees the same content.
# Pass --latest to ignore the pins and take each branch head instead.

set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
DEST="${CAPUT_UPSTREAM:-$ROOT/upstream}"
LOCK="$ROOT/upstream.lock"

LATEST=0
WITH_SS14=0
for arg in "$@"; do
  case "$arg" in
    --latest) LATEST=1 ;;
    --with-ss14) WITH_SS14=1 ;;
    -h|--help)
      sed -n '2,20p' "$0"; exit 0 ;;
    *) echo "unknown argument: $arg" >&2; exit 2 ;;
  esac
done

pin() { grep -E "^$1=" "$LOCK" | cut -d= -f2; }

# fetch <dir> <url> <commit-or-branch>
fetch() {
  local dir="$1" url="$2" rev="$3"
  if [[ ! -d "$dir/.git" ]]; then
    mkdir -p "$dir"
    git -C "$dir" init -q
    git -C "$dir" remote add origin "$url"
  fi
  echo ">> $url @ $rev"
  git -C "$dir" fetch -q --depth 1 origin "$rev"
  git -C "$dir" checkout -q --force FETCH_HEAD
  echo "   now at $(git -C "$dir" rev-parse HEAD)"
}

mkdir -p "$DEST"

if [[ $LATEST -eq 1 ]]; then
  fetch "$DEST/frontier" https://github.com/new-frontiers-14/frontier-station-14.git master
  # Follow whatever engine commit that Frontier head pins.
  ENGINE_REV="$(git -C "$DEST/frontier" ls-tree HEAD RobustToolbox | awk '{print $3}')"
  fetch "$DEST/RobustToolbox" https://github.com/space-wizards/RobustToolbox.git "$ENGINE_REV"
  [[ $WITH_SS14 -eq 1 ]] && fetch "$DEST/ss14" https://github.com/space-wizards/space-station-14.git master
else
  fetch "$DEST/frontier" https://github.com/new-frontiers-14/frontier-station-14.git "$(pin frontier)"
  fetch "$DEST/RobustToolbox" https://github.com/space-wizards/RobustToolbox.git "$(pin robusttoolbox)"
  [[ $WITH_SS14 -eq 1 ]] && fetch "$DEST/ss14" https://github.com/space-wizards/space-station-14.git "$(pin ss14)"
fi

echo
echo "Upstream ready in $DEST"
echo "Run the game with:  cargo run --release -p caput"

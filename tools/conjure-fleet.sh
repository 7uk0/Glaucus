#!/bin/bash
# Conjure the fleet: render every ship that has an official render in
# reference/images/frontier-ships/ with `caput-mortuum --render-map`, two at
# a time, into OUT (default: target/fleet). Skips ships already rendered.
#
#   tools/conjure-fleet.sh [OUT]
#   python3 -I tools/catoptromancy.py target/fleet reference/images/frontier-ships
#
# Do not rebuild the client while this runs: new launches would pick up the
# new binary halfway through. Needs xvfb + lavapipe (see docs/akasha/environment.md).
set -u
cd "$(dirname "$0")/.."
OUT=${1:-target/fleet}
mkdir -p "$OUT"
python3 -I -c "
import json
d=json.load(open('reference/images/frontier-ships/index.json'))
for f in d['files']: print(f['file'][:-4], '/'+f['map'])
" > "$OUT/list.txt"
render() {
  name=$1; map=$2
  [ -f "$OUT/$name.png" ] && return
  timeout 300 xvfb-run -a -s "-screen 0 2600x2600x24" env WGPU_BACKEND=vulkan \
    target/debug/caput-mortuum --render-map "$map" --screenshot "$OUT/$name.png" --frames 40 > "$OUT/$name.log" 2>&1
  echo "$name $?"
}
export -f render; export OUT
xargs -P 2 -L 1 bash -c 'render "$0" "$1"' < "$OUT/list.txt"
echo DONE

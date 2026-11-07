#!/bin/sh
# Copy the license files of every Rust crate linked into the build into
# licenses/ (or $LICENSES_DIR).
#
#   scripts/collect-licenses.sh [target-triple] [cargo feature flags...]
#
# Defaults: the host target and the default features (the Node-API addon).
set -eu
ROOT=$(cd "$(dirname "$0")/.." && pwd)
cd "$ROOT"
TARGET=${1:-$(rustc -vV | sed -n 's/^host: //p')}
[ $# -gt 0 ] && shift
L=${LICENSES_DIR:-licenses}
rm -rf "$L"
mkdir -p "$L"
cargo metadata --format-version 1 --filter-platform "$TARGET" "$@" \
  | python3 -c '
import json, sys
m = json.load(sys.stdin)
pkgs = {p["id"]: p for p in m["packages"]}
# Normal (non-build, non-dev) dependencies reachable from the root package.
nodes = {n["id"]: n for n in m["resolve"]["nodes"]}
seen, todo = set(), [m["resolve"]["root"]]
while todo:
    i = todo.pop()
    if i in seen: continue
    seen.add(i)
    for d in nodes[i]["deps"]:
        if any(k["kind"] is None for k in d["dep_kinds"]): todo.append(d["pkg"])
for i in sorted(seen):
    p = pkgs[i]
    if i == m["resolve"]["root"]: continue
    print(p["name"], p["version"], p["license"] or "", p["manifest_path"].rsplit("/", 1)[0], p.get("repository") or "", sep="\t")
' > "$L/crates.tsv"
while IFS="$(printf '\t')" read -r name version license dir repo; do
  dest="$L/$name-$version"
  mkdir -p "$dest"
  find "$dir" -maxdepth 1 -type f \( -iname 'LICENSE*' -o -iname 'COPYING*' -o -iname 'NOTICE*' -o -iname 'COPYRIGHT*' \) -exec cp {} "$dest/" \;
  if [ -z "$(ls "$dest")" ]; then
    # Some crates (napi-rs) do not ship their license file in the package.
    printf '%s %s is licensed under %s.\nThe crate package has no license file; see %s\n' \
      "$name" "$version" "$license" "$repo" > "$dest/LICENSE-NOTE.txt"
  fi
  printf '%s %s: %s\n' "$name" "$version" "$license" >> "$L/SUMMARY.txt"
done < "$L/crates.tsv"
rm "$L/crates.tsv"
echo "collected $(ls "$L" | wc -l) entries in $L"

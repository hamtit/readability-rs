#!/bin/sh
# Copy the license files of every Rust crate linked into the addon into licenses/.
set -eu
ROOT=$(cd "$(dirname "$0")/.." && pwd)
cd "$ROOT"
rm -rf licenses
mkdir -p licenses
cargo metadata --format-version 1 --filter-platform "$(rustc -vV | sed -n 's/^host: //p')" \
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
' > licenses/crates.tsv
while IFS="$(printf '\t')" read -r name version license dir repo; do
  dest="licenses/$name-$version"
  mkdir -p "$dest"
  find "$dir" -maxdepth 1 -type f \( -iname 'LICENSE*' -o -iname 'COPYING*' -o -iname 'NOTICE*' -o -iname 'COPYRIGHT*' \) -exec cp {} "$dest/" \;
  if [ -z "$(ls "$dest")" ]; then
    # Some crates (napi-rs) do not ship their license file in the package.
    printf '%s %s is licensed under %s.\nThe crate package has no license file; see %s\n' \
      "$name" "$version" "$license" "$repo" > "$dest/LICENSE-NOTE.txt"
  fi
  printf '%s %s: %s\n' "$name" "$version" "$license" >> licenses/SUMMARY.txt
done < licenses/crates.tsv
rm licenses/crates.tsv
echo "collected $(ls licenses | wc -l) entries"

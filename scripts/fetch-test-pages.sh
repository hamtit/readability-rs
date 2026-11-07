#!/bin/sh
# Fetch the test pages of mozilla/readability (at the commit this port
# follows) into tests/test-pages/. They are not committed here (about 30 MB).
set -eu
ROOT=$(cd "$(dirname "$0")/.." && pwd)
COMMIT=$(cat "$ROOT/UPSTREAM_COMMIT")
DEST="$ROOT/tests/test-pages"
if [ -f "$DEST/.commit" ] && [ "$(cat "$DEST/.commit")" = "$COMMIT" ]; then
  echo "test pages already at $COMMIT"
  exit 0
fi
TMP=$(mktemp -d)
trap 'rm -rf "$TMP"' EXIT
git -C "$TMP" init -q
git -C "$TMP" remote add origin https://github.com/mozilla/readability.git
git -C "$TMP" fetch -q --depth 1 origin "$COMMIT"
git -C "$TMP" checkout -q FETCH_HEAD -- test/test-pages
rm -rf "$DEST"
mv "$TMP/test/test-pages" "$DEST"
echo "$COMMIT" > "$DEST/.commit"
echo "fetched $(ls "$DEST" | wc -l) test pages into tests/test-pages"

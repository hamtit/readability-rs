#!/bin/sh
# Build the WebAssembly module (wasm32-unknown-unknown) into <out-dir>:
#   <out-dir>/dist/readability.wasm and <out-dir>/licenses/
# Needs: rustup target add wasm32-unknown-unknown. wasm-opt (binaryen) is
# used when it is on PATH.
set -eu
ROOT=$(cd "$(dirname "$0")/.." && pwd)
OUT=$(mkdir -p "${1:-out/wasm}" && cd "${1:-out/wasm}" && pwd)
cd "$ROOT"
cargo build --release --lib --target wasm32-unknown-unknown --no-default-features --features wasm
mkdir -p "$OUT/dist"
if command -v wasm-opt > /dev/null; then
  wasm-opt -O3 --enable-bulk-memory --enable-nontrapping-float-to-int --enable-sign-ext \
    target/wasm32-unknown-unknown/release/readability_rs.wasm -o "$OUT/dist/readability.wasm"
else
  cp target/wasm32-unknown-unknown/release/readability_rs.wasm "$OUT/dist/readability.wasm"
fi
LICENSES_DIR="$OUT/licenses" ./scripts/collect-licenses.sh wasm32-unknown-unknown --no-default-features --features wasm
echo "built $OUT/dist/readability.wasm ($(wc -c < "$OUT/dist/readability.wasm") bytes)"

#!/bin/sh
# Build the release addon for the host and place it next to index.js.
set -eu
ROOT=$(cd "$(dirname "$0")/.." && pwd)
cd "$ROOT"
cargo build --release "$@"
ARCH=$(uname -m)
case "$ARCH" in x86_64) ARCH=x64 ;; aarch64) ARCH=arm64 ;; esac
LIBC=gnu
if ldd --version 2>&1 | grep -qi musl; then LIBC=musl; fi
cp target/release/libreadability_rs.so "readability-rs.linux-$ARCH-$LIBC.node"
echo "built readability-rs.linux-$ARCH-$LIBC.node"

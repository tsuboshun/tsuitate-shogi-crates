#!/usr/bin/env sh
set -eu

ROOT="$(CDPATH= cd -- "$(dirname -- "$0")/.." && pwd)"

if ! rustup target list --installed | grep -qx 'aarch64-unknown-linux-gnu'; then
  echo "missing Rust target: run 'rustup target add aarch64-unknown-linux-gnu'" >&2
  exit 1
fi

if ! command -v zig >/dev/null 2>&1 && ! python -m ziglang version >/dev/null 2>&1; then
  echo "missing zig: run 'python -m pip install ziglang'" >&2
  exit 1
fi

export CARGO_ZIGBUILD_CACHE_DIR="${CARGO_ZIGBUILD_CACHE_DIR:-$ROOT/target/cargo-zigbuild-cache}"
export ZIG_GLOBAL_CACHE_DIR="${ZIG_GLOBAL_CACHE_DIR:-$ROOT/target/zig-global-cache}"
export ZIG_LOCAL_CACHE_DIR="${ZIG_LOCAL_CACHE_DIR:-$ROOT/target/zig-local-cache}"

cd "$ROOT/tsuitate_bindings"

exec maturin build \
  --release \
  --target aarch64-unknown-linux-gnu \
  --zig \
  --out "$ROOT/target/wheels"

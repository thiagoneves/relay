#!/usr/bin/env bash
# Build relay from this checkout and set the machine up, as a release
# install would: binary in ~/.local/bin, on PATH, hooks in every harness
# found. Run again after pulling to update.
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
cargo build --release --locked --quiet --manifest-path "$ROOT/Cargo.toml"
"$ROOT/target/release/relay" setup

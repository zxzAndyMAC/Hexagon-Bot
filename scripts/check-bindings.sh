#!/usr/bin/env bash
# reliability 01 / ADR 0054: compare generation with the working tree, not HEAD;
# valid DTO edits may be uncommitted, but stale or missing exports must fail.
set -euo pipefail
cd "$(dirname "$0")/.."
bindings_snapshot=$(mktemp -d)
trap 'rm -rf "$bindings_snapshot"' EXIT
cp -R ui/src/gen "$bindings_snapshot/gen"
cargo test --workspace export_bindings
diff -ru "$bindings_snapshot/gen" ui/src/gen

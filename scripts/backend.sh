#!/usr/bin/env bash
# Build the lib + builtins, then run the example backend.
# Usage:
#   ./scripts/backend.sh                  # boot on the default port (3030)
#   ODYSSEY_ADDR=127.0.0.1:4040 ./scripts/backend.sh
#   ODYSSEY_NO_FRONTEND=1 ./scripts/backend.sh   # skip serving the React app
set -euo pipefail

# Resolve repo root from this script's location so the script
# works regardless of the cwd it's invoked from. Splitting
# `cd` from the variable assignment keeps `cd`'s non-zero
# exit visible (`set -e` would otherwise mask it under the
# successful `pwd`).
SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "${SCRIPT_DIR}/.." && pwd)"
cd "${REPO_ROOT}"

# 1. Build the lib so the example sees fresh rmeta, then
#    build the example workspace. `cargo build` at the
#    workspace root produces the bin in `example/target/`,
#    not per-crate — that's where cargo's `target-dir`
#    default lives for a multi-member workspace.
(cd "${REPO_ROOT}/crate" && cargo build --quiet)
(cd "${REPO_ROOT}/example" && cargo build --quiet)

# 2. Run the example binary.
exec "${REPO_ROOT}/example/target/debug/odyssey-example-basic" "$@"

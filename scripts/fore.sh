#!/usr/bin/env bash
# Build + serve the React app.
# Usage:
#   ./scripts/fore.sh build     # type-check + bundle once
#   ./scripts/fore.sh dev       # Vite dev server with HMR
#   ./scripts/fore.sh preview   # serve the built bundle locally
#   ./scripts/fore.sh test      # jsdom smoke test (built bundle)
set -euo pipefail

# Resolve repo root from this script's location so the
# script works regardless of the cwd it's invoked from.
SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "${SCRIPT_DIR}/.." && pwd)"
cd "${REPO_ROOT}/example/fore"

ACTION="${1:-build}"
shift || true

case "${ACTION}" in
build)
    pnpm build "$@"
    ;;
dev)
    pnpm dev "$@"
    ;;
preview)
    pnpm preview "$@"
    ;;
test)
    node test/frontend.test.mjs "$@"
    ;;
*)
    echo "unknown action: ${ACTION}" >&2
    echo "usage: $0 {build|dev|preview|test} [args...]" >&2
    exit 2
    ;;
esac

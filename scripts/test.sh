#!/usr/bin/env bash
# Run every test in every workspace. Two Rust workspaces
# (`crate/` for the lib, `example/` for the binaries) plus
# the frontend's jsdom smoke test against a live API.
# Usage:
#   ./scripts/test.sh          # all three
#   ./scripts/test.sh lib      # crate/odyssey only
#   ./scripts/test.sh example  # example/ only (the big smoke test)
#   ./scripts/test.sh frontend # the jsdom test
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "${SCRIPT_DIR}/.." && pwd)"
SCOPE="${1:-all}"
shift || true

run_lib() {
    echo "=== crate/odyssey ==="
    (cd "${REPO_ROOT}/crate" && cargo test --quiet "$@")
}

run_example() {
    echo "=== example/backend ==="
    # Build the React bundle first; the smoke test asserts on it.
    "${REPO_ROOT}/scripts/frontend.sh" build
    (cd "${REPO_ROOT}/example" && cargo test --quiet "$@")
}

run_frontend() {
    echo "=== example/frontend (jsdom) ==="
    "${REPO_ROOT}/scripts/frontend.sh" build
    "${REPO_ROOT}/scripts/frontend.sh" test "$@"
}

case "${SCOPE}" in
    lib)     run_lib     "$@" ;;
    example) run_example "$@" ;;
    frontend) run_frontend "$@" ;;
    all)
        run_lib     "$@"
        run_example "$@"
        run_frontend "$@"
        ;;
    *)
        echo "unknown scope: ${SCOPE}" >&2
        echo "usage: $0 {all|lib|example|frontend} [args...]" >&2
        exit 2
        ;;
esac

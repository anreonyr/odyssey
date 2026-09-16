#!/usr/bin/env bash
# Run every test in every workspace. The lib lives in
# `crate/` (single workspace member) and the example app
# lives in `example/back/` (single Rust crate holding the
# builtin lib + the binary) plus `example/fore/` (pnpm /
# React). The frontend's jsdom smoke test runs against a
# live API.
# Usage:
#   ./scripts/test.sh        # all three
#   ./scripts/test.sh lib    # crate/odyssey only
#   ./scripts/test.sh back   # example/back only
#   ./scripts/test.sh fore   # the jsdom test
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "${SCRIPT_DIR}/.." && pwd)"
SCOPE="${1:-all}"
shift || true

run_lib() {
    echo "=== crate/odyssey ==="
    (cd "${REPO_ROOT}/crate" && cargo test --quiet "$@")
}

run_back() {
    echo "=== example/back ==="
    # Build the React bundle first; the smoke test asserts on it.
    "${REPO_ROOT}/scripts/fore.sh" build
    (cd "${REPO_ROOT}/example" && cargo test --quiet --manifest-path back/Cargo.toml "$@")
}

run_fore() {
    echo "=== example/fore (jsdom) ==="
    "${REPO_ROOT}/scripts/fore.sh" build
    "${REPO_ROOT}/scripts/fore.sh" test "$@"
}

case "${SCOPE}" in
lib)  run_lib  "$@" ;;
back) run_back "$@" ;;
fore) run_fore "$@" ;;
all)
    run_lib  "$@"
    run_back "$@"
    run_fore "$@"
    ;;
*)
    echo "unknown scope: ${SCOPE}" >&2
    echo "usage: $0 {all|lib|back|fore} [args...]" >&2
    exit 2
    ;;
esac

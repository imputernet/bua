#!/bin/bash
set -e

# Bua Build Script
# Usage: ./build.sh [--skip-jsc] [cargo build args...]

SKIP_JSC=0
CARGO_ARGS=()

for arg in "$@"; do
    if [ "$arg" == "--skip-jsc" ]; then
        SKIP_JSC=1
    else
        CARGO_ARGS+=("$arg")
    fi
done

if [ "$SKIP_JSC" -eq 1 ] || [ "$BUA_SKIP_JSC" == "1" ]; then
    echo "Building in STUB mode (no JavaScriptCore required)..."
    export BUA_SKIP_JSC=1
    cargo build --all "${CARGO_ARGS[@]}"
else
    echo "Building with full JavaScriptCore support..."
    cargo build --all "${CARGO_ARGS[@]}"
fi

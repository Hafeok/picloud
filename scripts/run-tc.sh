#!/usr/bin/env bash
# Runner script for product verify — invokes picloud-test with the correct arguments.
# Usage: scripts/run-tc.sh <scenario-name>
#
# Builds picloud-test if needed, then runs the specified scenario.
# Uses the local cluster.toml config. Tests that need a live cluster
# will SKIP gracefully (exit 0) when the cluster is unreachable.
set -euo pipefail

SCENARIO="${1:?Usage: run-tc.sh <scenario-name>}"
ROOT="$(cd "$(dirname "$0")/.." && pwd)"
CONFIG="$ROOT/crates/picloud-test/cluster.toml"
BINARY="$ROOT/target/debug/picloud-test"

# Build if binary doesn't exist or is older than source
if [ ! -f "$BINARY" ]; then
    cargo build -p picloud-test --quiet 2>/dev/null
fi

exec "$BINARY" run \
    --scenario "$SCENARIO" \
    --config "$CONFIG" \
    --workspace-root "$ROOT"

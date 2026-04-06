#!/usr/bin/env bash
# logs.sh — tail picloud logs across all Pi nodes simultaneously
#
# Usage:
#   ./deploy/logs.sh           # tail all nodes
#   ./deploy/logs.sh pi-01     # tail one node
#   ./deploy/logs.sh --errors  # errors only

set -euo pipefail

NODES=(pi-01 pi-02 pi-03 pi-04 pi-05)
REMOTE_USER="ubuntu"
SERVICE_NAME="picloud"

# Colour per node so output is distinguishable
COLOURS=('\033[0;34m' '\033[0;32m' '\033[0;33m' '\033[0;35m' '\033[0;36m')
NC='\033[0m'

ERRORS_ONLY=false
TARGET_NODES=("${NODES[@]}")

for arg in "$@"; do
  case $arg in
    --errors) ERRORS_ONLY=true ;;
    pi-*)     TARGET_NODES=("$arg") ;;
  esac
done

GREP_FILTER=""
$ERRORS_ONLY && GREP_FILTER="ERROR\|error\|WARN\|warn\|panic"

echo "Tailing logs from: ${TARGET_NODES[*]}"
echo "Press Ctrl+C to stop"
echo "─────────────────────────────────────────────────"

# Launch a background tail for each node, prefix with node name + colour
PIDS=()
for i in "${!TARGET_NODES[@]}"; do
  node="${TARGET_NODES[$i]}"
  colour="${COLOURS[$i % ${#COLOURS[@]}]}"
  label=$(printf "%-8s" "$node")

  if $ERRORS_ONLY; then
    ssh "$REMOTE_USER@$node" \
      "journalctl -fu $SERVICE_NAME --output=cat" 2>/dev/null | \
      grep --line-buffered -E "$GREP_FILTER" | \
      sed "s/^/${colour}[${label}]${NC} /" &
  else
    ssh "$REMOTE_USER@$node" \
      "journalctl -fu $SERVICE_NAME --output=cat" 2>/dev/null | \
      sed "s/^/${colour}[${label}]${NC} /" &
  fi

  PIDS+=($!)
done

# Clean up background jobs on Ctrl+C
trap 'kill "${PIDS[@]}" 2>/dev/null; exit 0' INT TERM

wait

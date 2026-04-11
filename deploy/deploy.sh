#!/usr/bin/env bash
# deploy.sh — build and deploy picloud-server to all Pi nodes
#
# Usage:
#   ./deploy/deploy.sh              # deploy to all nodes
#   ./deploy/deploy.sh pi-01        # deploy to one node
#   ./deploy/deploy.sh --check      # build only, no deploy
#
# Prerequisites:
#   cargo install cross
#   rustup target add aarch64-unknown-linux-gnu
#   SSH config set up for each node (see deploy/ssh-config.example)

set -euo pipefail

# ─── Configuration ────────────────────────────────────────────────────────────

NODES=(192.168.88.22 192.168.88.21 192.168.88.20)
REMOTE_USER="admin"
REMOTE_DIR="/usr/local/bin"
BINARY="picloud-server"
TARGET="aarch64-unknown-linux-gnu"
PROFILE="release"
SERVICE_NAME="picloud"

# ─── Colours ──────────────────────────────────────────────────────────────────

RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
BLUE='\033[0;34m'
NC='\033[0m'

info()    { echo -e "${BLUE}[→]${NC} $*"; }
success() { echo -e "${GREEN}[✓]${NC} $*"; }
warn()    { echo -e "${YELLOW}[!]${NC} $*"; }
error()   { echo -e "${RED}[✗]${NC} $*" >&2; exit 1; }

# ─── Argument parsing ─────────────────────────────────────────────────────────

CHECK_ONLY=false
TARGET_NODES=("${NODES[@]}")

for arg in "$@"; do
  case $arg in
    --check) CHECK_ONLY=true ;;
    pi-*)    TARGET_NODES=("$arg") ;;
    *)       error "Unknown argument: $arg" ;;
  esac
done

# ─── Preflight checks ─────────────────────────────────────────────────────────

command -v cross >/dev/null 2>&1 || \
  error "cross not found — run: cargo install cross"

rustup target list --installed | grep -q "$TARGET" || \
  error "Target not installed — run: rustup target add $TARGET"

# ─── Build ────────────────────────────────────────────────────────────────────

info "Building $BINARY for $TARGET ($PROFILE)..."

cross build \
  --bin "$BINARY" \
  --target "$TARGET" \
  --profile "$PROFILE" \
  2>&1 | grep -E "Compiling|Finished|error" || true

BINARY_PATH="target/$TARGET/$PROFILE/$BINARY"

[ -f "$BINARY_PATH" ] || error "Build failed — binary not found at $BINARY_PATH"

BINARY_SIZE=$(du -sh "$BINARY_PATH" | cut -f1)
success "Built $BINARY ($BINARY_SIZE)"

$CHECK_ONLY && { success "Check only — skipping deploy"; exit 0; }

# ─── Deploy ───────────────────────────────────────────────────────────────────

FAILED_NODES=()

for node in "${TARGET_NODES[@]}"; do
  info "Deploying to $node..."

  # Ensure remote directory exists
  ssh "$REMOTE_USER@$node" "mkdir -p $REMOTE_DIR" 2>/dev/null || {
    warn "Cannot reach $node — skipping"
    FAILED_NODES+=("$node")
    continue
  }

  # Copy binary
  rsync -az --progress \
    "$BINARY_PATH" \
    "$REMOTE_USER@$node:$REMOTE_DIR/$BINARY" || {
    warn "rsync failed for $node — skipping"
    FAILED_NODES+=("$node")
    continue
  }

  # Always update systemd service file
  info "Updating systemd service on $node..."
  scp "deploy/picloud.service" "$REMOTE_USER@$node:/tmp/picloud.service"
  ssh "$REMOTE_USER@$node" \
    "sudo mv /tmp/picloud.service /etc/systemd/system/$SERVICE_NAME.service && \
     sudo systemctl daemon-reload && \
     sudo systemctl enable $SERVICE_NAME" 2>/dev/null || true

  # Restart service
  ssh "$REMOTE_USER@$node" "sudo systemctl restart $SERVICE_NAME" && \
    success "$node — restarted" || {
    warn "$node — restart failed (check: ssh $REMOTE_USER@$node journalctl -fu $SERVICE_NAME)"
    FAILED_NODES+=("$node")
  }
done

# ─── Summary ──────────────────────────────────────────────────────────────────

echo ""
DEPLOYED=$(( ${#TARGET_NODES[@]} - ${#FAILED_NODES[@]} ))

if [ ${#FAILED_NODES[@]} -eq 0 ]; then
  success "Deployed to all ${#TARGET_NODES[@]} nodes"
else
  warn "Deployed to $DEPLOYED/${#TARGET_NODES[@]} nodes"
  warn "Failed nodes: ${FAILED_NODES[*]}"
fi

echo ""
info "To watch logs across all nodes:"
echo "  ./deploy/logs.sh"
echo ""
info "To check cluster status:"
echo "  picloud cluster status"

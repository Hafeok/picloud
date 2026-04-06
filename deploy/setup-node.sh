#!/usr/bin/env bash
# setup-node.sh — one-time setup on a fresh Pi node
#
# Run this once per node before first deploy:
#   ssh ubuntu@<pi-ip> 'bash -s' < deploy/setup-node.sh
#
# What it does:
#   - Creates the picloud data directory
#   - Installs avahi-daemon for mDNS (so picloud.local resolves)
#   - Configures firewall rules for picloud ports
#   - Leaves Nomad completely untouched

set -euo pipefail

PICLOUD_DIR="/home/ubuntu/picloud"
PICLOUD_DATA="$PICLOUD_DIR/data"

echo "[→] Creating picloud directories..."
mkdir -p "$PICLOUD_DIR" "$PICLOUD_DATA"
chmod 750 "$PICLOUD_DATA"

echo "[→] Installing avahi-daemon (mDNS)..."
sudo apt-get update -qq
sudo apt-get install -y -qq avahi-daemon

# Enable mDNS resolution in nsswitch if not already there
if ! grep -q "mdns4_minimal" /etc/nsswitch.conf; then
  sudo sed -i 's/^hosts:.*/hosts:          files mdns4_minimal [NOTFOUND=return] dns/' \
    /etc/nsswitch.conf
  echo "[✓] Enabled mDNS in nsswitch.conf"
fi

sudo systemctl enable avahi-daemon
sudo systemctl start avahi-daemon

echo "[→] Configuring firewall rules for picloud..."
# Allow picloud ports — do not touch Nomad ports (4646-4648)
sudo ufw allow 7000/tcp comment "picloud HTTP"   || true
sudo ufw allow 7001/tcp comment "picloud Raft"   || true
sudo ufw allow 7002/tcp comment "picloud Events" || true
sudo ufw allow 5353/udp comment "mDNS"           || true

echo ""
echo "[✓] Node setup complete"
echo "    Run ./deploy/deploy.sh to deploy picloud-server"

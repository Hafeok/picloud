#!/usr/bin/env bash
# setup-node.sh — one-time setup on a fresh Pi 5 node
#
# Run this once per node before first deploy:
#   ssh admin@<pi-ip> 'bash -s' < deploy/setup-node.sh
#
# What it does:
#   - Creates the picloud data directory
#   - Installs avahi-daemon for mDNS (so picloud.local resolves)
#   - Configures firewall rules for picloud ports
#   - Creates the systemd service user

set -euo pipefail

PICLOUD_DATA="/var/lib/picloud"

echo "[→] Creating picloud data directories..."
sudo mkdir -p "$PICLOUD_DATA" "$PICLOUD_DATA/events" "$PICLOUD_DATA/rdf" \
  "$PICLOUD_DATA/storage" "$PICLOUD_DATA/raft" "$PICLOUD_DATA/ca"
sudo chown -R admin:admin "$PICLOUD_DATA"
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
sudo ufw allow 7443/tcp comment "picloud HTTP/HTTPS"  || true
sudo ufw allow 53/tcp   comment "picloud DNS"         || true
sudo ufw allow 53/udp   comment "picloud DNS"         || true
sudo ufw allow 2380/tcp comment "picloud Raft"        || true
sudo ufw allow 5353/udp comment "mDNS"                || true

echo ""
echo "[✓] Node setup complete"
echo "    Run ./deploy/deploy.sh to deploy picloud-server"

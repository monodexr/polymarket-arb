#!/bin/bash
# WireGuard split-tunnel VPN setup.
# Run this on the TORONTO VPS (the exit node).
#
# After running this, it prints the NJ client config.
# Copy that config to the NJ VPS and activate it.
#
# Usage: ssh root@TORONTO_IP bash -s < scripts/setup-vpn.sh

set -euo pipefail

echo "=== WireGuard Exit Node Setup (Toronto) ==="
echo

# Install WireGuard
apt-get update -qq
apt-get install -y wireguard

# Enable IP forwarding
echo "net.ipv4.ip_forward = 1" > /etc/sysctl.d/99-wireguard.conf
sysctl -p /etc/sysctl.d/99-wireguard.conf

# Generate server keys
cd /etc/wireguard
umask 077
wg genkey | tee server_private.key | wg pubkey > server_public.key
wg genkey | tee client_private.key | wg pubkey > client_public.key

SERVER_PRIVATE=$(cat server_private.key)
SERVER_PUBLIC=$(cat server_public.key)
CLIENT_PRIVATE=$(cat client_private.key)
CLIENT_PUBLIC=$(cat client_public.key)
SERVER_IP=$(curl -s ifconfig.me)
IFACE=$(ip route | grep default | awk '{print $5}' | head -1)

# Write server config
cat > /etc/wireguard/wg0.conf << EOF
[Interface]
PrivateKey = ${SERVER_PRIVATE}
Address = 10.100.0.1/24
ListenPort = 51820
PostUp = iptables -t nat -A POSTROUTING -o ${IFACE} -j MASQUERADE
PostDown = iptables -t nat -D POSTROUTING -o ${IFACE} -j MASQUERADE

[Peer]
PublicKey = ${CLIENT_PUBLIC}
AllowedIPs = 10.100.0.2/32
EOF

# Start WireGuard
systemctl enable wg-quick@wg0
systemctl start wg-quick@wg0

# Open firewall
ufw allow 51820/udp 2>/dev/null || true

echo
echo "=== Toronto exit node running ==="
echo
echo "Now configure the NJ VPS. Create this file on NJ:"
echo "  /etc/wireguard/wg0.conf"
echo
echo "--- START NJ CONFIG ---"
cat << EOF
[Interface]
PrivateKey = ${CLIENT_PRIVATE}
Address = 10.100.0.2/24

[Peer]
PublicKey = ${SERVER_PUBLIC}
Endpoint = ${SERVER_IP}:51820
AllowedIPs = 0.0.0.0/0
PersistentKeepalive = 25
EOF
echo "--- END NJ CONFIG ---"
echo
echo "Then on NJ, run:"
echo "  apt-get install -y wireguard"
echo "  # paste the config above into /etc/wireguard/wg0.conf"
echo "  wg-quick up wg0"
echo
echo "To split-tunnel (only route CLOB POST through VPN):"
echo "  # Instead of AllowedIPs = 0.0.0.0/0, use the Cloudflare IPs"
echo "  # that Polymarket CLOB resolves to. Or route all traffic through"
echo "  # VPN and accept the ~10ms overhead on feeds (still fast)."
echo
echo "Server public key: ${SERVER_PUBLIC}"
echo "Server IP: ${SERVER_IP}:51820"

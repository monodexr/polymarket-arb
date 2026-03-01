#!/bin/bash
# Apply VPN split-tunnel: only route Polymarket (Cloudflare) traffic through VPN.
# Binance WS and other traffic goes direct from NJ.
#
# Run on the NJ VPS: bash scripts/apply-vpn-split-tunnel.sh
#
# To revert: change AllowedIPs back to 0.0.0.0/0 in /etc/wireguard/wg0.conf
#            and run: wg-quick down wg0 && wg-quick up wg0

set -euo pipefail

echo "=== VPN Split-Tunnel: Route Only Cloudflare Through VPN ==="

CONF="/etc/wireguard/wg0.conf"

if [ ! -f "$CONF" ]; then
    echo "ERROR: $CONF not found"
    exit 1
fi

# Cloudflare IPv4 CIDR ranges (complete list from cloudflare.com/ips-v4)
CF_CIDRS="173.245.48.0/20, 103.21.244.0/22, 103.22.200.0/22, 103.31.4.0/22, 141.101.64.0/18, 108.162.192.0/18, 190.93.240.0/20, 188.114.96.0/20, 197.234.240.0/22, 198.41.128.0/17, 162.158.0.0/15, 104.16.0.0/13, 104.24.0.0/14, 172.64.0.0/13, 131.0.72.0/22"

# Backup current config
cp "$CONF" "${CONF}.bak.$(date +%s)"

# Replace AllowedIPs line
if grep -q "AllowedIPs = 0.0.0.0/0" "$CONF"; then
    sed -i "s|AllowedIPs = 0.0.0.0/0|AllowedIPs = ${CF_CIDRS}|" "$CONF"
    echo "Updated AllowedIPs to Cloudflare CIDRs only"
else
    echo "WARNING: AllowedIPs = 0.0.0.0/0 not found in config."
    echo "Current AllowedIPs:"
    grep "AllowedIPs" "$CONF"
    echo "Manually update if needed."
    exit 1
fi

echo "Restarting WireGuard..."
wg-quick down wg0 2>/dev/null || true
sleep 1
wg-quick up wg0

echo ""
echo "=== Verifying ==="

# Public IP should be NJ (not Toronto) for general traffic
PUBLIC_IP=$(curl -s --max-time 5 ifconfig.me)
echo "Public IP (should be NJ): $PUBLIC_IP"

# CLOB should still work through VPN
CLOB_STATUS=$(curl -s --max-time 10 -o /dev/null -w "%{http_code}" https://clob.polymarket.com/)
echo "CLOB reachable: HTTP $CLOB_STATUS"

# Binance should work direct
BINANCE_STATUS=$(curl -s --max-time 10 -o /dev/null -w "%{http_code}" https://api.binance.com/api/v3/ping)
echo "Binance direct: HTTP $BINANCE_STATUS"

if [ "$CLOB_STATUS" = "200" ] || [ "$CLOB_STATUS" = "404" ]; then
    echo ""
    echo "SUCCESS: Split-tunnel active. Binance goes direct, CLOB through VPN."
else
    echo ""
    echo "WARNING: CLOB may not be reachable. Check VPN status: wg show"
    echo "To revert: cp ${CONF}.bak.* $CONF && wg-quick down wg0 && wg-quick up wg0"
fi

#!/bin/bash
# Apply low-latency TCP tuning for trading.
# Run on the NJ VPS: bash scripts/apply-tcp-tuning.sh

set -euo pipefail

echo "=== TCP Low-Latency Tuning ==="

cat > /etc/sysctl.d/99-low-latency.conf << 'EOF'
net.core.default_qdisc = fq
net.ipv4.tcp_congestion_control = bbr
net.ipv4.tcp_low_latency = 1
net.ipv4.tcp_rmem = 4096 131072 16777216
net.ipv4.tcp_wmem = 4096 131072 16777216
net.core.rmem_max = 16777216
net.core.wmem_max = 16777216
net.ipv4.tcp_tw_reuse = 1
net.ipv4.tcp_fin_timeout = 10
net.ipv4.tcp_keepalive_time = 30
net.ipv4.tcp_keepalive_intvl = 10
net.ipv4.tcp_keepalive_probes = 3
net.ipv4.tcp_window_scaling = 1
net.core.somaxconn = 65535
net.ipv4.tcp_max_syn_backlog = 65535
EOF

sysctl -p /etc/sysctl.d/99-low-latency.conf

echo ""
echo "=== Verifying ==="
echo "Congestion control: $(sysctl -n net.ipv4.tcp_congestion_control)"
echo "Queue discipline: $(sysctl -n net.core.default_qdisc)"
echo "TCP low latency: $(sysctl -n net.ipv4.tcp_low_latency)"
echo ""
echo "Done. TCP tuning applied."

#!/bin/bash
set -e
cd /home/anfer/Code/C/AntX
rm -rf build
make build/net/qx_net_apps.o 2>&1 | grep -E "error:|warning:" | head -10
ls build/net/qx_net_apps.o 2>/dev/null && echo "qx_net_apps.o OK" || echo "qx_net_apps.o FAILED"
make build/net/qx_fsdata.o 2>&1 | grep -E "error:|warning:" | head -5
ls build/net/qx_fsdata.o 2>/dev/null && echo "qx_fsdata.o OK" || echo "qx_fsdata.o FAILED"
make build/net/qx_netif.o 2>&1 | grep -E "error:" | head -5
ls build/net/qx_netif.o 2>/dev/null && echo "qx_netif.o OK" || echo "qx_netif.o FAILED"

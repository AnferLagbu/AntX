#!/bin/bash
# 快速修复: 移除 net_glue.c 中的 netif_set_link_up (太早调用)
# 改为在 netif.rs 的 netif_set_up 之后调用
set -e
cd /home/anfer/Code/C/AntX

cp src/kernel/net/arch/net_glue.c backup/net_dhcp_fix/net_glue.c.bak5
cp src/kernel/net/netif.rs backup/net_dhcp_fix/netif.rs.bak5

# Fix net_glue.c — remove netif_set_link_up
python3 << 'PYEOF'
with open("src/kernel/net/arch/net_glue.c", "r") as f:
    content = f.read()

old = """    netif->output = etharp_output;
    netif->output_ip6 = ethip6_output;
    netif->linkoutput = e1000_send;
    netif->hostname = "antx";
    netif->name[0] = 'e';
    netif->name[1] = 'n';

    netif_set_link_up(netif);
}"""

new = """    netif->output = etharp_output;
    netif->output_ip6 = ethip6_output;
    netif->linkoutput = e1000_send;
    netif->name[0] = 'e';
    netif->name[1] = 'n';
}"""

if old in content:
    content = content.replace(old, new)
    print("  OK: netif_set_link_up removed from glue")
else:
    print("  ERROR: pattern not found in net_glue.c")

with open("src/kernel/net/arch/net_glue.c", "w") as f:
    f.write(content)
PYEOF

# Fix netif.rs — add netif_set_link_up after netif_set_up
python3 << 'PYEOF'
with open("src/kernel/net/netif.rs", "r") as f:
    content = f.read()

old = """    // 启动接口
    netif_set_up(result);"""

new = """    // 启动接口
    netif_set_up(result);
    // 标记链路为 UP (触发 DHCP)
    extern "C" { fn netif_set_link_up(netif: *mut core::ffi::c_void); }
    netif_set_link_up(result);"""

if old in content:
    content = content.replace(old, new)
    print("  OK: netif_set_link_up added after netif_set_up")
else:
    print("  ERROR: pattern not found")

with open("src/kernel/net/netif.rs", "w") as f:
    f.write(content)
PYEOF

echo "Done"

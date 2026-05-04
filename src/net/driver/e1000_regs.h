#ifndef E1000_REGS_H
#define E1000_REGS_H

/* ============================================================
 * Intel 82540EM (E1000) 寄存器定义
 *
 * 基于 Intel 82540EM Gigabit Ethernet Controller Datasheet
 * QEMU 默认模拟此设备 (-device e1000)
 * ============================================================ */

/* ---- 控制寄存器 ---- */
#define E1000_CTRL       0x00000  /* 设备控制 */
#define E1000_STATUS     0x00008  /* 设备状态 */
#define E1000_EERD       0x00014  /* EEPROM 读 */
#define E1000_CTRL_EXT   0x00018  /* 扩展设备控制 */
#define E1000_MDIC       0x00020  /* MDI 控制 (PHY 访问) */

/* ---- 中断 ---- */
#define E1000_ICR        0x000C0  /* 中断原因读取 */
#define E1000_ICS        0x000C8  /* 中断原因设置 (写1触发) */
#define E1000_IMS        0x000D0  /* 中断掩码设置 */
#define E1000_IMC        0x000D8  /* 中断掩码清除 */

/* ---- 接收 ---- */
#define E1000_RCTL       0x00100  /* 接收控制 */
#define E1000_RDBAL      0x02800  /* 接收描述符基址低32位 */
#define E1000_RDBAH      0x02804  /* 接收描述符基址高32位 */
#define E1000_RDLEN      0x02808  /* 接收描述符环长度 */
#define E1000_RDH        0x02810  /* 接收描述符头指针 */
#define E1000_RDT        0x02818  /* 接收描述符尾指针 */

/* ---- 发送 ---- */
#define E1000_TCTL       0x00400  /* 发送控制 */
#define E1000_TDBAL      0x03800  /* 发送描述符基址低32位 */
#define E1000_TDBAH      0x03804  /* 发送描述符基址高32位 */
#define E1000_TDLEN      0x03808  /* 发送描述符环长度 */
#define E1000_TDH        0x03810  /* 发送描述符头指针 */
#define E1000_TDT        0x03818  /* 发送描述符尾指针 */
#define E1000_TIPG       0x00410  /* 帧间间隔 */

/* ---- MAC 地址 ---- */
#define E1000_RA         0x05400  /* 接收地址 (MAC 过滤, 16字节 × 16) */
#define E1000_RAL_BASE   0x05400  /* RA Low */
#define E1000_RAH_BASE   0x05404  /* RA High */
#define E1000_MTA        0x05200  /* 多播表 (128×32位 = 4096 bits) */

/* ---- 统计 ---- */
#define E1000_CRCERRS    0x04000  /* CRC 错误 */
#define E1000_ALGNERRC   0x04004  /* 对齐错误 */
#define E1000_SYMERRS    0x04008  /* 符号错误 */
#define E1000_RXERRC     0x0400C  /* 接收错误 */
#define E1000_MPC        0x04010  /* 错包计数 */
#define E1000_TXC        0x04018  /* 发送完成 */
#define E1000_TXCARR     0x04038  /* 发送载波错误 */

/* ---- CTRL 位定义 ---- */
#define E1000_CTRL_FD        (1 <<  0)  /* 全双工 */
#define E1000_CTRL_LRST      (1 <<  3)  /* 链路复位 */
#define E1000_CTRL_ASDE      (1 <<  5)  /* 自动速率检测 */
#define E1000_CTRL_SLU       (1 <<  6)  /* 设置链路 */
#define E1000_CTRL_ILOS      (1 <<  7)  /* 反转信号丢失 */
#define E1000_CTRL_SPEED_10  (0 <<  8)  /* 10Mbps */
#define E1000_CTRL_SPEED_100 (1 <<  8)  /* 100Mbps */
#define E1000_CTRL_SPEED_1000 (2 << 8)  /* 1000Mbps */
#define E1000_CTRL_FRCSPD    (1 << 11)  /* 强制速率 */
#define E1000_CTRL_FRCDPX    (1 << 12)  /* 强制双工 */
#define E1000_CTRL_RST       (1 << 26)  /* 设备复位 */
#define E1000_CTRL_VME       (1 << 30)  /* VLAN 模式 */
#define E1000_CTRL_PHY_RST   (1 << 31)  /* PHY 复位 */

/* ---- STATUS 位定义 ---- */
#define E1000_STATUS_FD      (1 <<  0)  /* 全双工 */
#define E1000_STATUS_LU      (1 <<  1)  /* 链路指示 */
#define E1000_STATUS_SPEED_10   0
#define E1000_STATUS_SPEED_100  (1 << 6)
#define E1000_STATUS_SPEED_1000 (2 << 6)

/* ---- RCTL 位定义 ---- */
#define E1000_RCTL_EN        (1 <<  1)  /* 接收使能 */
#define E1000_RCTL_SBP       (1 <<  2)  /* 存储坏包 */
#define E1000_RCTL_UPE       (1 <<  3)  /* 单播混杂 */
#define E1000_RCTL_MPE       (1 <<  4)  /* 多播混杂 */
#define E1000_RCTL_LPE       (1 <<  5)  /* 长包接收 */
#define E1000_RCTL_LBM_NO    (0 <<  6)  /* 无回环 */
#define E1000_RCTL_LBM_PHY   (3 <<  6)  /* PHY 回环 */
#define E1000_RCTL_RDMTS_HALF (0 <<  8) /* RX Descriptor Minimum Threshold */
#define E1000_RCTL_RDMTS_QUARTER (1 <<  8)
#define E1000_RCTL_RDMTS_EIGHTH  (2 <<  8)
#define E1000_RCTL_BAM       (1 << 15)  /* 广播接收 */
#define E1000_RCTL_BSIZE_2048 (0 << 16)
#define E1000_RCTL_BSIZE_4096 (1 << 17)
#define E1000_RCTL_BSIZE_8192 (1 << 16)
#define E1000_RCTL_BSIZE_16384 (3 << 16)
#define E1000_RCTL_SECRC     (1 << 26)  /* 剥离 CRC */

/* ---- TCTL 位定义 ---- */
#define E1000_TCTL_EN        (1 <<  1)  /* 发送使能 */
#define E1000_TCTL_PSP       (1 <<  3)  /* 填充短包 */
#define E1000_TCTL_CT_SHIFT  4
#define E1000_TCTL_CT(x)     ((x) << 4) /* 碰撞阈值 */
#define E1000_TCTL_COLD_SHIFT 12
#define E1000_TCTL_COLD(x)   ((x) << 12) /* 碰撞距离 */

/* ---- EEC 位定义 ---- */
#define E1000_EERD_START     (1 <<  0)
#define E1000_EERD_DONE      (1 <<  4)
#define E1000_EERD_ADDR_SHIFT 2
#define E1000_EERD_DATA_SHIFT 16

/* ---- 中断位定义 ---- */
#define E1000_ICR_TXDW       (1 <<  0)  /* 发送描述符写回 */
#define E1000_ICR_TXQE       (1 <<  1)  /* 发送队列空 */
#define E1000_ICR_LSC        (1 <<  2)  /* 链路状态改变 */
#define E1000_ICR_RXSEQ      (1 <<  3)  /* 接收序列错误 */
#define E1000_ICR_RXDMT0     (1 <<  4)  /* 接收描述符最小阈值到达 */
#define E1000_ICR_RXO        (1 <<  6)  /* 接收器溢出 */
#define E1000_ICR_RXT0       (1 <<  7)  /* 接收定时器中断 */
#define E1000_ICR_MDAC       (1 <<  9)  /* MDI/O 访问完成 */
#define E1000_ICR_RXCFG      (1 << 10)  /* 接收配置 */
#define E1000_ICR_PHYINT     (1 << 12)  /* PHY 中断 */
#define E1000_ICR_GPI        (1 << 16)  /* 通用中断 */

/* ---- 发送描述符命令 ---- */
#define E1000_TXD_CMD_EOP    (1 <<  0)  /* 包结束 */
#define E1000_TXD_CMD_IFCS   (1 <<  1)  /* 插入 FCS/CRC */
#define E1000_TXD_CMD_IC     (1 <<  2)  /* 插入校验和 */
#define E1000_TXD_CMD_RS     (1 <<  3)  /* 报告状态 */
#define E1000_TXD_CMD_RPS    (1 <<  4)  /* 报告包发送 */
#define E1000_TXD_CMD_DEXT   (1 <<  5)  /* 描述符扩展 */
#define E1000_TXD_CMD_VLE    (1 <<  6)  /* VLAN 包使能 */
#define E1000_TXD_CMD_IDE    (1 <<  7)  /* 中断延迟使能 */

/* ---- 发送描述符状态 ---- */
#define E1000_TXD_STAT_DD    (1 <<  0)  /* 描述符完成 */

/* ---- 接收描述符状态 ---- */
#define E1000_RXD_STAT_DD    (1 <<  0)  /* 描述符完成 */
#define E1000_RXD_STAT_EOP   (1 <<  1)  /* 包结束 */
#define E1000_RXD_STAT_IXSM  (1 <<  2)  /* IP 校验和已计算 */
#define E1000_RXD_STAT_TCPCS (1 <<  5)  /* TCP 校验和已计算 */
#define E1000_RXD_STAT_UDPCS (1 <<  6)  /* UDP 校验和已计算 */
#define E1000_RXD_STAT_VP    (1 <<  7)  /* 匹配 VLAN */

/* ---- 接收描述符错误 ---- */
#define E1000_RXD_ERR_CE     (1 <<  0)  /* CRC 错误 */
#define E1000_RXD_ERR_SE     (1 <<  2)  /* 序列错误 */
#define E1000_RXD_ERR_SEQ    (1 <<  4)  /* 序列/符号错误 */
#define E1000_RXD_ERR_CXE    (1 <<  5)  /* 载波扩展错误 */
#define E1000_RXD_ERR_TCPE   (1 <<  6)  /* TCP/UDP 校验和错误 */
#define E1000_RXD_ERR_IPE    (1 <<  7)  /* IP 校验和错误 */
#define E1000_RXD_ERR_RXE    (1 <<  8)  /* RX 数据错误 */

/* ---- 环形大小 ---- */
#define E1000_TX_RING_SIZE   32
#define E1000_RX_RING_SIZE   32
#define E1000_RX_BUFFER_SIZE 2048

#endif /* E1000_REGS_H */

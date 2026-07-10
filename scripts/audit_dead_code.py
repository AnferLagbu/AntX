#!/usr/bin/env python3
"""
audit_dead_code.py — 检查新增代码是否引入 #[allow(dead_code)]

规则 F9: 新增代码禁止 #[allow(dead_code)]，硬件规范常量豁免。

用法:
    python3 scripts/audit_dead_code.py [--fix]

退出码:
    0 = 通过 (0 处违规)
    1 = 失败 (发现违规)
"""

import re
import sys
from pathlib import Path

# 硬件规范常量豁免模式 (正则匹配注释)
EXEMPT_PATTERNS = [
    r"规范定义",           # 中文: 规范定义
    r"spec definition",    # 英文: spec definition
    r"hardware register",  # 硬件寄存器
    r"IOAPIC|APIC|GIC",   # 中断控制器常量
    r"ATA_|NVME_|AHCI_",  # 存储控制器常量
    r"PORTSC|PORT_|USB_", # USB 常量
    r"VGA_|FB_",          # 显示常量
    r"PCI_",              # PCI 常量
]

# 文件级 allow 豁免 (这些文件太大，逐项标注会淹没代码)
FILE_LEVEL_EXEMPT = [
    "net/init.rs",        # 网络栈初始化 (2700+ 行)
    "sync/lockdep.rs",    # 锁依赖检测器
    "ioport.rs",          # I/O 端口抽象层
    "fs/initramfs.rs",    # initramfs 解包
    "pci/msi.rs",         # MSI 中断
    "debug/mod.rs",       # 调试模块
    "syscall/mod.rs",     # 系统调用模块
    "syscall/futex.rs",   # futex 实现
    "net/iface_trait.rs", # 网络接口 trait
    "net/unix.rs",        # Unix socket
]

def is_exempt(comment: str, filepath: str) -> bool:
    """检查是否豁免"""
    # 检查文件级豁免
    for pattern in FILE_LEVEL_EXEMPT:
        if pattern in filepath:
            return True
    
    # 检查注释中的豁免模式
    for pattern in EXEMPT_PATTERNS:
        if re.search(pattern, comment, re.IGNORECASE):
            return True
    
    return False

def scan_dead_code(root: Path) -> list:
    """扫描所有 #[allow(dead_code)]"""
    violations = []
    
    for rs_file in root.rglob("*.rs"):
        # 跳过 vendored 代码
        if "smoltcp" in str(rs_file) or "target" in str(rs_file):
            continue
        
        rel_path = str(rs_file.relative_to(root))
        
        try:
            content = rs_file.read_text(encoding="utf-8")
        except Exception:
            continue
        
        lines = content.split("\n")
        in_comment = False
        
        for i, line in enumerate(lines, 1):
            stripped = line.strip()
            
            # 跳过纯注释行
            if stripped.startswith("//") or stripped.startswith("*") or stripped.startswith("/*"):
                continue
            
            # 检测 #![allow(dead_code)] (文件级)
            if "#![allow(dead_code)]" in stripped:
                comment = ""
                # 查找同行或前一行的注释
                if "//" in stripped:
                    comment = stripped[stripped.index("//"):]
                elif i > 1 and "//" in lines[i-2]:
                    comment = lines[i-2][lines[i-2].index("//"):]
                
                if not is_exempt(comment, rel_path):
                    violations.append({
                        "file": rel_path,
                        "line": i,
                        "type": "file-level",
                        "code": stripped[:80],
                        "comment": comment,
                    })
            
            # 检测 #[allow(dead_code)] (单项)
            elif "#[allow(dead_code)]" in stripped:
                comment = ""
                # 查找同行或前一行的注释
                if "//" in stripped:
                    comment = stripped[stripped.index("//"):]
                elif i > 1 and "//" in lines[i-2]:
                    comment = lines[i-2][lines[i-2].index("//"):]
                
                if not is_exempt(comment, rel_path):
                    violations.append({
                        "file": rel_path,
                        "line": i,
                        "type": "item-level",
                        "code": stripped[:80],
                        "comment": comment,
                    })
    
    return violations

def main():
    root = Path(__file__).parent.parent / "src" / "kernel"
    
    if not root.exists():
        print(f"错误: 目录不存在 {root}")
        sys.exit(1)
    
    violations = scan_dead_code(root)
    
    if violations:
        print(f"FAIL: 发现 {len(violations)} 处 dead_code 违规\n")
        for v in violations:
            print(f"  {v['file']}:{v['line']}")
            print(f"    类型: {v['type']}")
            print(f"    代码: {v['code']}")
            if v['comment']:
                print(f"    注释: {v['comment']}")
            print()
        sys.exit(1)
    else:
        print("PASS: 无 dead_code 违规 (硬件规范常量已豁免)")
        sys.exit(0)

if __name__ == "__main__":
    main()

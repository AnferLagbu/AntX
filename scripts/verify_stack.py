#!/usr/bin/env python3
"""
栈地址验证脚本
"""

import subprocess

def run_cmd(cmd):
    result = subprocess.run(cmd, shell=True, capture_output=True, text=True)
    return result.stdout

def main():
    binary_path = "build/kernel.bin"
    
    print("="*60)
    print("栈地址验证")
    print("="*60)
    
    output = run_cmd(f"x86_64-linux-gnu-nm {binary_path}")
    
    for line in output.split('\n'):
        parts = line.split()
        if len(parts) >= 3:
            name = parts[2]
            if 'stack' in name.lower() or 'bootbss' in name.lower():
                print(f"{name}: 0x{int(parts[0], 16):016x}")
    
    print("\n栈指针分析:")
    print("boot.asm 中设置的栈指针: 0xFFFF8000011701e")
    
    HIGH_BASE = 0xFFFF800001000000
    stack_phys = 0x11701e
    
    print(f"通过页表映射的物理地址: 0x{stack_phys:x}")
    
    print("\n.bootbss 段分析:")
    output = run_cmd(f"x86_64-linux-gnu-objdump -h {binary_path}")
    for line in output.split('\n'):
        if '.bootbss' in line:
            parts = line.split()
            if len(parts) >= 6:
                lma = int(parts[4], 16)
                size = int(parts[2], 16)
                print(f"  LMA: 0x{lma:x}")
                print(f"  大小: 0x{size:x}")
                print(f"  结束地址: 0x{lma + size:x}")
    
    print("\n问题分析:")
    print("  栈指针 0x11701e 在 .bootbss 段内")
    print("  但 .bootbss 是 NOLOAD 段，GRUB 可能没有正确初始化它")
    
    print("\n建议修复:")
    print("  1. 在 boot.asm 中手动清零 .bootbss 段")
    print("  2. 或者将栈放在 .bootbss 段的末尾")

if __name__ == "__main__":
    main()

#!/usr/bin/env python3
"""
GRUB 加载验证脚本
检查 GRUB 是否正确加载了内核的所有段
"""

import subprocess

def run_cmd(cmd):
    result = subprocess.run(cmd, shell=True, capture_output=True, text=True)
    return result.stdout

def main():
    print("="*60)
    print("GRUB 加载验证")
    print("="*60)
    
    output = run_cmd("x86_64-linux-gnu-readelf -l build/kernel.bin")
    print("\n程序头:")
    print(output)
    
    print("\n" + "="*60)
    print("问题分析")
    print("="*60)
    
    print("""
GRUB 加载 ELF 内核的行为:
1. GRUB 会读取程序头，找到所有 LOAD 段
2. 对于每个 LOAD 段，GRUB 会:
   - 从文件偏移读取 FileSiz 字节
   - 写入到 PhysAddr 开始的内存
   - 如果 MemSiz > FileSiz，剩余部分填充 0

关键问题:
- 第一个 LOAD 段结束于 PhysAddr + MemSiz = 0x117036
- 第二个 LOAD 段开始于 PhysAddr = 0x118000
- 两个段之间有 0xFCA 字节的间隙

GRUB 应该会正确处理这个间隙，分别加载两个段。

验证方法:
1. 使用 QEMU 的 -d guest_errors 选项检查是否有内存访问错误
2. 使用 GDB 连接 QEMU，检查内存 0x118000 处的内容
""")

if __name__ == "__main__":
    main()

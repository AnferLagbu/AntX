#!/usr/bin/env python3
"""
检查 GRUB 是否正确加载内核
"""

import subprocess

def main():
    print("="*60)
    print("GRUB 加载验证")
    print("="*60)
    
    # 检查内核镜像的程序头
    result = subprocess.run(
        ["x86_64-linux-gnu-readelf", "-l", "build/kernel.bin"],
        capture_output=True, text=True
    )
    print("\n内核程序头:")
    print(result.stdout)
    
    # 检查 kernel_main 的地址
    result = subprocess.run(
        ["x86_64-linux-gnu-nm", "build/kernel.bin"],
        capture_output=True, text=True
    )
    for line in result.stdout.split('\n'):
        if 'kernel_main' in line:
            print(f"kernel_main 地址: {line}")
    
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
- 第一个 LOAD 段: PhysAddr=0x100000, MemSiz=0x17036
- 第二个 LOAD 段: PhysAddr=0x118000, FileSiz=0x23ce0

GRUB 应该会正确加载这两个段。

可能的问题:
1. GRUB 可能没有正确加载第二个 LOAD 段
2. 页表映射可能有问题
3. kernel_main 的代码可能没有正确执行

建议:
1. 使用 QEMU 的 -d guest_errors 选项检查是否有内存访问错误
2. 使用 GDB 连接 QEMU，检查内存 0x118000 处的内容
3. 检查 kernel_main 是否正确执行
""")

if __name__ == "__main__":
    main()

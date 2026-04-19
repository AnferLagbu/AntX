#!/usr/bin/env python3
"""
AntX 内核镜像分析脚本
分析 ELF 段布局、页表映射和地址转换
"""

import struct
import subprocess
import sys
import os

def run_cmd(cmd):
    """运行命令并返回输出"""
    result = subprocess.run(cmd, shell=True, capture_output=True, text=True)
    return result.stdout

def parse_elf_segments(binary_path):
    """解析 ELF 程序头"""
    output = run_cmd(f"x86_64-linux-gnu-readelf -l {binary_path}")
    segments = []
    
    lines = output.split('\n')
    in_load = False
    for line in lines:
        if 'LOAD' in line and 'Offset' not in line:
            parts = line.split()
            if len(parts) >= 5:
                try:
                    seg = {
                        'type': parts[0],
                        'offset': int(parts[1], 16) if parts[1].startswith('0x') else int(parts[1]),
                        'virt_addr': int(parts[2], 16) if parts[2].startswith('0x') else int(parts[2]),
                        'phys_addr': int(parts[3], 16) if parts[3].startswith('0x') else int(parts[3]),
                        'file_size': int(parts[4], 16) if parts[4].startswith('0x') else int(parts[4]),
                        'mem_size': int(parts[5], 16) if parts[5].startswith('0x') else int(parts[5]),
                        'flags': parts[6] if len(parts) > 6 else ''
                    }
                    segments.append(seg)
                except (ValueError, IndexError) as e:
                    pass
    return segments

def parse_elf_sections(binary_path):
    """解析 ELF 节头"""
    output = run_cmd(f"x86_64-linux-gnu-objdump -h {binary_path}")
    sections = []
    
    lines = output.split('\n')
    for line in lines:
        parts = line.split()
        if len(parts) >= 6 and parts[0].isdigit():
            try:
                sec = {
                    'idx': int(parts[0]),
                    'name': parts[1],
                    'size': int(parts[2], 16),
                    'vma': int(parts[3], 16),
                    'lma': int(parts[4], 16),
                    'file_offset': int(parts[5], 16)
                }
                sections.append(sec)
            except (ValueError, IndexError):
                pass
    return sections

def parse_symbols(binary_path):
    """解析符号表"""
    output = run_cmd(f"x86_64-linux-gnu-nm {binary_path}")
    symbols = {}
    
    for line in output.split('\n'):
        parts = line.split()
        if len(parts) >= 3:
            try:
                addr = int(parts[0], 16)
                sym_type = parts[1]
                name = parts[2]
                symbols[name] = {'addr': addr, 'type': sym_type}
            except (ValueError, IndexError):
                pass
    return symbols

def analyze_page_table(binary_path):
    """分析页表结构"""
    print("\n" + "="*60)
    print("页表映射分析")
    print("="*60)
    
    sections = parse_elf_sections(binary_path)
    symbols = parse_symbols(binary_path)
    
    bootbss = None
    pd_high = None
    pdpt_high = None
    pml4 = None
    
    for sec in sections:
        if sec['name'] == '.bootbss':
            bootbss = sec
        if sec['name'] == '.text':
            print(f"\n.text 段:")
            print(f"  VMA: 0x{sec['vma']:016x}")
            print(f"  LMA: 0x{sec['lma']:016x}")
            print(f"  Size: 0x{sec['size']:x}")
    
    for name, sym in symbols.items():
        if name == 'pd_high':
            pd_high = sym['addr']
        elif name == 'pdpt_high':
            pdpt_high = sym['addr']
        elif name == 'pml4':
            pml4 = sym['addr']
        elif name == 'kernel_main':
            print(f"\nkernel_main 地址: 0x{sym['addr']:016x}")
        elif name == '_start':
            print(f"_start 地址: 0x{sym['addr']:016x}")
    
    print(f"\n页表符号地址:")
    print(f"  pml4: 0x{pml4:016x}" if pml4 else "  pml4: 未找到")
    print(f"  pdpt_high: 0x{pdpt_high:016x}" if pdpt_high else "  pdpt_high: 未找到")
    print(f"  pd_high: 0x{pd_high:016x}" if pd_high else "  pd_high: 未找到")
    
    return sections, symbols

def analyze_virtual_to_physical(virt_addr, pd_high_base=0x87):
    """
    分析虚拟地址到物理地址的映射
    基于 boot.asm 中的页表设置
    """
    print(f"\n虚拟地址 0x{virt_addr:016x} 的映射分析:")
    
    if virt_addr >= 0xFFFF800000000000:
        offset = virt_addr - 0xFFFF800001000000
        if offset >= 0:
            phys_addr = offset
            print(f"  通过高地址映射: 物理地址 = 0x{phys_addr:016x}")
            return phys_addr
    
    return None

def check_kernel_loading(binary_path):
    """检查内核加载布局"""
    print("\n" + "="*60)
    print("内核加载布局分析")
    print("="*60)
    
    segments = parse_elf_segments(binary_path)
    
    print("\nLOAD 段:")
    for i, seg in enumerate(segments):
        print(f"\n  段 {i}:")
        print(f"    物理地址: 0x{seg['phys_addr']:016x}")
        print(f"    虚拟地址: 0x{seg['virt_addr']:016x}")
        print(f"    文件大小: 0x{seg['file_size']:x}")
        print(f"    内存大小: 0x{seg['mem_size']:x}")
        
        end_phys = seg['phys_addr'] + seg['mem_size']
        print(f"    结束物理地址: 0x{end_phys:016x}")
    
    if len(segments) >= 2:
        gap = segments[1]['phys_addr'] - (segments[0]['phys_addr'] + segments[0]['mem_size'])
        print(f"\n  段间隙: 0x{gap:x} 字节")
        if gap > 0:
            print(f"  警告: 段之间存在间隙，GRUB 可能不会填充!")
    
    return segments

def check_copy_overlap():
    """检查复制操作是否重叠"""
    print("\n" + "="*60)
    print("复制操作重叠分析")
    print("="*60)
    
    src_start = 0x118000
    src_end = 0x118000 + 0x124000
    dst_virt_start = 0xFFFF800001118000
    dst_virt_end = dst_virt_start + 0x124000
    
    dst_phys_start = 0x118000
    dst_phys_end = 0x118000 + 0x124000
    
    print(f"\n原始复制操作:")
    print(f"  源地址 (物理): 0x{src_start:x} - 0x{src_end:x}")
    print(f"  目标地址 (虚拟): 0x{dst_virt_start:016x} - 0x{dst_virt_end:016x}")
    print(f"  目标地址 (物理): 0x{dst_phys_start:x} - 0x{dst_phys_end:x}")
    
    if src_start == dst_phys_start:
        print(f"\n  错误: 源地址和目标物理地址完全重叠!")
        print(f"  这会导致复制操作破坏源数据!")
    
    print(f"\n建议修复方案:")
    print(f"  方案1: 移除复制操作 (代码已在正确位置)")
    print(f"  方案2: 修改 pd_high 映射起始地址")

def analyze_iretq_frame():
    """分析 iretq 栈帧"""
    print("\n" + "="*60)
    print("iretq 栈帧分析")
    print("="*60)
    
    print("""
iretq 栈帧结构 (64位模式):
  SS     <- 栈顶
  RSP
  RFLAGS
  CS
  RIP    <- 栈底

iretq 恢复的寄存器:
  - RIP: 指令指针
  - CS:  代码段选择子
  - RFLAGS: 标志寄存器
  - RSP: 栈指针
  - SS:  栈段选择子

iretq 不会恢复的寄存器:
  - DS, ES, FS, GS (数据段选择子)

关键点:
  1. iretq 只恢复 SS, RSP, RFLAGS, CS, RIP
  2. DS/ES/FS/GS 必须在 iretq 之前设置
  3. 在 CPL=3 时，数据段选择子不能为 NULL
""")

def analyze_mature_os_solutions():
    """分析成熟操作系统的解决方案"""
    print("\n" + "="*60)
    print("成熟操作系统用户态切换方案")
    print("="*60)
    
    print("""
=== Linux 方案 ===
1. 使用 swapgs 指令交换 GS 基址
2. 在 entry_trampoline 中设置段寄存器
3. iretq 前通过汇编代码设置 DS/ES/FS/GS

关键代码路径:
  - entry_64.S: entry_SYSCALL_64
  - 使用 SWAPGS 切换内核/用户 GS
  - 通过 per-CPU 区域存储段选择子

=== FreeBSD 方案 ===
1. 使用 trampoline 代码
2. 在 iretq 前设置段寄存器
3. 使用 fxsave/fxrstor 保存浮点状态

=== OpenBSD 方案 ===
1. 类似 FreeBSD
2. 额外的安全检查

=== 共同点 ===
1. iretq 前必须设置 DS/ES/FS/GS
2. 使用汇编 trampoline 代码
3. 设置正确的段选择子 (0x23 for user data)
4. 确保 GDT 中有正确的用户段描述符

=== AntX 应该采用的方案 ===
1. 在 iretq 前设置 DS/ES/FS/GS = 0x23
2. 确保 GDT 中用户数据段 DPL=3
3. 用户程序入口使用 naked 函数
4. 用户程序入口立即设置段寄存器
""")

def main():
    binary_path = "build/kernel.bin"
    
    if not os.path.exists(binary_path):
        print(f"错误: 找不到 {binary_path}")
        print("请先运行 make all")
        return 1
    
    print("="*60)
    print("AntX 内核镜像分析")
    print("="*60)
    
    check_kernel_loading(binary_path)
    analyze_page_table(binary_path)
    check_copy_overlap()
    analyze_iretq_frame()
    analyze_mature_os_solutions()
    
    return 0

if __name__ == "__main__":
    sys.exit(main())

#!/usr/bin/env python3
"""
页表映射验证脚本
检查虚拟地址到物理地址的映射是否正确
"""

import subprocess
import struct
import sys

def run_cmd(cmd):
    result = subprocess.run(cmd, shell=True, capture_output=True, text=True)
    return result.stdout

def parse_elf_segments(binary_path):
    """解析 ELF 程序头"""
    output = run_cmd(f"x86_64-linux-gnu-readelf -l {binary_path}")
    segments = []
    
    lines = output.split('\n')
    for i, line in enumerate(lines):
        if 'LOAD' in line and 'Offset' not in line:
            parts = line.split()
            if len(parts) >= 6:
                try:
                    seg = {
                        'type': parts[0],
                        'flags': parts[1],
                        'offset': int(parts[2], 16),
                        'virt_addr': int(parts[3], 16),
                        'phys_addr': int(parts[4], 16),
                        'file_size': int(parts[5], 16),
                        'mem_size': int(parts[6], 16) if len(parts) > 6 else 0
                    }
                    segments.append(seg)
                except (ValueError, IndexError):
                    pass
    return segments

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

def parse_sections(binary_path):
    """解析节头"""
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

def analyze_page_mapping(virt_addr, pd_high_base=0x87):
    """
    分析虚拟地址通过 pd_high 的映射
    pd_high 映射 0xFFFF800001000000 开始的 1GB 空间
    每个页目录项映射 2MB
    """
    print(f"\n虚拟地址 0x{virt_addr:016x} 的映射分析:")
    
    HIGH_BASE = 0xFFFF800001000000
    
    if virt_addr < HIGH_BASE or virt_addr >= HIGH_BASE + 0x40000000:
        print(f"  地址不在 pd_high 映射范围内")
        return None
    
    offset = virt_addr - HIGH_BASE
    pd_index = offset // 0x200000
    page_offset = offset % 0x200000
    
    pd_entry_addr = 0x87 + pd_index * 0x200000
    phys_addr = pd_entry_addr + page_offset
    
    print(f"  偏移: 0x{offset:x}")
    print(f"  页目录索引: {pd_index}")
    print(f"  页内偏移: 0x{page_offset:x}")
    print(f"  物理地址: 0x{phys_addr:x}")
    
    return phys_addr

def check_grub_loading(binary_path):
    """检查 GRUB 加载情况"""
    print("\n" + "="*60)
    print("GRUB 加载分析")
    print("="*60)
    
    segments = parse_elf_segments(binary_path)
    sections = parse_sections(binary_path)
    symbols = parse_symbols(binary_path)
    
    print("\nLOAD 段:")
    for i, seg in enumerate(segments):
        print(f"\n  段 {i}:")
        print(f"    物理地址: 0x{seg['phys_addr']:016x}")
        print(f"    虚拟地址: 0x{seg['virt_addr']:016x}")
        print(f"    文件大小: 0x{seg['file_size']:x}")
        print(f"    内存大小: 0x{seg['mem_size']:x}")
        
        end_phys = seg['phys_addr'] + seg['mem_size']
        print(f"    结束物理地址: 0x{end_phys:016x}")
    
    kernel_main = symbols.get('kernel_main', {})
    if kernel_main:
        print(f"\nkernel_main 符号:")
        print(f"  虚拟地址: 0x{kernel_main['addr']:016x}")
        
        phys_addr = analyze_page_mapping(kernel_main['addr'])
        
        if phys_addr:
            in_segment = False
            for seg in segments:
                if seg['phys_addr'] <= phys_addr < seg['phys_addr'] + seg['mem_size']:
                    in_segment = True
                    print(f"  物理地址在 LOAD 段内: 是")
                    print(f"    段物理地址范围: 0x{seg['phys_addr']:x} - 0x{seg['phys_addr'] + seg['mem_size']:x}")
                    break
            
            if not in_segment:
                print(f"  物理地址在 LOAD 段内: 否 (警告!)")
                print(f"  GRUB 可能没有加载这个地址的代码!")
    
    print("\n.text 段:")
    for sec in sections:
        if sec['name'] == '.text':
            print(f"  VMA: 0x{sec['vma']:016x}")
            print(f"  LMA: 0x{sec['lma']:016x}")
            print(f"  大小: 0x{sec['size']:x}")
            print(f"  文件偏移: 0x{sec['file_offset']:x}")
            
            phys_start = sec['lma']
            phys_end = phys_start + sec['size']
            print(f"  物理地址范围: 0x{phys_start:x} - 0x{phys_end:x}")

def check_boot_code(binary_path):
    """检查启动代码位置"""
    print("\n" + "="*60)
    print("启动代码分析")
    print("="*60)
    
    sections = parse_sections(binary_path)
    symbols = parse_symbols(binary_path)
    
    for sec in sections:
        if 'boot' in sec['name'] or sec['name'] == '.text':
            print(f"\n{sec['name']} 段:")
            print(f"  VMA: 0x{sec['vma']:016x}")
            print(f"  LMA: 0x{sec['lma']:016x}")
            print(f"  大小: 0x{sec['size']:x}")
    
    _start = symbols.get('_start', {})
    if _start:
        print(f"\n_start 符号:")
        print(f"  地址: 0x{_start['addr']:016x}")
    
    trampoline = symbols.get('trampoline64_high', {})
    if trampoline:
        print(f"\ntrampoline64_high 符号:")
        print(f"  地址: 0x{trampoline['addr']:016x}")

def main():
    binary_path = "build/kernel.bin"
    
    print("="*60)
    print("AntX 页表映射验证")
    print("="*60)
    
    check_grub_loading(binary_path)
    check_boot_code(binary_path)
    
    print("\n" + "="*60)
    print("问题诊断")
    print("="*60)
    
    symbols = parse_symbols(binary_path)
    sections = parse_sections(binary_path)
    
    kernel_main = symbols.get('kernel_main', {}).get('addr', 0)
    text_section = None
    for sec in sections:
        if sec['name'] == '.text':
            text_section = sec
            break
    
    if kernel_main and text_section:
        text_phys_start = text_section['lma']
        text_phys_end = text_phys_start + text_section['size']
        
        HIGH_BASE = 0xFFFF800001000000
        kernel_main_phys = kernel_main - HIGH_BASE
        
        print(f"\nkernel_main 虚拟地址: 0x{kernel_main:016x}")
        print(f"kernel_main 物理地址 (通过页表映射): 0x{kernel_main_phys:x}")
        print(f".text 段物理地址范围: 0x{text_phys_start:x} - 0x{text_phys_end:x}")
        
        if text_phys_start <= kernel_main_phys < text_phys_end:
            print(f"\n结论: kernel_main 在 .text 段内，映射正确")
        else:
            print(f"\n警告: kernel_main 不在 .text 段内!")
            print(f"  这可能导致 GPF，因为代码没有被正确加载")

if __name__ == "__main__":
    main()

#!/usr/bin/env python3
"""
AntX 内核启动诊断脚本 - 增强版
全面检查内核镜像、页表映射和 GRUB 加载问题
"""

import subprocess
import struct
import sys
import os
import re

def run_cmd(cmd):
    """运行命令并返回输出"""
    result = subprocess.run(cmd, shell=True, capture_output=True, text=True)
    return result.stdout

def parse_elf_program_headers(binary_path):
    """解析 ELF 程序头 - 处理两行格式"""
    output = run_cmd(f"x86_64-linux-gnu-readelf -l {binary_path}")
    segments = []
    
    lines = output.split('\n')
    i = 0
    while i < len(lines):
        line = lines[i]
        # 检查是否是 LOAD 行
        if 'LOAD' in line and 'Type' not in line and 'Offset' not in line:
            parts = line.split()
            # 找到 LOAD 在 parts 中的位置
            load_idx = -1
            for idx, p in enumerate(parts):
                if p == 'LOAD':
                    load_idx = idx
                    break
            
            # LOAD 后面应该有 4 个字段: Offset, VirtAddr, PhysAddr
            if load_idx >= 0 and len(parts) >= load_idx + 4:
                try:
                    seg = {
                        'type': parts[load_idx],
                        'offset': int(parts[load_idx + 1], 16),
                        'virt_addr': int(parts[load_idx + 2], 16),
                        'phys_addr': int(parts[load_idx + 3], 16),
                    }
                    # 下一行包含 FileSiz, MemSiz, Flags, Align
                    if i + 1 < len(lines):
                        next_line = lines[i + 1].strip()
                        next_parts = next_line.split()
                        if len(next_parts) >= 2:
                            seg['file_size'] = int(next_parts[0], 16)
                            seg['mem_size'] = int(next_parts[1], 16)
                            seg['flags'] = next_parts[2] if len(next_parts) > 2 else ''
                            segments.append(seg)
                            i += 1
                except (ValueError, IndexError) as e:
                    print(f"解析错误: {e}")
        i += 1
    
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

def analyze_page_mapping(virt_addr):
    """分析虚拟地址到物理地址的映射"""
    HIGH_BASE = 0xFFFF800001000000
    
    if virt_addr < HIGH_BASE or virt_addr >= HIGH_BASE + 0x40000000:
        return None
    
    offset = virt_addr - HIGH_BASE
    phys_addr = offset
    return phys_addr

def check_kernel_loading(binary_path):
    """检查内核加载情况"""
    print("\n" + "="*60)
    print("1. 内核加载分析")
    print("="*60)
    
    output = run_cmd(f"x86_64-linux-gnu-readelf -l {binary_path}")
    print("\n程序头输出:")
    print(output)
    
    segments = parse_elf_program_headers(binary_path)
    symbols = parse_symbols(binary_path)
    
    print("\n解析的 LOAD 段:")
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
        if phys_addr is not None:
            print(f"  映射物理地址: 0x{phys_addr:x}")
            
            in_segment = False
            for i, seg in enumerate(segments):
                seg_start = seg['phys_addr']
                seg_end = seg['phys_addr'] + seg['mem_size']
                if seg_start <= phys_addr < seg_end:
                    in_segment = True
                    print(f"  在 LOAD 段内: 是 (段 {i}: 0x{seg_start:x} - 0x{seg_end:x})")
                    break
            
            if not in_segment:
                print(f"  在 LOAD 段内: 否 (警告!)")
                print(f"  GRUB 可能没有加载这个地址的代码!")
    
    return segments, symbols

def check_page_table_setup(binary_path):
    """检查页表设置"""
    print("\n" + "="*60)
    print("2. 页表设置分析")
    print("="*60)
    
    symbols = parse_symbols(binary_path)
    
    pml4 = symbols.get('pml4', {})
    pdpt_high = symbols.get('pdpt_high', {})
    pd_high = symbols.get('pd_high', {})
    
    print(f"\n页表符号地址:")
    print(f"  pml4: 0x{pml4.get('addr', 0):016x}")
    print(f"  pdpt_high: 0x{pdpt_high.get('addr', 0):016x}")
    print(f"  pd_high: 0x{pd_high.get('addr', 0):016x}")
    
    print(f"\n页表映射分析:")
    print(f"  pd_high 映射虚拟地址范围: 0xFFFF800001000000 - 0xFFFF800041000000")
    print(f"  映射公式: 物理地址 = 虚拟地址 - 0xFFFF800001000000")
    
    kernel_main = symbols.get('kernel_main', {})
    if kernel_main:
        virt = kernel_main['addr']
        phys = analyze_page_mapping(virt)
        print(f"\n  kernel_main 虚拟地址: 0x{virt:016x}")
        print(f"  kernel_main 物理地址: 0x{phys:x}")

def check_kernel_main_code(binary_path):
    """检查 kernel_main 代码"""
    print("\n" + "="*60)
    print("3. kernel_main 代码分析")
    print("="*60)
    
    output = run_cmd(f"x86_64-linux-gnu-objdump -d {binary_path}")
    
    in_kernel_main = False
    lines_count = 0
    for line in output.split('\n'):
        if '<kernel_main>:' in line:
            in_kernel_main = True
            print(f"\nkernel_main 反汇编:")
        
        if in_kernel_main:
            print(f"  {line}")
            lines_count += 1
            if lines_count > 25:
                print("  ...")
                break

def check_file_content(binary_path):
    """检查文件内容"""
    print("\n" + "="*60)
    print("4. 文件内容检查")
    print("="*60)
    
    symbols = parse_symbols(binary_path)
    kernel_main = symbols.get('kernel_main', {})
    
    if kernel_main:
        virt_addr = kernel_main['addr']
        phys_addr = analyze_page_mapping(virt_addr)
        
        if phys_addr is not None:
            file_offset = 0x3000 + (phys_addr - 0x118000)
            
            print(f"\nkernel_main 文件偏移: 0x{file_offset:x}")
            
            with open(binary_path, 'rb') as f:
                f.seek(file_offset)
                data = f.read(32)
                
                print(f"文件内容 (前 32 字节):")
                print(f"  {data.hex()}")
                
                if data[0] == 0x55:
                    print(f"  第一条指令: push rbp (0x55) - 正确")
                else:
                    print(f"  第一条指令: 未知 (0x{data[0]:02x}) - 可能有问题")

def generate_diagnosis_report(binary_path):
    """生成诊断报告"""
    print("\n" + "="*60)
    print("5. 诊断报告")
    print("="*60)
    
    segments = parse_elf_program_headers(binary_path)
    symbols = parse_symbols(binary_path)
    kernel_main = symbols.get('kernel_main', {})
    
    issues = []
    
    if kernel_main:
        virt_addr = kernel_main['addr']
        phys_addr = analyze_page_mapping(virt_addr)
        
        if phys_addr is not None:
            in_segment = False
            for seg in segments:
                seg_start = seg['phys_addr']
                seg_end = seg['phys_addr'] + seg['mem_size']
                if seg_start <= phys_addr < seg_end:
                    in_segment = True
                    break
            
            if not in_segment:
                issues.append("kernel_main 不在任何 LOAD 段内")
    
    if len(segments) >= 2:
        gap = segments[1]['phys_addr'] - (segments[0]['phys_addr'] + segments[0]['mem_size'])
        if gap < 0:
            issues.append("LOAD 段重叠")
    
    if issues:
        print("\n发现的问题:")
        for i, issue in enumerate(issues, 1):
            print(f"  {i}. {issue}")
    else:
        print("\n未发现明显问题")
    
    print("\n建议的调试步骤:")
    print("  1. 使用 GDB 检查内存 0x118000 处的内容")
    print("  2. 使用 QEMU 的 -d guest_errors 选项检查内存访问错误")
    print("  3. 在 boot.asm 中添加更多调试输出")
    print("  4. 检查 CR3 寄存器的值是否正确")

def main():
    binary_path = "build/kernel.bin"
    
    if not os.path.exists(binary_path):
        print(f"错误: 找不到 {binary_path}")
        print("请先运行 make all")
        return 1
    
    print("="*60)
    print("AntX 内核启动诊断")
    print("="*60)
    
    check_kernel_loading(binary_path)
    check_page_table_setup(binary_path)
    check_kernel_main_code(binary_path)
    check_file_content(binary_path)
    generate_diagnosis_report(binary_path)
    
    return 0

if __name__ == "__main__":
    sys.exit(main())

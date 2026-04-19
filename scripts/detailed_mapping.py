#!/usr/bin/env python3
"""
详细页表映射验证脚本
分析虚拟地址到物理地址的映射
"""

def analyze_address(virt_addr, name=""):
    """分析单个虚拟地址的映射"""
    HIGH_BASE = 0xFFFF800001000000
    
    print(f"\n虚拟地址 0x{virt_addr:016x} ({name}) 的映射分析:")
    
    if virt_addr < HIGH_BASE or virt_addr >= HIGH_BASE + 0x40000000:
        print(f"  地址不在 pd_high 映射范围内!")
        return None
    
    offset = virt_addr - HIGH_BASE
    pd_index = offset // 0x200000
    page_offset = offset % 0x200000
    
    phys_addr = pd_index * 0x200000 + page_offset
    
    print(f"  偏移: 0x{offset:x}")
    print(f"  页目录索引: {pd_index} (0x{pd_index:x})")
    print(f"  页内偏移: 0x{page_offset:x}")
    print(f"  物理地址: 0x{phys_addr:x}")
    
    return phys_addr

def main():
    print("="*60)
    print("AntX 详细页表映射验证")
    print("="*60)
    
    HIGH_BASE = 0xFFFF800001000000
    
    print(f"\npd_high 映射虚拟地址范围: 0x{HIGH_BASE:016x} - 0x{HIGH_BASE + 0x40000000:016x}")
    print(f"映射公式: 物理地址 = 虚拟地址 - 0xFFFF800001000000")
    
    print("\n" + "="*60)
    print("关键地址映射验证")
    print("="*60)
    
    # boot.asm 中的错误地址
    stack_virt_wrong = 0xFFFF8000011701e  # 少了一个 1
    print(f"\nboot.asm 中的栈指针地址: 0x{stack_virt_wrong:016x}")
    stack_phys_wrong = analyze_address(stack_virt_wrong, "boot.asm 中的栈指针")
    
    # 正确的地址
    stack_virt_correct = 0xFFFF80000111701e  # 正确的地址
    print(f"\n正确的栈指针地址: 0x{stack_virt_correct:016x}")
    stack_phys_correct = analyze_address(stack_virt_correct, "正确的栈指针")
    
    kernel_main_virt = 0xFFFF8000011182bb
    kernel_main_phys = analyze_address(kernel_main_virt, "kernel_main")
    
    print("\n" + "="*60)
    print("问题诊断")
    print("="*60)
    
    print(f"\nstack_top 符号物理地址: 0x11701e")
    print(f".bootbss 段物理地址范围: 0x102000 - 0x117036")
    
    if stack_phys_wrong is not None:
        print(f"\nboot.asm 中的栈指针虚拟地址: 0x{stack_virt_wrong:016x}")
        print(f"  映射到物理地址: 0x{stack_phys_wrong:x}")
        if stack_phys_wrong < 0x102000 or stack_phys_wrong >= 0x117036:
            print(f"  这个地址不在 .bootbss 段内! (错误)")
    
    if stack_phys_correct is not None:
        print(f"\n正确的栈指针虚拟地址: 0x{stack_virt_correct:016x}")
        print(f"  映射到物理地址: 0x{stack_phys_correct:x}")
        if 0x102000 <= stack_phys_correct < 0x117036:
            print(f"  这个地址在 .bootbss 段内! (正确)")
    
    print("\n" + "="*60)
    print("修复建议")
    print("="*60)
    
    print(f"\n修改 boot.asm 中的栈指针:")
    print(f"  旧值: mov rsp, qword 0x{stack_virt_wrong:X}")
    print(f"  新值: mov rsp, qword 0x{stack_virt_correct:X}")
    print(f"\n差异: 地址中少了一个 '1'")
    print(f"  旧地址偏移: 0x1701e")
    print(f"  新地址偏移: 0x11701e")
    
    print("\n验证 kernel_main 映射:")
    print(f"  虚拟地址: 0x{kernel_main_virt:016x}")
    print(f"  物理地址: 0x{kernel_main_phys:x}")
    print(f"  .text 段物理地址范围: 0x118000 - 0x13071a")
    
    if 0x118000 <= kernel_main_phys < 0x13071a:
        print("  结论: kernel_main 映射正确")

if __name__ == "__main__":
    main()

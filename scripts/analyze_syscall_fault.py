#!/usr/bin/env python3
"""
Analyze System Calls page fault at address 0x40000000
"""

def analyze_page_fault():
    fault_addr = 0x40000000
    rip = 0x12D1B8
    rsp = 0x116D9E
    rbp = 0x116D9E
    rax = 0x40000000
    rcx = 0x14D940
    
    print("=== Page Fault Analysis ===")
    print(f"Fault Address (CR2): 0x{fault_addr:016X}")
    print(f"RIP: 0x{rip:016X}")
    print(f"RSP: 0x{rsp:016X}")
    print(f"RBP: 0x{rbp:016X}")
    print(f"RAX: 0x{rax:016X}")
    print(f"RCX: 0x{rcx:016X}")
    print()
    
    print("=== Address Analysis ===")
    print(f"Fault address 0x{fault_addr:X} = {fault_addr // (1024*1024)} MB")
    print(f"This is a low address, possibly:")
    print("  - Unmapped user space address")
    print("  - NULL pointer + offset")
    print("  - Physical address used incorrectly")
    print()
    
    print("=== Register Analysis ===")
    print(f"RAX = 0x{rax:X} - Same as fault address!")
    print("  This suggests RAX was used as a pointer")
    print(f"RCX = 0x{rcx:X} - This looks like a kernel data address")
    print()
    
    print("=== Memory Layout Check ===")
    print("Kernel heap: 0xFFFF800001000000 - 0xFFFF800002000000")
    print(f"Fault address 0x{fault_addr:X} is NOT in kernel heap range")
    print()
    
    print("=== Possible Causes ===")
    print("1. memset/memcpy with invalid destination pointer")
    print("2. Writing to a structure field through a NULL pointer")
    print("3. Incorrect pointer arithmetic")
    print("4. Using physical address instead of virtual address")
    print()
    
    print("=== Next Steps ===")
    print("1. Check the code at RIP 0x12D1B8")
    print("2. Look for memset calls in syscall.c or vfs.c")
    print("3. Check if RAX was derived from a NULL pointer")

if __name__ == "__main__":
    analyze_page_fault()

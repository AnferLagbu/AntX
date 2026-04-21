#!/usr/bin/env python3
"""
Analyze ramfs_data structure size and address
"""

def analyze_ramfs():
    RAMFS_MAX_INODES = 64
    RAMFS_MAX_BLOCKS = 256
    RAMFS_BLOCK_SIZE = 512
    
    print("=== RAMFS Configuration ===")
    print(f"RAMFS_MAX_INODES: {RAMFS_MAX_INODES}")
    print(f"RAMFS_MAX_BLOCKS: {RAMFS_MAX_BLOCKS}")
    print(f"RAMFS_BLOCK_SIZE: {RAMFS_BLOCK_SIZE}")
    print()
    
    inode_size = 4 + 4 + 4 + 4 + 4 + 4 + 4 + 4 + 4 + 4 * 12
    print(f"Estimated inode size: {inode_size} bytes")
    
    inodes_size = inode_size * RAMFS_MAX_INODES
    print(f"Inodes array size: {inodes_size} bytes ({inodes_size / 1024:.1f} KB)")
    
    data_area_size = RAMFS_MAX_BLOCKS * RAMFS_BLOCK_SIZE
    print(f"Data area size: {data_area_size} bytes ({data_area_size / 1024:.1f} KB)")
    
    inode_bitmap_size = RAMFS_MAX_INODES // 8
    print(f"Inode bitmap size: {inode_bitmap_size} bytes")
    
    block_bitmap_size = RAMFS_MAX_BLOCKS // 8
    print(f"Block bitmap size: {block_bitmap_size} bytes")
    
    total_size = inodes_size + data_area_size + inode_bitmap_size + block_bitmap_size + 4 + 4 + 4
    print(f"\nTotal ramfs_data size: {total_size} bytes ({total_size / 1024:.1f} KB)")
    
    print()
    print("=== Page Fault Analysis ===")
    fault_addr = 0x40000000
    print(f"Fault address: 0x{fault_addr:X} ({fault_addr / (1024*1024):.0f} MB)")
    
    print()
    print("=== BSS Section Analysis ===")
    print("ramfs_data is a static variable, so it's in BSS section")
    print("BSS section is typically at a low address in the kernel")
    print("But 0x40000000 (1GB) is too high for BSS")
    print()
    
    print("=== Possible Issue ===")
    print("The fault address 0x40000000 is NOT related to ramfs_data")
    print("It's likely from a different source:")
    print("1. A NULL pointer + 0x40000000 offset")
    print("2. An uninitialized pointer")
    print("3. A physical address being used incorrectly")
    
    print()
    print("=== Checking memset calls ===")
    print("Need to find which memset call is using address 0x40000000")

if __name__ == "__main__":
    analyze_ramfs()

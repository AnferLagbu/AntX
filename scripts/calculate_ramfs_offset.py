#!/usr/bin/env python3
"""
Calculate ramfs_data.data_area offset
"""

def calculate_data_area_offset():
    RAMFS_MAX_INODES = 64
    RAMFS_MAX_BLOCKS = 256
    RAMFS_BLOCK_SIZE = 512
    RAMFS_MAX_NAME = 64
    
    print("=== ramfs_inode Structure Layout ===")
    
    inode_offset = 0
    print(f"inode_num:    offset={inode_offset:4d}, size=4 (uint32_t)")
    inode_offset += 4
    
    print(f"type:         offset={inode_offset:4d}, size=4 (uint32_t)")
    inode_offset += 4
    
    print(f"perm:         offset={inode_offset:4d}, size=2 (uint16_t)")
    inode_offset += 2
    
    print(f"padding:      offset={inode_offset:4d}, size=2")
    inode_offset += 2
    
    print(f"size:         offset={inode_offset:4d}, size=4 (uint32_t)")
    inode_offset += 4
    
    print(f"owner_pwid:   offset={inode_offset:4d}, size=8 (uint64_t)")
    inode_offset += 8
    
    print(f"atime:        offset={inode_offset:4d}, size=8 (uint64_t)")
    inode_offset += 8
    
    print(f"mtime:        offset={inode_offset:4d}, size=8 (uint64_t)")
    inode_offset += 8
    
    print(f"ctime:        offset={inode_offset:4d}, size=8 (uint64_t)")
    inode_offset += 8
    
    print(f"link_count:   offset={inode_offset:4d}, size=4 (uint32_t)")
    inode_offset += 4
    
    print(f"used:         offset={inode_offset:4d}, size=1 (uint8_t)")
    inode_offset += 1
    
    print(f"padding:      offset={inode_offset:4d}, size=3")
    inode_offset += 3
    
    print(f"direct_blocks: offset={inode_offset:4d}, size={12*4}=48 (uint32_t[12])")
    inode_offset += 48
    
    print(f"\nTotal inode size: {inode_offset} bytes")
    
    print("\n=== ramfs_data Structure Layout ===")
    
    data_offset = 0
    print(f"inodes:       offset={data_offset:6d}, size={inode_offset * RAMFS_MAX_INODES}")
    data_offset += inode_offset * RAMFS_MAX_INODES
    
    print(f"data_area:    offset={data_offset:6d}, size={RAMFS_MAX_BLOCKS * RAMFS_BLOCK_SIZE}")
    data_area_offset = data_offset
    data_offset += RAMFS_MAX_BLOCKS * RAMFS_BLOCK_SIZE
    
    print(f"inode_bitmap: offset={data_offset:6d}, size={RAMFS_MAX_INODES // 8}")
    data_offset += RAMFS_MAX_INODES // 8
    
    print(f"block_bitmap: offset={data_offset:6d}, size={RAMFS_MAX_BLOCKS // 8}")
    data_offset += RAMFS_MAX_BLOCKS // 8
    
    print(f"root_inode:   offset={data_offset:6d}, size=4")
    data_offset += 4
    
    print(f"free_inodes:  offset={data_offset:6d}, size=4")
    data_offset += 4
    
    print(f"free_blocks:  offset={data_offset:6d}, size=4")
    data_offset += 4
    
    print(f"\nTotal ramfs_data size: {data_offset} bytes ({data_offset / 1024:.1f} KB)")
    
    print("\n=== Address Calculation ===")
    ramfs_data_addr = 0x1aa800
    print(f"ramfs_data address: 0x{ramfs_data_addr:X}")
    print(f"data_area offset: {data_area_offset} bytes")
    print(f"data_area address: 0x{ramfs_data_addr + data_area_offset:X}")
    
    print("\n=== get_block Analysis ===")
    for block_num in [0, 1, 10, 100, 255]:
        block_addr = ramfs_data_addr + data_area_offset + block_num * RAMFS_BLOCK_SIZE
        print(f"get_block({block_num}): 0x{block_addr:X}")
    
    print("\n=== Page Fault Address Analysis ===")
    fault_addr = 0x40000000
    print(f"Fault address: 0x{fault_addr:X}")
    print(f"This is NOT within ramfs_data range!")
    
    print("\n=== Possible Issue ===")
    print("The fault address 0x40000000 is 1GB, which is a typical user space address.")
    print("It might be:")
    print("1. A NULL pointer + 0x40000000 offset")
    print("2. An uninitialized pointer")
    print("3. A physical address being used incorrectly")

if __name__ == "__main__":
    calculate_data_area_offset()

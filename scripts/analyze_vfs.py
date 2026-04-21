#!/usr/bin/env python3
"""
Analyze vfs_file structure layout
"""

def analyze_vfs_file():
    VFS_MAX_PATH = 256
    
    print("=== vfs_file Structure Layout ===")
    
    offset = 0
    print(f"fd:         offset={offset:3d}, size=4 (uint32_t)")
    offset += 4
    
    print(f"inode_num:  offset={offset:3d}, size=4 (uint32_t)")
    offset += 4
    
    print(f"offset:     offset={offset:3d}, size=8 (uint64_t)")
    offset += 8
    
    print(f"flags:      offset={offset:3d}, size=4 (int)")
    offset += 4
    
    print(f"pwid:       offset={offset:3d}, size=8 (uint64_t)")
    offset += 8
    
    print(f"used:       offset={offset:3d}, size=1 (uint8_t)")
    offset += 1
    
    print(f"type:       offset={offset:3d}, size=1 (uint8_t)")
    offset += 1
    
    # Padding for alignment
    offset = ((offset + 7) // 8) * 8
    print(f"(padding):  offset={offset:3d}")
    
    print(f"path:       offset={offset:3d}, size={VFS_MAX_PATH} (char[{VFS_MAX_PATH}])")
    offset += VFS_MAX_PATH
    
    print(f"fs_data:    offset={offset:3d}, size=8 (void*)")
    offset += 8
    
    print(f"private:    offset={offset:3d}, size=8 (void*)")
    offset += 8
    
    print(f"fops:       offset={offset:3d}, size=8 (struct*)")
    offset += 8
    
    print(f"\nTotal size: {offset} bytes")
    
    print("\n=== vfs_fd_table Analysis ===")
    VFS_MAX_FDS = 16
    vfs_fd_table_addr = 0x151720
    
    print(f"vfs_fd_table address: 0x{vfs_fd_table_addr:X}")
    print(f"vfs_fd_table size: {offset * VFS_MAX_FDS} bytes ({offset * VFS_MAX_FDS / 1024:.1f} KB)")
    print(f"vfs_fd_table end: 0x{vfs_fd_table_addr + offset * VFS_MAX_FDS:X}")
    
    print("\n=== Page Fault Address Analysis ===")
    fault_addr = 0x40000000
    print(f"Fault address: 0x{fault_addr:X}")
    print(f"This is NOT within vfs_fd_table range!")
    
    print("\n=== Checking mount->fs->fs_data ===")
    print("file->fs_data = mount->fs->fs_data")
    print("If mount->fs is NULL, then mount->fs->fs_data would be 0x...??")
    print("But we check mount->fs == NULL in vfs_open...")

if __name__ == "__main__":
    analyze_vfs_file()

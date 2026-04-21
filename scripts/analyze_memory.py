#!/usr/bin/env python3
"""
Memory allocation analysis script for AntX kernel debugging.
"""

def analyze_allocation():
    allocation_size = 557056
    page_size = 4096
    heap_header_size = 32
    min_block_size = 16
    align_size = 8
    
    print("=== Memory Allocation Analysis ===")
    print(f"Requested allocation size: {allocation_size} bytes ({allocation_size / 1024:.1f} KB)")
    print(f"Page size: {page_size} bytes ({page_size / 1024:.1f} KB)")
    print(f"Heap header size: {heap_header_size} bytes")
    print()
    
    aligned_size = ((allocation_size + align_size - 1) // align_size) * align_size
    if aligned_size < min_block_size:
        aligned_size = min_block_size
    
    print(f"Aligned allocation size: {aligned_size} bytes ({aligned_size / 1024:.1f} KB)")
    
    total_needed = aligned_size + heap_header_size
    print(f"Total needed (with header): {total_needed} bytes ({total_needed / 1024:.1f} KB)")
    
    pages_needed = (total_needed + page_size - 1) // page_size
    print(f"Pages needed: {pages_needed}")
    
    current_expand_pages = 16
    print(f"Current heap_expand pages: {current_expand_pages} ({current_expand_pages * page_size / 1024:.1f} KB)")
    
    print()
    print("=== Problem Analysis ===")
    if pages_needed > current_expand_pages:
        print(f"WARNING: Need {pages_needed} pages but heap_expand only allocates {current_expand_pages} pages!")
        print(f"Missing pages: {pages_needed - current_expand_pages}")
        print(f"Missing memory: {(pages_needed - current_expand_pages) * page_size} bytes")
    
    print()
    print("=== Solution ===")
    print("Option 1: Increase heap_expand to allocate more pages at once")
    print(f"  Recommended: heap_expand should allocate at least {pages_needed} pages")
    
    print()
    print("Option 2: Loop heap_expand until enough memory is available")
    print("  This is more flexible but may be slower")
    
    print()
    print("=== Calculating optimal expand size ===")
    max_inode_table_size = 4096 * 136
    print(f"Max inode table size (4096 inodes * 136 bytes): {max_inode_table_size} bytes ({max_inode_table_size / 1024:.1f} KB)")
    
    optimal_pages = (max_inode_table_size + page_size - 1) // page_size
    print(f"Optimal pages for max inode table: {optimal_pages}")
    
    recommended_expand = max(16, optimal_pages)
    print(f"Recommended heap_expand pages: {recommended_expand}")

if __name__ == "__main__":
    analyze_allocation()

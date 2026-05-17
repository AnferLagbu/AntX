# 内存管理子系统

> AntX的三层内存管理架构

---

## 🎯 概述

AntX采用三层内存管理架构，从底到顶分别是：

```
┌─────────────────────────────────┐
│        堆管理器         │  ← kmalloc/kfree
├─────────────────────────────────┤
│      虚拟内存管理器      │  ← 页表、映射
├─────────────────────────────────┤
│      物理内存管理器      │  ← 物理页分配
└─────────────────────────────────┘
```

---

## 📦 物理内存管理器 (PMM)

### 职责

- 管理所有物理内存页
- 分配和释放物理页
- 支持连续页分配

### 数据结构

```rust
pub struct Pmm {
    bitmap: Mutex<BitMap>,        // 页分配位图
    total_pages: usize,           // 总页数
    free_pages: AtomicUsize,      // 空闲页数
    page_size: usize,             // 页大小（4KB）
}

impl Pmm {
    /// 分配n个连续物理页
    pub fn alloc(&self, count: usize) -> Option<PhysAddr>
    
    /// 释放n个连续物理页
    pub fn free(&self, addr: PhysAddr, count: usize)
    
    /// 获取空闲页数
    pub fn free_count(&self) -> usize
}
```

### 分配算法

**位图分配算法**:
1. 在位图中查找连续的0位（空闲页）
2. 标记为1（已分配）
3. 返回物理地址

**时间复杂度**: O(n) 查找，O(1) 分配

### 使用示例

```rust
// 分配单个页
let page = PMM.alloc(1).unwrap();

// 分配连续10个页
let pages = PMM.alloc(10).unwrap();

// 释放
PMM.free(page, 1);
PMM.free(pages, 10);
```

---

## 📦 虚拟内存管理器 (VMM)

### 职责

- 管理虚拟地址空间
- 维护页表结构
- 处理页错误

### 页表结构

AntX使用x86_64的四级页表：

```
虚拟地址 (48位)
┌─────────┬─────────┬─────────┬─────────┬──────────┐
│ PML4索引│ PDPT索引│ PD索引  │ PT索引  │ 页内偏移 │
│  (9位)  │  (9位)  │  (9位)  │  (9位)  │  (12位)  │
└─────────┴─────────┴─────────┴─────────┴──────────┘
```

**转换流程**:
```
CR3 → PML4 → PDPT → PD → PT → 物理页
```

### 数据结构

```rust
pub struct Vmm {
    pml4: Mutex<PageTable>,       // PML4表
    kernel_pml4: PhysAddr,        // 内核页表
}

pub struct PageTable {
    entries: [PageTableEntry; 512],
}

pub struct PageTableEntry {
    phys_addr: u64,               // 物理地址
    present: bool,                // 存在位
    writable: bool,               // 可写位
    user: bool,                   // 用户态可访问
    no_execute: bool,             // 不可执行
}
```

### 核心API

```rust
impl Vmm {
    /// 映射虚拟页到物理页
    pub fn map_page(
        &self,
        vaddr: VirtAddr,
        paddr: PhysAddr,
        flags: PageFlags,
    ) -> Result<(), VmmError>
    
    /// 取消映射
    pub fn unmap_page(&self, vaddr: VirtAddr)
    
    /// 虚拟地址转物理地址
    pub fn virt_to_phys(&self, vaddr: VirtAddr) -> Option<PhysAddr>
    
    /// 创建新的地址空间
    pub fn create_address_space() -> Arc<AddressSpace>
}
```

### 使用示例

```rust
// 映射一页
VMM.map_page(
    0xFFFF800000100000,  // 虚拟地址
    0x100000,            // 物理地址
    PageFlags::PRESENT | PageFlags::WRITABLE,
)?;

// 虚拟地址转物理地址
let paddr = VMM.virt_to_phys(0xFFFF800000100000);
```

---

## 📦 堆管理器

### 职责

- 提供动态内存分配
- kmalloc/kfree接口
- 内存池管理

### 实现策略

**二分伙伴系统**:
- 支持2^n大小的分配
- 减少内存碎片
- O(log n)分配时间

**Slab分配器**:
- 针对小对象优化
- 预分配对象缓存
- O(1)分配时间

### 数据结构

```rust
pub struct Heap {
    buddy: Mutex<BuddyAllocator>,    // 伙伴系统
    slabs: [Mutex<SlabAllocator>; 16], // Slab分配器
}

pub struct BuddyAllocator {
    free_lists: [Vec<PhysAddr>; MAX_ORDER],
    base_addr: VirtAddr,
    total_size: usize,
}

pub struct SlabAllocator {
    obj_size: usize,                 // 对象大小
    partial: Vec<SlabPage>,          // 部分满的页
    full: Vec<SlabPage>,             // 满页
    empty: Vec<SlabPage>,            // 空页
}
```

### 核心API

```rust
/// 分配内存
pub fn kmalloc(size: usize) -> *mut u8

/// 释放内存
pub fn kfree(ptr: *mut u8)

/// 分配并初始化为零
pub fn kzalloc(size: usize) -> *mut u8

/// 重新分配
pub fn krealloc(ptr: *mut u8, new_size: usize) -> *mut u8
```

### 使用示例

```rust
// 分配内存
let ptr = kmalloc(1024) as *mut i32;

// 使用
unsafe { *ptr = 42; }

// 释放
kfree(ptr as *mut u8);

// 分配并清零
let ptr = kzalloc(1024);
```

---

## 🔄 内存映射

### 内核地址空间布局

```
0x0000000000000000 - 0x00007FFFFFFFFFFF: 用户空间 (128TB)
0xFFFF800000000000 - 0xFFFF8000000FFFFF: 内核代码 (1MB)
0xFFFF800000100000 - 0xFFFF800001FFFFFF: 内核数据 (32MB)
0xFFFF800002000000 - 0xFFFF800003FFFFFF: 内核堆 (32MB)
0xFFFF800004000000 - 0xFFFF800007FFFFFF: 设备映射 (64MB)
0xFFFF800008000000 - 0xFFFF8000FFFFFFFF: 物理内存映射 (2GB)
0xFFFF800100000000 - 0xFFFF87FFFFFFFFFF: 内核栈等
```

### 映射类型

| 类型 | 说明 | 属性 |
|------|------|------|
| 代码映射 | 内核代码段 | R-X |
| 数据映射 | 内核数据段 | RW- |
| 堆映射 | 动态分配 | RW- |
| 设备映射 | MMIO | RW- (uncached) |
| 物理映射 | 恒等映射 | RW- |

---

## 📊 性能指标

| 操作 | 时间复杂度 | 说明 |
|------|-----------|------|
| PMM分配 | O(n) | 查找连续页 |
| VMM映射 | O(1) | 页表操作 |
| kmalloc | O(log n) | 伙伴系统 |
| Slab分配 | O(1) | 对象缓存 |

---

## 🧪 测试

```rust
#[test_case]
fn test_pmm_alloc_free() {
    let page = PMM.alloc(1).unwrap();
    assert!(!page.is_null());
    PMM.free(page, 1);
}

#[test_case]
fn test_vmm_map_unmap() {
    let vaddr = 0xFFFF800000100000;
    let paddr = PMM.alloc(1).unwrap();
    
    VMM.map_page(vaddr, paddr, PageFlags::WRITABLE).unwrap();
    assert_eq!(VMM.virt_to_phys(vaddr), Some(paddr));
    
    VMM.unmap_page(vaddr);
    assert_eq!(VMM.virt_to_phys(vaddr), None);
}

#[test_case]
fn test_kmalloc_kfree() {
    let ptr = kmalloc(1024);
    assert!(!ptr.is_null());
    kfree(ptr);
}
```

---

**最后更新**: 2026-05-18

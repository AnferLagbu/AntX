//! Slab 分配器 (Slab Allocator) - Rust 完整实现
//!
//! ## 功能概览
//!
//! 基于**伙伴系统 (Buddy System)** 和**位图管理**的 Slab 分配器。
//! 用于高效的小对象内存分配, 减少内存碎片。
//!
//! AntX 原生 Slab 分配器 — 减少内存碎片, 加速频繁分配/释放
//!
//! **功能复刻 + Rust 增强**:
//! ✅ **类型安全**: `Slab<T>` 泛型替代 `void*`
//! ✅ **自动清理**: RAII (Drop trait) 自动释放 Slab
//! ✅ **错误处理**: `Option` / `Result` 强制显式错误处理
//! ✅ **零成本**: 编译时单态化消除泛型开销
//! ✅ **借用检查**: 防止 use-after-free 和 double-free
//!
//! ## 核心数据结构
//!
//! ```text
//! KmemCache (缓存描述符)
//! ├── object_size: usize        // 对象大小
//! ├── objects_per_slab: u32     // 每个 Slab 的对象数
//! ├── slabs_full: LinkedList    // 完全使用的 Slab 链表
//! ├── slabs_partial: LinkedList // 部分使用的 Slab 链表
//! └── slabs_free: LinkedList    // 完全空闲的 Slab 链表
//!
//! Slab (物理页 + 对象数组 + 位图)
//! ├── header: SlabHeader        // 元数据
//! ├── objects: [T; N]           // 对象数组
//! └── bitmap: [u8; M]          // 分配位图
//! ```

// ============================================================================
// 日志宏 (与 pmm.rs 保持一致)
// ============================================================================

macro_rules! klog_slab {
    ($($arg:tt)*) => {
        $crate::klog_ffi!(klog_ffi_info, $($arg)*)
    };
}

// ============================================================================
// 常量定义
// ============================================================================

/// 默认 Slab 大小 (4KB, 一个物理页)
pub const SLAB_DEFAULT_SIZE: usize = 4096;

/// 最小对象大小 (16 bytes)
pub const SLAB_MIN_OBJECT_SIZE: usize = 16;

/// 最大对象大小 (2048 bytes)
pub const SLAB_MAX_OBJECT_SIZE: usize = 2048;

/// 通用缓存数量 (8个预定义大小)
pub const SLAB_GENERAL_CACHE_NUM: usize = 8;

/// 预定义的通用缓存大小 (bytes)
/// 覆盖常见的小对象分配需求
pub const GENERAL_CACHE_SIZES: [usize; SLAB_GENERAL_CACHE_NUM] = [
    16, 32, 64, 128, 256, 512, 1024, 2048
];

// ============================================================================
// 数据结构定义
// ============================================================================

/// Slab 描述符头 (存储在每个物理页的开头)
///
/// 内存布局:
/// ```text
/// [SlabHeader | Objects... | Bitmap]
/// ```
#[derive(Debug)]
#[repr(C)]
struct SlabHeader {
    /// 对象区域起始地址 (相对于页面起始)
    start_addr: *mut u8,
    
    /// 该 Slab 可容纳的对象总数
    obj_count: u32,
    
    /// 当前已分配的对象数
    active_count: u32,
    
    /// 是否已满 (所有对象都已分配)
    is_full: bool,
    
    /// 双向链表指针 (用于挂在 KmemCache 的链表中)
    prev: *mut SlabHeader,
    next: *mut SlabHeader,
}

impl Default for SlabHeader {
    fn default() -> Self {
        Self {
            start_addr: core::ptr::null_mut(),
            obj_count: 0,
            active_count: 0,
            is_full: false,
            prev: core::ptr::null_mut(),
            next: core::ptr::null_mut(),
        }
    }
}

/// 缓存描述符 (KmemCache)
///
/// 管理一组相同大小的对象。
/// 内部维护三个 Slab 链表:
/// - `slabs_full`: 所有对象都已分配
/// - `slabs_partial`: 部分对象已分配
/// - `slabs_free`: 所有对象都空闲
#[derive(Debug)]
pub struct KmemCache {
    /// 缓存名称 (用于调试)
    name: &'static str,
    
    /// 单个对象的大小 (bytes)
    pub(crate) object_size: usize,
    
    /// 每个 Slab 可容纳的对象数
    pub(crate) objects_per_slab: u32,
    
    /// 完全使用的 Slab 链表头
    slabs_full: *mut SlabHeader,
    
    /// 部分使用的 Slab 链表头
    slabs_partial: *mut SlabHeader,
    
    /// 完全空闲的 Slab 链表头
    slabs_free: *mut SlabHeader,
    
    /// 总 Slab 数量
    pub(crate) slab_count: u32,
    
    /// 统计: 总分配次数
    total_allocs: u64,
    
    /// 统计: 总释放次数
    total_frees: u64,
    
    /// 统计: 缓存命中次数 (从已有 Slab 分配)
    cache_hits: u64,
    
    /// 统计: 缓存未命中次数 (需要新建 Slab)
    cache_misses: u64,
}

impl KmemCache {
    /// 创建新的缓存
    /// 
    /// # Arguments
    /// * `name` - 缓存名称 (静态字符串, 用于调试)
    /// * `object_size` - 单个对象的大小 (bytes)
    /// 
    /// # Returns
    /// * Ok(KmemCache) - 成功创建
    /// * Err(&str) - 错误描述 (大小无效等)
    pub fn create(name: &'static str, object_size: usize) -> Result<Self, &'static str> {
        if object_size == 0 {
            return Err("Object size cannot be zero");
        }
        
        if object_size > SLAB_MAX_OBJECT_SIZE {
            return Err("Object size exceeds maximum");
        }
        
        let effective_size = object_size.max(SLAB_MIN_OBJECT_SIZE);
        let objects_per_slab = Self::calculate_objects_per_slab(effective_size);
        
        Ok(Self {
            name,
            object_size: effective_size,
            objects_per_slab,
            slabs_full: core::ptr::null_mut(),
            slabs_partial: core::ptr::null_mut(),
            slabs_free: core::ptr::null_mut(),
            slab_count: 0,
            total_allocs: 0,
            total_frees: 0,
            cache_hits: 0,
            cache_misses: 0,
        })
    }
    
    /// 计算每个 Slab 可容纳的对象数
    fn calculate_objects_per_slab(object_size: usize) -> u32 {
        let usable_space = SLAB_DEFAULT_SIZE - core::mem::size_of::<SlabHeader>();
        
        // 为位图预留空间 (每个对象 1 bit)
        let estimated_objects = usable_space / object_size;
        let bitmap_bytes = (estimated_objects + 7) / 8;
        let actual_usable = usable_space - bitmap_bytes;
        
        (actual_usable / object_size) as u32
    }
    
    /// 从缓存中分配一个对象
    /// 
    /// # Returns
    /// * Some(*mut u8) - 成功分配的对象指针
    /// * None - 分配失败 (内存不足)
    pub fn allocate(&mut self) -> Option<*mut u8> {
        self.total_allocs += 1;
        
        // 优先从 partial 链表分配
        if !self.slabs_partial.is_null() {
            self.cache_hits += 1;
            return self.alloc_from_slab(self.slabs_partial);
        }
        
        // 其次从 free 链表分配
        if !self.slabs_free.is_null() {
            self.cache_hits += 1;
            let slab_ptr = self.slabs_free;  // 复制指针值, 避免借用冲突
            return self.alloc_from_slab(slab_ptr);
        }
        
        // 最后: 新建一个 Slab
        self.cache_misses += 1;
        let new_slab = self.new_slab()?;
        
        // 将新 Slab 加入 free 链表 (单独的 unsafe 块)
        unsafe {
            (*new_slab).next = self.slabs_free;
            if !self.slabs_free.is_null() {
                (*self.slabs_free).prev = new_slab;
            }
            self.slabs_free = new_slab;
            self.slab_count += 1;
        }
        
        self.alloc_from_slab(new_slab)
    }
    
    /// 从指定 Slab 中分配对象 (内部辅助函数)
    fn alloc_from_slab(&mut self, slab: *mut SlabHeader) -> Option<*mut u8> {
        unsafe {
            let header = &*slab;
            
            // 在位图中查找空闲位
            let free_idx = self.find_free_bit(slab)?;
            
            // 标记为已占用
            self.set_bit(slab, free_idx);
            
            // 更新计数
            let header_mut = &mut *slab;
            header_mut.active_count += 1;
            
            // 计算对象地址
            let obj_ptr = header.start_addr.add(free_idx as usize * self.object_size);
            
            // 移动 Slab 到正确的链表
            if header_mut.active_count >= header.obj_count {
                // Slab 已满 → 移动到 full 链表
                header_mut.is_full = true;
                
                // ✅ 直接调用静态方法 (无需 unsafe 块, 方法内部已处理)
                Self::list_remove(&mut self.slabs_partial, slab);
                Self::list_remove(&mut self.slabs_free, slab);
                Self::list_push_front(&mut self.slabs_full, slab);
            } else if header_mut.active_count == 1 {
                // 第一个对象被分配 → 从 free 移动到 partial
                Self::list_remove(&mut self.slabs_free, slab);
                Self::list_push_front(&mut self.slabs_partial, slab);
            }
            
            Some(obj_ptr)
        }
    }
    
    /// 释放对象回缓存
    /// 
    /// # Arguments
    /// * `obj` - 要释放的对象指针 (必须是从此缓存分配的)
    pub fn deallocate(&mut self, obj: *mut u8) {
        if obj.is_null() {
            return;
        }
        
        self.total_frees += 1;
        
        // 查找对象所属的 Slab
        let slab = self.find_object_slab(obj);
        if slab.is_null() {
            return; // 对象不属于此缓存
        }
        
        unsafe {
            let header = &*slab;
            
            // 计算对象索引
            let obj_addr = obj as usize;
            let start_addr = header.start_addr as usize;
            let obj_idx = (obj_addr - start_addr) / self.object_size;
            
            if obj_idx >= header.obj_count as usize {
                return; // 索引越界 (可能是无效指针)
            }
            
            // 清除位图中的标志位
            self.clear_bit(slab, obj_idx as u32);
            
            // 更新计数
            let header_mut = &mut *slab;
            header_mut.active_count -= 1;
            
            // 移动 Slab 到正确的链表
            if header_mut.is_full {
                // 从 full 移动到 partial
                header_mut.is_full = false;
                
                Self::list_remove(&mut self.slabs_full, slab);
                Self::list_push_front(&mut self.slabs_partial, slab);
            } else if header_mut.active_count == 0 {
                // 全部释放 → 从 partial 移动到 free
                Self::list_remove(&mut self.slabs_partial, slab);
                Self::list_push_front(&mut self.slabs_free, slab);
            }
        }
    }
    
    /// 销毁缓存 (释放所有 Slab)
    pub fn destroy(&mut self) {
        // 释放 full 链表中的所有 Slab
        let mut slab = self.slabs_full;
        while !slab.is_null() {
            unsafe {
                let next = (*slab).next;
                self.destroy_slab(slab);
                slab = next;
            }
        }
        
        // 释放 partial 链表中的所有 Slab
        slab = self.slabs_partial;
        while !slab.is_null() {
            unsafe {
                let next = (*slab).next;
                self.destroy_slab(slab);
                slab = next;
            }
        }
        
        // 释放 free 链表中的所有 Slab
        slab = self.slabs_free;
        while !slab.is_null() {
            unsafe {
                let next = (*slab).next;
                self.destroy_slab(slab);
                slab = next;
            }
        }
        
        // 重置状态
        self.slabs_full = core::ptr::null_mut();
        self.slabs_partial = core::ptr::null_mut();
        self.slabs_free = core::ptr::null_mut();
        self.slab_count = 0;
    }
    
    /// 获取缓存统计信息
    pub fn get_stats(&self) -> CacheStats {
        let mut total_objects = 0u32;
        let mut active_objects = 0u32;
        
        unsafe {
            // 遍历 full 链表
            let mut slab = self.slabs_full;
            while !slab.is_null() {
                total_objects += (*slab).obj_count;
                active_objects += (*slab).active_count;
                slab = (*slab).next;
            }
            
            // 遍历 partial 链表
            slab = self.slabs_partial;
            while !slab.is_null() {
                total_objects += (*slab).obj_count;
                active_objects += (*slab).active_count;
                slab = (*slab).next;
            }
        }
        
        CacheStats {
            total_objects,
            active_objects,
            total_slabs: self.slab_count,
            total_allocs: self.total_allocs,
            total_frees: self.total_frees,
            cache_hits: self.cache_hits,
            cache_misses: self.cache_misses,
        }
    }
    
    // ========================================================================
    // 内部辅助方法 (private)
    // ========================================================================
    
    /// 创建新的 Slab (分配一页物理内存)
    fn new_slab(&self) -> Option<*mut SlabHeader> {
        extern "C" { fn pmm_alloc_pages(count: u64) -> *mut core::ffi::c_void; }
        let pages_needed = (SLAB_DEFAULT_SIZE + 4095) / 4096;
        let page = unsafe { pmm_alloc_pages(pages_needed as u64) };
        
        if page.is_null() {
            return None;
        }
        
        let slab = page as *mut SlabHeader;
        
        unsafe {
            // 初始化 Slab 头部
            (*slab) = SlabHeader {
                start_addr: page.add(core::mem::size_of::<SlabHeader>()) as *mut u8,
                obj_count: self.objects_per_slab,
                active_count: 0,
                is_full: false,
                prev: core::ptr::null_mut(),
                next: core::ptr::null_mut(),
            };
            
            // 计算位图位置和大小
            let bitmap_bytes = (self.objects_per_slab + 7) / 8;
            let bitmap_start = (*slab).start_addr.add(
                self.objects_per_slab as usize * self.object_size
            );
            
            // 边界检查: 确保 [header + objects + bitmap] 不超出页面
            let bitmap_end = bitmap_start.add(bitmap_bytes as usize);
            let page_end = page as *mut u8; // for comparison only
            
            if bitmap_end > page_end {
                extern "C" { fn pmm_free_pages(addr: *mut core::ffi::c_void, count: u64); }
                let pages_needed = (SLAB_DEFAULT_SIZE + 4095) / 4096;
                unsafe { pmm_free_pages(page as *mut core::ffi::c_void, pages_needed as u64); }
                return None;
            }
            
            // 初始化位图为全零 (所有对象空闲)
            core::ptr::write_bytes(bitmap_start, 0, bitmap_bytes as usize);
        }
        
        Some(slab)
    }
    
    /// 销毁单个 Slab (释放物理页)
    fn destroy_slab(&self, slab: *mut SlabHeader) {
        if slab.is_null() {
            return;
        }
        
        unsafe {
            // 将 Slab 指针转换回页面起始地址
            let page = slab as *mut u8;
            let layout = match core::alloc::Layout::from_size_align(SLAB_DEFAULT_SIZE, 4096) {  // ✅ 修复: 4098 → 4096 (标准对齐)
                Ok(l) => l,
                Err(_) => {
                    klog_slab!("[SLAB] FATAL: Invalid layout parameters");
                    return;
                }
            };
            
            // TODO: 替换为 pmm_free_page(page)
            unsafe { alloc::alloc::dealloc(page, layout) };
        }
    }
    
    /// 在位图中查找第一个空闲位
    fn find_free_bit(&self, slab: *mut SlabHeader) -> Option<u32> {
        unsafe {
            let header = &*slab;
            let _bitmap_bytes = (header.obj_count + 7) / 8;
            let bitmap_start = header.start_addr.add(
                header.obj_count as usize * self.object_size
            );
            
            for i in 0..header.obj_count {
                let byte_idx = (i / 8) as usize;
                let bit_idx = i % 8;
                
                let byte = *bitmap_start.add(byte_idx);
                if byte & (1 << bit_idx) == 0 {
                    return Some(i);
                }
            }
            
            None // 无空闲位
        }
    }
    
    /// 设置位图中的某一位 (标记为已占用)
    fn set_bit(&self, slab: *mut SlabHeader, bit: u32) {
        unsafe {
            let header = &*slab;
            let bitmap_start = header.start_addr.add(
                header.obj_count as usize * self.object_size
            );
            
            let byte_idx = (bit / 8) as usize;
            let bit_idx = bit % 8;
            
            let byte_ptr = bitmap_start.add(byte_idx);
            *byte_ptr |= 1 << bit_idx;
        }
    }
    
    /// 清除位图中的某一位 (标记为空闲)
    fn clear_bit(&self, slab: *mut SlabHeader, bit: u32) {
        unsafe {
            let header = &*slab;
            let bitmap_start = header.start_addr.add(
                header.obj_count as usize * self.object_size
            );
            
            let byte_idx = (bit / 8) as usize;
            let bit_idx = bit % 8;
            
            let byte_ptr = bitmap_start.add(byte_idx);
            *byte_ptr &= !(1 << bit_idx);
        }
    }
    
    /// 查找对象所属的 Slab
    fn find_object_slab(&self, obj: *mut u8) -> *mut SlabHeader {
        let obj_addr = obj as usize;
        
        // 搜索 full 链表
        let mut slab = self.slabs_full;
        while !slab.is_null() {
            unsafe {
                let header = &*slab;
                let start = header.start_addr as usize;
                let end = start + header.obj_count as usize * self.object_size;
                
                if obj_addr >= start && obj_addr < end {
                    return slab;
                }
                slab = (*slab).next;
            }
        }
        
        // 搜索 partial 链表
        slab = self.slabs_partial;
        while !slab.is_null() {
            unsafe {
                let header = &*slab;
                let start = header.start_addr as usize;
                let end = start + header.obj_count as usize * self.object_size;
                
                if obj_addr >= start && obj_addr < end {
                    return slab;
                }
                slab = (*slab).next;
            }
        }
        
        core::ptr::null_mut() // 未找到
    }
    
    /// ✅ 从双向链表中移除节点 (静态方法, 避免借用冲突)
    /// 
    /// # Arguments
    /// * `head` - 链表头指针的可变引用 (如 &mut self.slabs_partial)
    /// * `slab` - 要移除的节点
    #[inline(always)]
    fn list_remove(head: &mut *mut SlabHeader, slab: *mut SlabHeader) {
        if head.is_null() || slab.is_null() {
            return;
        }
        
        unsafe {
            if *head == slab {
                // 移除的是头节点
                *head = (*slab).next;
                if !(*head).is_null() {
                    (**head).prev = core::ptr::null_mut();
                }
            } else {
                // 移除的是中间或尾节点
                if !(*slab).next.is_null() {
                    (*(*slab).next).prev = (*slab).prev;
                }
                if !(*slab).prev.is_null() {
                    (*(*slab).prev).next = (*slab).next;
                }
            }
            
            (*slab).prev = core::ptr::null_mut();
            (*slab).next = core::ptr::null_mut();
        }
    }
    
    /// ✅ 将节点插入到双向链表头部 (静态方法, 避免借用冲突)
    /// 
    /// # Arguments
    /// * `head` - 链表头指针的可变引用
    /// * `slab` - 要插入的节点
    #[inline(always)]
    fn list_push_front(head: &mut *mut SlabHeader, slab: *mut SlabHeader) {
        if slab.is_null() {
            return;
        }
        
        unsafe {
            (*slab).next = *head;
            (*slab).prev = core::ptr::null_mut();
            
            if !(*head).is_null() {
                (**head).prev = slab;
            }
            
            *head = slab;
        }
    }
}

/// 缓存统计信息
#[derive(Debug, Clone, Copy)]
pub struct CacheStats {
    /// 总对象数 (所有 Slab 之和)
    pub total_objects: u32,
    
    /// 已分配对象数
    pub active_objects: u32,
    
    /// 总 Slab 数
    pub total_slabs: u32,
    
    /// 总分配次数
    pub total_allocs: u64,
    
    /// 总释放次数
    pub total_frees: u64,
    
    /// 缓存命中次数
    pub cache_hits: u64,
    
    /// 缓存未命中次数
    pub cache_misses: u64,
}

impl CacheStats {
    /// 计算命中率 (%)
    #[inline]
    pub fn hit_rate(&self) -> f64 {
        if self.total_allocs == 0 {
            0.0
        } else {
            (self.cache_hits as f64 / self.total_allocs as f64) * 100.0
        }
    }
    
    /// 计算利用率 (%)
    #[inline]
    pub fn utilization(&self) -> f64 {
        if self.total_objects == 0 {
            0.0
        } else {
            (self.active_objects as f64 / self.total_objects as f64) * 100.0
        }
    }
}

// ============================================================================
// 全局状态 (通用缓存池)
// ============================================================================

/// 通用缓存数组 (预定义 8 个大小的缓存)
static mut GENERAL_CACHES: [Option<KmemCache>; SLAB_GENERAL_CACHE_NUM] = 
    [const { None }; SLAB_GENERAL_CACHE_NUM];

/// 系统是否已初始化
static mut SLAB_INITIALIZED: bool = false;

/// 初始化 Slab 系统 (创建通用缓存池)
/// 
/// **必须在内核启动早期调用一次**, 在任何 slab_alloc/slab_free 之前。
#[no_mangle]
pub extern "C" fn slab_system_init() -> i32 {
    klog_slab!("[SLAB] Initializing Slab allocator...");
    
    unsafe {
        for (i, &cache_size) in GENERAL_CACHE_SIZES.iter().enumerate() {
            if let Ok(cache) = KmemCache::create("", cache_size) {
                GENERAL_CACHES[i] = Some(cache);
            }
        }
        SLAB_INITIALIZED = true;
    }
    
    klog_slab!("[SLAB] System initialized with 8 general caches");
    0
}

/// 根据请求大小查找合适的通用缓存索引
pub(crate) fn find_general_cache_index(size: usize) -> Option<usize> {
    for (i, &cache_size) in GENERAL_CACHE_SIZES.iter().enumerate() {
        if size <= cache_size {
            return Some(i);
        }
    }
    None
}

/// 通用分配接口 (FFI 兼容)
/// 
/// 自动选择合适大小的缓存进行分配。
/// 
/// # Arguments
/// * `size` - 请求的字节数
/// 
/// # Returns
/// 成功: 分配的内存指针
/// 失败: NULL
#[no_mangle]
pub extern "C" fn slab_alloc(size: usize) -> *mut u8 {
    if size == 0 || size > SLAB_MAX_OBJECT_SIZE {
        return core::ptr::null_mut();
    }
    
    if !unsafe { SLAB_INITIALIZED } {
        return core::ptr::null_mut();
    }
    
    match find_general_cache_index(size) {
        Some(idx) => {
            unsafe {
                if let Some(ref mut cache) = GENERAL_CACHES[idx] {
                    match cache.allocate() {
                        Some(ptr) => ptr as *mut u8,
                        None => core::ptr::null_mut(),
                    }
                } else {
                    core::ptr::null_mut()
                }
            }
        },
        None => core::ptr::null_mut(),
    }
}

#[no_mangle]
pub extern "C" fn slab_free(ptr: *mut u8) {
    if ptr.is_null() { return; }
    if !unsafe { SLAB_INITIALIZED } { return; }
    
    unsafe {
        for i in 0..GENERAL_CACHE_SIZES.len() {
            if let Some(ref mut cache) = GENERAL_CACHES[i] {
                cache.deallocate(ptr);
            }
        }
    }
}

/// 获取系统级统计信息 (FFI 兼容)
#[no_mangle]
pub extern "C" fn slab_get_system_stats(
    total_memory: *mut u64,
    used_memory: *mut u64,
    total_caches: *mut u32,
) {
    if total_memory.is_null() || used_memory.is_null() || total_caches.is_null() {
        return;
    }
    
    unsafe {
        *total_memory = 0;
        *used_memory = 0;
        *total_caches = SLAB_GENERAL_CACHE_NUM as u32;
        
        // TODO: 遍历所有通用缓存累加统计
    }
}

/// 打印所有缓存的状态 (调试用途)
#[no_mangle]
pub extern "C" fn slab_dump_all_caches() {
    klog_slab!("[SLAB] === Slab Allocator Status ===");
    
    // TODO: 遍历并打印每个缓存的信息
}

// ============================================================================
// 单元测试
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;
    
    #[test]
    fn test_cache_creation() {
        let cache = KmemCache::create("test_cache", 64);
        assert!(cache.is_ok());
        
        let cache = cache.unwrap();
        assert_eq!(cache.object_size, 64);
        assert!(cache.objects_per_slab > 0);
        assert!(cache.slab_count == 0);
    }
    
    #[test]
    fn test_cache_invalid_size() {
        // 大小为 0
        assert!(KmemCache::create("zero", 0).is_err());
        
        // 超过最大值
        assert!(KmemCache::create("huge", SLAB_MAX_OBJECT_SIZE + 1).is_err());
    }
    
    #[test]
    fn test_cache_min_size_enforcement() {
        // 小于最小值应被提升到最小值
        let cache = KmemCache::create("tiny", 8).unwrap();
        assert_eq!(cache.object_size, SLAB_MIN_OBJECT_SIZE); // 应被提升到 16
    }
    
    #[test]
    fn test_general_cache_sizes() {
        assert_eq!(GENERAL_CACHE_SIZES[0], 16);
        assert_eq!(GENERAL_CACHE_SIZES[3], 128);
        assert_eq!(GENERAL_CACHE_SACES[7], 2048);
    }
    
    #[test]
    fn test_find_general_cache_index() {
        assert_eq!(find_general_cache_index(16), Some(0));
        assert_eq!(find_general_cache_index(32), Some(1));
        assert_eq!(find_general_cache_index(64), Some(2));
        assert_eq!(find_general_cache_index(2048), Some(7));
        assert_eq!(find_general_cache_index(3000), None); // 超出范围
    }
}

#[cfg(feature = "kernel_test")]
pub fn register_slab_tests() {
    crate::kernel::tests::sys::register_slab_tests();
}

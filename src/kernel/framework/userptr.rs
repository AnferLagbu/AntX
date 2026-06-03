//! UserPtr — 安全封装用户态裸指针的读写 (TCB)
//!
//! ## 设计目标
//!
//! 在框内核架构中，C-ABI 入口 (`#[no_mangle] fn`) 接收来自 syscall 层或用户态的裸指针。
//! 这些裸指针必须在 TCB (framework) 中转换为安全的 Rust 类型，然后传递给 services 层。
//!
//! `UserReadPtr` / `UserWritePtr` 封装 `unsafe { from_raw_parts }` 操作，
//! 将不安全转换集中在 framework，对外暴露安全 API。
//!
//! ## 与 Asterinas OSTD 的关系
//!
//! Asterinas 的 `UserSpace` trait 提供 `read_val`/`write_val` 等方法，
//! 本模块是轻量等价物：仅做指针→切片的生命周期安全转换，不主动拷贝数据。
//!
//! ## 安全契约（调用方责任）
//!
//! 构造 `UserReadPtr` / `UserWritePtr` 仍需要 `unsafe`，调用方必须确保：
//! 1. 指针指向有效的用户态虚拟地址
//! 2. 地址范围内存已映射且可读（/可写）
//! 3. 在切片使用期间，该内存不会被释放或重新映射
//!
//! 在 C-ABI 层级（syscall dispatcher → VFS api），这些条件由 asm stub + 页表机制保证。
//!
//! ## 使用示例
//!
//! ```rust,ignore
//! // 当前 (unsafe 散布):
//! let buf_slice = unsafe { core::slice::from_raw_parts_mut(buf, count as usize) };
//! fs.read(fd, buf_slice, offset);
//!
//! // 框内核 (unsafe 集中在构造):
//! let mut user_buf = unsafe { UserWritePtr::new(buf, count as usize) };
//! fs.read(fd, user_buf.as_mut_slice(), offset);
//! ```

use core::slice;

/// 用户态只读字节指针
///
/// 封装 `*const u8` → `&[u8]` 的 unsafe 转换。
/// 构造时 unsafe，使用 (`as_slice()`) 时 safe。
pub struct UserReadPtr {
    ptr: *const u8,
    len: usize,
}

impl UserReadPtr {
    /// 从裸指针构造。
    ///
    /// # Safety
    ///
    /// 调用方必须确保 `ptr` 指向至少 `len` 字节的有效、已映射、可读的用户态内存，
    /// 且在返回的 `UserReadPtr` 存活期间该内存不会被释放或重新映射。
    pub unsafe fn new(ptr: *const u8, len: usize) -> Self {
        Self { ptr, len }
    }

    /// 以不可变字节切片形式访问用户内存。
    ///
    /// 此方法是安全的，因为构造时的 `unsafe` 契约已保证了指针有效性。
    pub fn as_slice(&self) -> &[u8] {
        // SAFETY: 构造时的 unsafe 契约保证 ptr + len 是有效的用户态内存
        unsafe { slice::from_raw_parts(self.ptr, self.len) }
    }

    /// 检查指针是否为空。
    pub fn is_null(&self) -> bool {
        self.ptr.is_null()
    }

    /// 获取字节长度。
    pub fn len(&self) -> usize {
        self.len
    }

    /// 长度为零
    #[inline(always)]
    pub fn is_empty(&self) -> bool {
        self.len == 0
    }
}

/// 用户态可写字节指针
///
/// 封装 `*mut u8` → `&mut [u8]` 的 unsafe 转换。
/// 构造时 unsafe，使用 (`as_mut_slice()`) 时 safe。
pub struct UserWritePtr {
    ptr: *mut u8,
    len: usize,
}

impl UserWritePtr {
    /// 从裸指针构造。
    ///
    /// # Safety
    ///
    /// 调用方必须确保 `ptr` 指向至少 `len` 字节的有效、已映射、可写的用户态内存，
    /// 且在返回的 `UserWritePtr` 存活期间该内存不会被释放或重新映射。
    pub unsafe fn new(ptr: *mut u8, len: usize) -> Self {
        Self { ptr, len }
    }

    /// 以可变字节切片形式访问用户内存。
    ///
    /// 此方法是安全的，因为构造时的 `unsafe` 契约已保证了指针有效性。
    pub fn as_mut_slice(&mut self) -> &mut [u8] {
        // SAFETY: 构造时的 unsafe 契约保证 ptr + len 是有效的可写用户态内存
        unsafe { slice::from_raw_parts_mut(self.ptr, self.len) }
    }

    /// 以不可变字节切片形式访问用户内存。
    pub fn as_slice(&self) -> &[u8] {
        // SAFETY: 构造时的 unsafe 契约保证 ptr + len 是有效的用户态内存
        unsafe { slice::from_raw_parts(self.ptr as *const u8, self.len) }
    }

    /// 检查指针是否为空。
    pub fn is_null(&self) -> bool {
        self.ptr.is_null()
    }

    /// 获取字节长度。
    pub fn len(&self) -> usize {
        self.len
    }

    /// 长度为零
    #[inline(always)]
    pub fn is_empty(&self) -> bool {
        self.len == 0
    }
}

/// 用户态结构体指针（可写）
///
/// 封装 `*mut T` → `&mut T` 的 unsafe 转换，用于 stat/fstat/getdents 等
/// 需要向用户态 struct 写入完整结构的场景。
pub struct UserRefMut<T> {
    ptr: *mut T,
}

impl<T> UserRefMut<T> {
    /// 从裸指针构造。
    ///
    /// # Safety
    ///
    /// 调用方必须确保 `ptr` 指向有效的、已映射、可写的用户态 `T` 实例。
    pub unsafe fn new(ptr: *mut T) -> Self {
        Self { ptr }
    }

    /// 获取对用户态结构体的可变引用。
    ///
    /// 此方法是安全的，因为构造时的 `unsafe` 契约已保证了指针有效性。
    #[allow(clippy::should_implement_trait)]
    pub fn as_mut(&mut self) -> &mut T {
        // SAFETY: 构造时的 unsafe 契约保证 ptr 是有效的可写 T
        unsafe { &mut *self.ptr }
    }

    /// 检查指针是否为空。
    pub fn is_null(&self) -> bool {
        self.ptr.is_null()
    }
}
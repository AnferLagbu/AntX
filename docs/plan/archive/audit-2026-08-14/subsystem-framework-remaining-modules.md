# framework 剩余模块（constants/console/io/alloc/link/lib）深度审计报告

> **审计范围**：`src/kernel/framework/` 中5 个剩余模块（constants/console/io/alloc/link/lib）
> **审计日期**：2026-08-14
> **代码规模**：约 2,727 LoC
> **总体结论**：✅ 含 unsafe（TCB，**符合 F4 SAFETY 100% 覆盖**）/ ⚠️ **19 个问题（P0×4, P1×6, P2×6, P3×3）**

## 1. 子系统概览

| 子模块 | 文件数 | LoC | 主要职责 | 风险等级 |
|---|---:|---:|---|---|
| framework/constants/ | 2 | 40 | TCB 内部容量上限 | 中 |
| framework/console/ | 2 | 369 | 图形控制台（gfx_console）| **高** |
| framework/io/ | 2 | 21 | io_uring re-export 桩 | 低 |
| framework/alloc/ | 3 | 193 | Frame/Slab 分配器 trait | **高** |
| framework/lib/ | 3 | 1,367 | 字符串 + 内存 + CStr | **高** |
| framework/link/ | 0 | 0 | 空目录 | — |

## 2. 严重问题

### 2.1 [P0] `lib/string.rs:1112` 单文件 1112 行字符串/内存操作——**违反简单优先**

- **位置**：[string.rs:1-1112](file:///home/anfer/QueenX/src/kernel/framework/lib/string.rs)（实际路径）
- **问题**：
  - 单文件 1112 行包含：strlen/strcmp/strcpy/strcat/strchr/strrchr/strstr + memcpy/memmove/memset/memcmp/memchr/secure_zero + 全部 FFI 包装。
  - 应拆分为 `lib/string.rs` + `lib/memory.rs` + `lib/ffi.rs`。
- **建议方案**：
  1. 拆分为 3 个模块。

### 2.2 [P0] `console/gfx_console.rs:22` `fb: *mut Framebuffer` 裸指针——**生命周期未文档化**

- **位置**：[gfx_console.rs:21-32](file:///home/anfer/Code/QueenX/src/kernel/framework/console/gfx_console.rs#L21-L32)
- **代码**：
  ```rust
  pub struct GfxConsole {
      fb: *mut Framebuffer,
      font: &'static Font,
      cursor_x: u32,
      ...
  }
  ```
- **问题**：
  - `fb: *mut Framebuffer` 裸指针——`unsafe { &*fb }` 在 `new()` 与 `fb_mut()` 中。
  - GfxConsole 生命周期与 Framebuffer 不绑定。
  - Framebuffer 释放后 GfxConsole 仍存在 → use-after-free。
- **建议方案**：
  1. 改用 `NonNull<Framebuffer>` + PhantomData 标注。

### 2.3 [P0] `lib/string.rs:48-67` `strlen(s: *const i8)` 无界循环——**恶意指针可触发任意内存读**

- **位置**：[string.rs:47-67](file:///home/anfer/Code/QueenX/src/kernel/framework/lib/string.rs)（实际）
- **代码**：
  ```rust
  pub unsafe extern "C" fn strlen(s: *const i8) -> usize {
      unsafe {
          if s.is_null() { return 0; }
          let mut len = 0;
          let mut ptr = s;
          while *ptr != 0 {  // ← 无上界循环
              len += 1;
              ptr = ptr.add(1);
          }
          len
      }
  }
  ```
- **问题**：
  - 无 `MAX_CSTR_LEN` 上限（[cstr.rs:53-60](file:///home/anfer/Code/QueenX/src/kernel/framework/lib/cstr.rs#L53-L60)）已有上限但 `strlen` 未用。
  - FFI 端传恶意指针 → 内核无限读取内存 → 触发 #PF 或读到敏感数据。
- **建议方案**：
  1. 添加 `if len > MAX_CSTR_LEN { break }`。

### 2.4 [P0] `console/gfx_console.rs:36-37` `new()` 中 `unsafe { &*fb }` 立即解引用

- **位置**：[gfx_console.rs:34-52](file:///home/anfer/Code/QueenX/src/kernel/framework/console/gfx_console.rs#L34-L52)
- **代码**：
  ```rust
  pub fn new(fb: *mut Framebuffer, font: &'static Font) -> Self {
      let fb_ref = unsafe { &*fb };   // ← 立即解引用
      let cols = fb_ref.width() / font.glyph_width;
      ...
  }
  ```
- **问题**：
  - 构造时立即解引用裸指针（仅读 `width`/`height`）。
  - 若 `fb` 是 dangling 指针 → 即时 UB。
  - 之后 `self.fb` 持有裸指针，**再次解引用 `fb_mut`** 时也可能 dangling。
- **建议方案**：
  1. 改为 `&'static Framebuffer` 借用。
  2. 或使用 `NonNull<Framebuffer>`。

## 3. P1 问题

### 3.1 [P1] `lib/string.rs:1112` `memcpy/memmove/memset` 在 Rust safe API 中存在但**未使用 FFI 函数**

- **位置**：[string.rs](file:///home/anfer/Code/QueenX/src/kernel/framework/lib/string.rs)
- **问题**：
  - FFI 版本与 safe 版本并存——可能调用方误用 unsafe 版本。

### 3.2 [P1] `alloc/frame_alloc.rs:57-58` `unsafe { Some(Frame::from_raw(phys, 0)) }` ——**Frame 引用泄漏**

- **位置**：[alloc/frame_alloc.rs:51-60](file:///home/anfer/Code/QueenX/src/kernel/framework/alloc/frame_alloc.rs#L51-L60)
- **问题**：
  - `Frame::from_raw(phys, 0)` 是 unsafe——但调用方通过 safe trait API 调用。
  - `Frame` 是 `!Send`（之前审计 [subsystem-framework-toplevel.md §2.7](../audit/subsystem-framework-toplevel.md) 已识别）。
  - 多线程并发 alloc → 多个 Frame 持有同一 phys → 引用计数 race。

### 3.3 [P1] `lib/cstr.rs:189` `MAX_CSTR_LEN = 4096` 仅适用于 VFS 路径——其他场景需更大上限

- **位置**：[cstr.rs:53-60](file:///home/anfer/Code/QueenX/src/kernel/framework/lib/cstr.rs#L53-L60)
- **问题**：
  - 注释（[cstr.rs:54-58](file:///home/anfer/Code/QueenX/src/kernel/framework/lib/cstr.rs#L54-L58)）承认 4KB 选自 VFS 默认。
  - cred `note: *const u8` 可能更长（POSIX `LOGIN_NAME_MAX=256`）——但路径可能超 4KB（罕见但可能）。
- **建议方案**：
  1. 调用方传 `max_len` 参数。

### 3.4 [P1] `console/gfx_console.rs:1-298` 帧缓冲设备**未深审 Framebuffer 依赖**

- **位置**：[gfx_console.rs:1](file:///home/anfer/Code/QueenX/src/kernel/framework/console/gfx_console.rs#L1)
- **问题**：
  - `Framebuffer` 来自 `framework::driver`。
  - 依赖 [subsystem-driver.md](../audit/subsystem-driver.md) 的 framebuffer 驱动。

### 3.5 [P1] `lib/string.rs:1112` `secure_zero` 实现是否真"安全"未审

- **位置**：[string.rs](file:///home/anfer/Code/QueenX/src/kernel/framework/lib/string.rs)
- **问题**：
  - `secure_zero` 防止编译器优化清零内存（密码学关键）。
  - 必须用 `core::ptr::write_volatile` 或汇编防止 LLVM 删除。
- **建议方案**：
  1. 验证实现。

### 3.6 [P1] `alloc/slab_alloc.rs:78` Slab 分配**调用 KernelHeap 但不验证 layout**

- **位置**：[alloc/slab_alloc.rs:45-60](file:///home/anfer/Code/QueenX/src/kernel/framework/alloc/slab_alloc.rs#L45-L60)
- **代码**：
  ```rust
  fn alloc(&self, layout: Layout) -> Option<NonNull<u8>> {
      let heap = crate::kernel::framework::mm::get_kmalloc();
      let ptr = heap.allocate(layout.size());   // ← 仅传 size，未传 align
      ...
  }
  ```
- **问题**：
  - `Layout` 含 size + align，但 `heap.allocate(size)` **忽略 align**。
  - 调用方传 `Layout::from_size_align(64, 256)` → 实际分配 64 字节**未对齐到 256** → UB。
- **建议方案**：
  1. 用 `heap.allocate_aligned(size, align)`。

## 4. P2 问题

### 4.1 [P2] `constants/limits.rs:24` `MAX_LOCK_CLASSES = 64` 静默截断——**死锁检测可能失效**

- **位置**：[constants/limits.rs:20-29](file:///home/anfer/Code/QueenX/src/kernel/framework/constants/limits.rs#L20-L29)
- **问题**：
  - 注释承认"满时静默截断"——**死锁检测覆盖失效**。
- **建议方案**：
  1. 满时 panic 启动期。

### 4.2 [P2] `constants/limits.rs:29` `MAX_HELD_LOCKS = 8` 嵌套锁深度上限 8——Linux 默认 48

- **位置**：[constants/limits.rs:26-29](file:///home/anfer/Code/QueenX/src/kernel/framework/constants/limits.rs#L26-L29)
- **问题**：
  - Linux lockdep 默认 48 项嵌套锁深度——QueenX 仅 8。

### 4.3 [P2] `console/mod.rs:71` 模块入口简单

- **位置**：[console/mod.rs:1-71](file:///home/anfer/Code/QueenX/src/kernel/framework/console/mod.rs#L1-L71)
- **问题**：
  - 重导出列表。

### 4.4 [P2] `alloc/mod.rs:7` 入口极简

- **位置**：[alloc/mod.rs:1-7](file:///home/anfer/Code/QueenX/src/kernel/framework/alloc/mod.rs#L1-L7)
- **问题**：
  - 仅 trait re-export。

### 4.5 [P2] `lib/cstr.rs:189` `from_utf8` 失败降级——**可能误处理非 UTF-8 字符串**

- **位置**：[lib/cstr.rs](file:///home/anfer/Code/QueenX/src/kernel/framework/lib/cstr.rs)
- **问题**：
  - 中文/日文用户文件名等非 UTF-8 字符串被静默降级。

### 4.6 [P2] `lib/string.rs` `memcpy` 用 `ptr::copy_nonoverlapping` ——**未检查重叠**

- **位置**：[lib/string.rs](file:///home/anfer/Code/QueenX/src/kernel/framework/lib/string.rs)
- **问题**：
  - 重叠区域应使用 `memmove`（Linux man page）。

## 5. P3 问题

### 5.1 [P3] `console/gfx_console.rs:5` `PANIC_MODE: AtomicBool` 全局

- **位置**：[gfx_console.rs:4-5](file:///home/anfer/Code/QueenX/src/kernel/framework/console/gfx_console.rs#L4-L5)
- **问题**：
  - panic 模式全局标记——多线程并发 panic 可能错乱。

### 5.2 [P3] `io/iouring.rs:13` 仅 13 行 re-export 桩

- **位置**：[io/iouring.rs:1-13](file:///home/anfer/Code/QueenX/src/kernel/framework/io/iouring.rs#L1-L13)
- **问题**：
  - 实际在 services。

### 5.3 [P3] `link/` 空目录——**死目录**

- **位置**：[link/](file:///home/anfer/Code/QueenX/src/kernel/framework/link/)
- **问题**：
  - 应清理。

## 6. 跨子系统关联

### 6.1 lib ↔ services/credo + pwm_*

- `cstr.rs::CStrExt` 在 services/credo、services/proc 等多场景使用。
- `strlen` 在 FFI 入口广泛使用。

### 6.2 console ↔ driver

- `GfxConsole` 依赖 `framework::driver::Framebuffer`。
- 与 [subsystem-driver.md](../audit/subsystem-driver.md) 关联。

### 6.3 alloc ↔ mm

- `FrameAlloc` / `SlabAlloc` trait 委托 framework/mm。
- 与 [subsystem-framework-mm-remaining.md](../audit/subsystem-framework-mm-remaining.md) 关联。

### 6.4 constants ↔ 全 framework

- 所有模块共享容量常量。

## 7. 修复优先级总表

| 优先级 | 问题数 | 估算工作量 |
|---|---:|---:|
| **P0** | 4 | 3-4 天 |
| **P1** | 6 | 4-5 天 |
| **P2** | 6 | 2-3 天 |
| **P3** | 3 | 0.5 天 |
| **合计** | **19** | **10-13 天** |

### P0 修复路径（建议执行顺序）

1. **§2.3 strlen 无界循环**（0.5-1 天，**安全关键**）
2. **§2.2 GfxConsole fb 裸指针生命周期**（1 天）
3. **§2.4 GfxConsole::new 立即解引用**（与 §2.2 合并）
4. **§2.1 string.rs 单文件拆分**（1-2 天）
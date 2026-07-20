# WASM WASI preview1 集成实施计划

> **For agentic workers:** REQUIRED SUB-SKILL: Use compose:subagent (recommended) or compose:execute to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** 实现 WASI snapshot_preview1 标准接口，使 QueenX 可运行 WASI 编译的 WASM 模块

**Architecture:** 独立 WASI 适配层 + 复用底层 POSIX 服务。WASM 解释器通过名称解析的 host function 机制接入 WASI 函数。每个 WASM 实例持有独立 WasiContext (fd 表 + 参数 + 环境)。

**Tech Stack:** Rust (no_std), WASM 1.0, WASI snapshot_preview1, QueenX framekernel

## Global Constraints

- 双架构编译 0 warning 0 error (`./ci/build.sh all`)
- 审计全部通过 (`ci/audit.sh`)
- host-tests 全部通过 (`make test-host`)
- 中文注释强制
- framework `unsafe` 块必须配 `// SAFETY:` 注释
- services 层 `#![deny(unsafe_code)]`
- 设计规格: [wasm-wasi-integration-design.md](./wasm-wasi-integration-design.md)

---

## Task 1: 解释器增强 — 名称解析 host function 注册

**Covers:** [S3]

**Files:**
- Modify: `src/kernel/services/wasm/interpreter.rs`

**Interfaces:**
- Consumes: `Interpreter` struct, `Module.imports`, `ImportDesc`, `ImportKind`
- Produces: `register_named_host_function()`, `auto_register_wasi()`

**Steps:**

- [ ] **Step 1: 在 Interpreter 中添加名称注册存储**

在 `interpreter.rs` 的 `Interpreter` struct 中添加:

```rust
/// 名称索引的 host function (module, name) → index 映射
named_host_functions: alloc::collections::BTreeMap<(alloc::string::String, alloc::string::String), usize>,
```

- [ ] **Step 2: 实现 register_named_host_function**

```rust
/// 注册名称匹配的 host function
///
/// 注册后，auto_register_wasi 可根据 WASM import section 的 module/name
/// 自动查找并注册到正确的 index 位置。
pub fn register_named_host_function(
    &mut self,
    module: &str,
    name: &str,
    f: Box<dyn Fn(&mut Interpreter) -> Result<(), WasmError>>,
) {
    let idx = self.host_functions.len();
    self.host_functions.push(f);
    self.named_host_functions.insert(
        (module.into(), name.into()),
        idx,
    );
}
```

- [ ] **Step 3: 实现 auto_register_wasi**

```rust
/// 自动注册 WASI 函数 (根据 WASM 模块 import section)
///
/// 遍历 module.imports，对每个 (module="wasi_snapshot_preview1", desc=Function) 的 import，
/// 根据 name 查找已注册的 named host function，将其移动到正确的 index 位置。
pub fn auto_register_wasi(&mut self) {
    let mut func_idx = 0u32;
    for import in &self.module.imports {
        if let ImportKind::Function(_) = import.desc {
            let module_name = core::str::from_utf8(&import.module).unwrap_or("");
            let func_name = core::str::from_utf8(&import.name).unwrap_or("");
            if module_name == "wasi_snapshot_preview1" {
                if let Some(&idx) = self.named_host_functions.get(&(module_name.into(), func_name.into())) {
                    // 确保 host_functions 数组足够大
                    while self.host_functions.len() <= func_idx as usize {
                        self.host_functions.push(Box::new(|_| Ok(())));
                    }
                    // 交换到正确位置
                    self.host_functions.swap(func_idx as usize, idx);
                }
            }
            func_idx += 1;
        }
    }
    self.import_func_count = func_idx;
}
```

- [ ] **Step 4: 双架构编译验证**

```bash
./ci/build.sh all
```

- [ ] **Step 5: Commit**

```bash
git add src/kernel/services/wasm/interpreter.rs
git commit -m "feat(wasm): 解释器增强名称解析 host function 注册 (P1)"
```

---

## Task 2: WasiFdTable + WasiContext + errno

**Covers:** [S2]

**Files:**
- Create: `src/kernel/services/wasm/wasi/mod.rs`
- Create: `src/kernel/services/wasm/wasi/fd_table.rs`
- Create: `src/kernel/services/wasm/wasi/errno.rs`
- Modify: `src/kernel/services/wasm/mod.rs` (添加 `pub mod wasi`)

**Interfaces:**
- Consumes: 无
- Produces: `WasiContext`, `WasiFdTable`, `WasiFdEntry`, `WasiRights`, `WasiFileType`, `WasiErrno`, `wasi_success()`, `wasi_errno()`

**Steps:**

- [ ] **Step 1: 创建 wasi/errno.rs**

```rust
//! WASI errno 映射

/// WASI errno 值 (wasi_snapshot_preview1)
#[repr(i32)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WasiErrno {
    Success = 0,
    Badf = 8,
    Exist = 20,
    Inval = 28,
    Io = 29,
    Nametoolong = 37,
    Noent = 44,
    Nospc = 69,
    Notdir = 78,
    Notempty = 79,
    Notsock = 88,
    Notsup = 58,
    Overflow = 61,
    Perm = 63,
    Race = 26,
    Sknotconn = 107,
    Txtbsy = 112,
    // ... 其余 WASI errno
}

impl WasiErrno {
    pub fn as_i32(self) -> i32 { self as i32 }
}

pub fn wasi_success() -> i32 { WasiErrno::Success.as_i32() }
pub fn wasi_errno(e: WasiErrno) -> i32 { e.as_i32() }
```

- [ ] **Step 2: 创建 wasi/fd_table.rs**

```rust
//! WASI 文件描述符表

use super::errno::WasiErrno;

pub const WASI_STDIN: u32 = 0;
pub const WASI_STDOUT: u32 = 1;
pub const WASI_STDERR: u32 = 2;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WasiFileType {
    Directory,
    RegularFile,
    Symlink,
    CharacterDevice,
    Socket,
}

#[derive(Debug, Clone, Copy)]
pub struct WasiRights {
    pub base: u64,
    pub inheriting: u64,
}

impl WasiRights {
    pub const ALL: Self = Self { base: u64::MAX, inheriting: u64::MAX };
    pub const DIRECTORY: Self = Self {
        base: 0x10000000 | 0x800 | 0x400 | 0x200 | 0x100 | 0x80 | 0x40 | 0x20 | 0x10 | 0x08 | 0x04 | 0x02 | 0x01,
        inheriting: u64::MAX,
    };
}

pub struct WasiFdEntry {
    pub file_type: WasiFileType,
    pub rights: WasiRights,
    pub inner_fd: i32,
    pub path: Option<alloc::string::String>,
}

pub struct WasiFdTable {
    entries: alloc::vec::Vec<Option<WasiFdEntry>>,
    max_fds: u32,
}

impl WasiFdTable {
    pub fn new(max_fds: u32) -> Self {
        let mut entries = alloc::vec::Vec::with_capacity(max_fds as usize);
        entries.resize_with(max_fds as usize, || None);
        Self { entries, max_fds }
    }

    pub fn alloc(&mut self, entry: WasiFdEntry) -> Result<u32, WasiErrno> {
        for i in 3..self.max_fds {
            if self.entries[i as usize].is_none() {
                self.entries[i as usize] = Some(entry);
                return Ok(i);
            }
        }
        Err(WasiErrno::Badf)
    }

    pub fn get(&self, fd: u32) -> Result<&WasiFdEntry, WasiErrno> {
        self.entries.get(fd as usize)
            .and_then(|e| e.as_ref())
            .ok_or(WasiErrno::Badf)
    }

    pub fn close(&mut self, fd: u32) -> Result<(), WasiErrno> {
        if fd < 3 { return Err(WasiErrno::Badf); }
        self.entries.get_mut(fd as usize)
            .ok_or(WasiErrno::Badf)?
            .take()
            .ok_or(WasiErrno::Badf)?;
        Ok(())
    }
}
```

- [ ] **Step 3: 创建 wasi/mod.rs**

```rust
//! WASI snapshot_preview1 适配层

#![deny(unsafe_code)]

pub mod errno;
pub mod fd_table;

pub use errno::{WasiErrno, wasi_success, wasi_errno};
pub use fd_table::{WasiFdTable, WasiFdEntry, WasiRights, WasiFileType, WASI_STDIN, WASI_STDOUT, WASI_STDERR};

/// WASI 运行时上下文 (每个 WASM 实例一个)
pub struct WasiContext {
    pub fd_table: WasiFdTable,
    pub args: alloc::vec::Vec<alloc::string::String>,
    pub env: alloc::vec::Vec<(alloc::string::String, alloc::string::String)>,
}

impl WasiContext {
    pub fn new() -> Self {
        Self {
            fd_table: WasiFdTable::new(256),
            args: alloc::vec::Vec::new(),
            env: alloc::vec::Vec::new(),
        }
    }
}
```

- [ ] **Step 4: 在 services/wasm/mod.rs 中添加 wasi 模块**

```rust
pub mod wasi;
```

- [ ] **Step 5: 双架构编译验证**

```bash
./ci/build.sh all
```

- [ ] **Step 6: Commit**

```bash
git add src/kernel/services/wasm/wasi/ src/kernel/services/wasm/mod.rs
git commit -m "feat(wasm): WasiFdTable + WasiContext + errno 基础设施 (P2)"
```

---

## Task 3: G1 进程控制 + G2 时钟/随机 + G3 环境/参数

**Covers:** [S4 G1, G2, G3]

**Files:**
- Create: `src/kernel/services/wasm/wasi/process.rs`
- Create: `src/kernel/services/wasm/wasi/clock_random.rs`
- Create: `src/kernel/services/wasm/wasi/env_args.rs`
- Modify: `src/kernel/services/wasm/wasi/mod.rs` (添加子模块 + WASI 注册表)

**Interfaces:**
- Consumes: `WasiContext`, `WasiErrno`, `Interpreter`
- Produces: `wasi_proc_exit()`, `wasi_sched_yield()`, `wasi_clock_time_get()`, `wasi_random_get()`, `wasi_environ_get()`, `wasi_environ_sizes_get()`, `wasi_args_get()`, `wasi_args_sizes_get()`, `register_all_wasi_functions()`

**Steps:**

- [ ] **Step 1: 创建 wasi/process.rs**

```rust
//! WASI 进程控制: proc_exit, sched_yield

use crate::kernel::services::wasm::interpreter::{Interpreter, Value, WasmError};
use super::{WasiContext, wasi_success};

pub fn wasi_proc_exit(_ctx: &mut WasiContext, interp: &mut Interpreter) -> Result<(), WasmError> {
    let code = interp.stack.pop_i32()?;
    // WASI proc_exit 终止当前 WASM 实例
    // 通过 gas 耗尽模拟终止 (后续可改为实例级终止)
    interp.gas_used = interp.config.max_gas;
    interp.exit_code = code;
    Err(WasmError::Terminated)
}

pub fn wasi_sched_yield(_ctx: &mut WasiContext, _interp: &mut Interpreter) -> Result<(), WasmError> {
    // 简化实现: 立即返回成功
    _interp.stack.push(Value::I32(wasi_success()))?;
    Ok(())
}
```

- [ ] **Step 2: 创建 wasi/clock_random.rs**

```rust
//! WASI 时钟/随机: clock_time_get, random_get

use crate::kernel::services::wasm::interpreter::{Interpreter, Value, WasmError};
use super::{WasiContext, wasi_success, wasi_errno, WasiErrno};
use crate::kernel::services::wasm::wasi::fd_table::write_u32_to_memory;

/// WASI clock IDs
const CLOCK_REALTIME: u32 = 0;
const CLOCK_MONOTONIC: u32 = 1;

pub fn wasi_clock_time_get(ctx: &mut WasiContext, interp: &mut Interpreter) -> Result<(), WasmError> {
    let clock_id = interp.stack.pop_i32()? as u32;
    let _precision = interp.stack.pop_i64()?;
    let result_ptr = interp.stack.pop_i32()? as u32;

    let nanos = match clock_id {
        CLOCK_MONOTONIC => {
            // 使用 framework timer 获取单调时钟
            crate::kernel::framework::timer::clock_gettime_monotonic_ns()
        }
        CLOCK_REALTIME => {
            crate::kernel::framework::timer::clock_gettime_realtime_ns()
        }
        _ => {
            interp.stack.push(Value::I32(wasi_errno(WasiErrno::Inval)))?;
            return Ok(());
        }
    };

    // 写入 timestamp (i64, nanoseconds) 到 WASM 线性内存
    write_i64_to_memory(interp, result_ptr, nanos as i64);
    interp.stack.push(Value::I32(wasi_success()))?;
    Ok(())
}

pub fn wasi_random_get(_ctx: &mut WasiContext, interp: &mut Interpreter) -> Result<(), WasmError> {
    let buf_ptr = interp.stack.pop_i32()? as u32;
    let buf_len = interp.stack.pop_i32()? as u32;

    // 填充随机字节
    let buf = interp.memory.get_slice_mut(buf_ptr, buf_len)?;
    for byte in buf.iter_mut() {
        *byte = crate::kernel::framework::rand::random_u8();
    }

    interp.stack.push(Value::I32(wasi_success()))?;
    Ok(())
}
```

- [ ] **Step 3: 创建 wasi/env_args.rs**

```rust
//! WASI 环境/参数: environ_get, environ_sizes_get, args_get, args_sizes_get

use crate::kernel::services::wasm::interpreter::{Interpreter, Value, WasmError};
use super::{WasiContext, wasi_success};
use crate::kernel::services::wasm::wasi::fd_table::{write_u32_to_memory, write_i32_to_memory};

pub fn wasi_environ_sizes_get(ctx: &mut WasiContext, interp: &mut Interpreter) -> Result<(), WasmError> {
    let count_ptr = interp.stack.pop_i32()? as u32;
    let buf_size_ptr = interp.stack.pop_i32()? as u32;

    let count = ctx.env.len() as u32;
    let buf_size: u32 = ctx.env.iter()
        .map(|(k, v)| k.len() as u32 + 1 + v.len() as u32 + 1)
        .sum();

    write_u32_to_memory(interp, count_ptr, count);
    write_u32_to_memory(interp, buf_size_ptr, buf_size);
    interp.stack.push(Value::I32(0))?;
    Ok(())
}

pub fn wasi_environ_get(ctx: &mut WasiContext, interp: &mut Interpreter) -> Result<(), WasmError> {
    let environ_ptr = interp.stack.pop_i32()? as u32;
    let buf_ptr = interp.stack.pop_i32()? as u32;

    let mut offset = 0u32;
    for (i, (key, val)) in ctx.env.iter().enumerate() {
        // 写入指针数组
        write_u32_to_memory(interp, environ_ptr + (i as u32) * 4, buf_ptr + offset);
        // 写入 "key=value\0"
        let entry = alloc::format!("{}={}", key, val);
        let bytes = entry.as_bytes();
        let buf = interp.memory.get_slice_mut(buf_ptr + offset, bytes.len() as u32)?;
        buf.copy_from_slice(bytes);
        offset += bytes.len() as u32 + 1; // +1 for NUL
    }

    interp.stack.push(Value::I32(0))?;
    Ok(())
}

pub fn wasi_args_sizes_get(ctx: &mut WasiContext, interp: &mut Interpreter) -> Result<(), WasmError> {
    let count_ptr = interp.stack.pop_i32()? as u32;
    let buf_size_ptr = interp.stack.pop_i32()? as u32;

    let count = ctx.args.len() as u32;
    let buf_size: u32 = ctx.args.iter()
        .map(|a| a.len() as u32 + 1)
        .sum();

    write_u32_to_memory(interp, count_ptr, count);
    write_u32_to_memory(interp, buf_size_ptr, buf_size);
    interp.stack.push(Value::I32(0))?;
    Ok(())
}

pub fn wasi_args_get(ctx: &mut WasiContext, interp: &mut Interpreter) -> Result<(), WasmError> {
    let argv_ptr = interp.stack.pop_i32()? as u32;
    let buf_ptr = interp.stack.pop_i32()? as u32;

    let mut offset = 0u32;
    for (i, arg) in ctx.args.iter().enumerate() {
        write_u32_to_memory(interp, argv_ptr + (i as u32) * 4, buf_ptr + offset);
        let bytes = arg.as_bytes();
        let buf = interp.memory.get_slice_mut(buf_ptr + offset, bytes.len() as u32)?;
        buf.copy_from_slice(bytes);
        offset += bytes.len() as u32 + 1;
    }

    interp.stack.push(Value::I32(0))?;
    Ok(())
}
```

- [ ] **Step 4: 在 wasi/mod.rs 中添加 WASI 函数注册表**

```rust
/// WASI 函数注册表: name → 函数指针
pub type WasiFunc = fn(&mut WasiContext, &mut Interpreter) -> Result<(), WasmError>;

pub fn wasi_function_table() -> &'static [(&'static str, WasiFunc)] {
    &[
        ("proc_exit", process::wasi_proc_exit as WasiFunc),
        ("sched_yield", process::wasi_sched_yield as WasiFunc),
        ("clock_time_get", clock_random::wasi_clock_time_get as WasiFunc),
        ("random_get", clock_random::wasi_random_get as WasiFunc),
        ("environ_sizes_get", env_args::wasi_environ_sizes_get as WasiFunc),
        ("environ_get", env_args::wasi_environ_get as WasiFunc),
        ("args_sizes_get", env_args::wasi_args_sizes_get as WasiFunc),
        ("args_get", env_args::wasi_args_get as WasiFunc),
    ]
}
```

- [ ] **Step 5: 双架构编译验证**

```bash
./ci/build.sh all
```

- [ ] **Step 6: Commit**

```bash
git add src/kernel/services/wasm/wasi/
git commit -m "feat(wasm): WASI G1/G2/G3 实现 — 进程控制/时钟随机/环境参数 (P3)"
```

---

## Task 4: G4 FD 管理 + G5 FD I/O

**Covers:** [S4 G4, G5]

**Files:**
- Create: `src/kernel/services/wasm/wasi/fd_ops.rs`
- Modify: `src/kernel/services/wasm/wasi/mod.rs` (更新 WASI 注册表)

**Interfaces:**
- Consumes: `WasiContext`, `WasiFdTable`, `WasiErrno`
- Produces: `wasi_fd_close()`, `wasi_fd_seek()`, `wasi_fd_tell()`, `wasi_fd_sync()`, `wasi_fd_prestat_get()`, `wasi_fd_prestat_dir_name()`, `wasi_fd_stat_get()`, `wasi_fd_read()`, `wasi_fd_write()`, `wasi_fd_pread()`, `wasi_fd_pwrite()`, `wasi_fd_allocate()`, `wasi_fd_advise()`

**Steps:**

- [ ] **Step 1: 创建 wasi/fd_ops.rs — FD 管理函数**

实现 `fd_close`, `fd_seek`, `fd_tell`, `fd_sync`, `fd_prestat_get`, `fd_prestat_dir_name`, `fd_stat_get`。每个函数从 WASM 栈弹出参数，查 fd_table，调用底层 VFS。

- [ ] **Step 2: 创建 wasi/fd_ops.rs — FD I/O 函数**

实现 `fd_read`, `fd_write`, `fd_pread`, `fd_pwrite`, `fd_allocate`, `fd_advise`。`fd_read`/`fd_write` 需要解析 WASM iovec 数组 (从线性内存读取)。

- [ ] **Step 3: 在 wasi/mod.rs 中注册 G4/G5 函数**

将 13 个新函数添加到 `wasi_function_table()`。

- [ ] **Step 4: 双架构编译验证**

```bash
./ci/build.sh all
```

- [ ] **Step 5: Commit**

```bash
git add src/kernel/services/wasm/wasi/fd_ops.rs src/kernel/services/wasm/wasi/mod.rs
git commit -m "feat(wasm): WASI G4/G5 实现 — FD 管理 + FD I/O (P4)"
```

---

## Task 5: G6 路径操作

**Covers:** [S4 G6]

**Files:**
- Create: `src/kernel/services/wasm/wasi/path_ops.rs`
- Modify: `src/kernel/services/wasm/wasi/mod.rs` (更新 WASI 注册表)

**Interfaces:**
- Consumes: `WasiContext`, `WasiFdTable`, VFS
- Produces: `wasi_path_open()`, `wasi_path_create_directory()`, `wasi_path_remove_directory()`, `wasi_path_unlink_file()`, `wasi_path_rename()`, `wasi_path_symlink()`, `wasi_path_readlink()`, `wasi_path_filestat_get()`, `wasi_path_filestat_set_times()`, `wasi_path_link()`

**Steps:**

- [ ] **Step 1: 创建 wasi/path_ops.rs — 路径解析辅助**

实现 `resolve_path(ctx, dirfd, path_ptr, path_len)` 辅助函数，将 WASM 路径 + dirfd 解析为 QueenX 内部路径。

- [ ] **Step 2: 实现 path_open (核心)**

`path_open` 是最复杂的 WASI 函数：解析路径 → 查找目录 fd → 调用 VFS open → 创建新 fd entry → 注册到 fd_table。

- [ ] **Step 3: 实现其余 9 个路径操作函数**

每个函数: 弹出参数 → 解析路径 → 调用 VFS → 返回结果。

- [ ] **Step 4: 在 wasi/mod.rs 中注册 G6 函数**

- [ ] **Step 5: 双架构编译验证**

```bash
./ci/build.sh all
```

- [ ] **Step 6: Commit**

```bash
git add src/kernel/services/wasm/wasi/path_ops.rs src/kernel/services/wasm/wasi/mod.rs
git commit -m "feat(wasm): WASI G6 实现 — 路径操作 (P5)"
```

---

## Task 6: G7 高级 FD + G8 Socket

**Covers:** [S4 G7, G8]

**Files:**
- Modify: `src/kernel/services/wasm/wasi/fd_ops.rs` (添加 fd_renumber, fd_dup, fd_readdir)
- Create: `src/kernel/services/wasm/wasi/sock.rs`
- Modify: `src/kernel/services/wasm/wasi/mod.rs` (更新 WASI 注册表)

**Interfaces:**
- Consumes: `WasiContext`, `WasiFdTable`, services::net
- Produces: `wasi_fd_renumber()`, `wasi_fd_dup()`, `wasi_fd_readdir()`, `wasi_sock_accept()`, `wasi_sock_connect()`, `wasi_sock_recv()`, `wasi_sock_send()`

**Steps:**

- [ ] **Step 1: 在 fd_ops.rs 中添加 fd_renumber, fd_dup, fd_readdir**

- [ ] **Step 2: 创建 wasi/sock.rs — Socket 函数**

实现 4 个 socket 函数，桥接到 services::net。

- [ ] **Step 3: 在 wasi/mod.rs 中注册 G7/G8 函数**

- [ ] **Step 4: 双架构编译验证**

```bash
./ci/build.sh all
```

- [ ] **Step 5: Commit**

```bash
git add src/kernel/services/wasm/wasi/
git commit -m "feat(wasm): WASI G7/G8 实现 — 高级 FD + Socket (P6)"
```

---

## Task 7: 集成测试

**Covers:** [S6]

**Files:**
- Create: `host-tests/tests/wasi_test.rs`

**Interfaces:**
- Consumes: 全部 WASI 函数
- Produces: WASI 合规测试用例

**Steps:**

- [ ] **Step 1: 创建 wasi_test.rs — 基础测试**

测试 WasiFdTable 创建、alloc/close/get、WasiContext 初始化、errno 映射。

- [ ] **Step 2: 创建 WASI 函数单元测试**

为每组 WASI 函数编写测试：mock Interpreter + 线性内存，验证参数解析和返回值。

- [ ] **Step 3: 运行 host-tests**

```bash
make test-host
```

- [ ] **Step 4: 双架构编译验证**

```bash
./ci/build.sh all
```

- [ ] **Step 5: Commit**

```bash
git add host-tests/tests/wasi_test.rs
git commit -m "test(wasm): WASI preview1 集成测试 (P7)"
```

---

## Task 8: 全量验证 + 文档更新

**Covers:** [S6]

**Steps:**

- [ ] **Step 1: 双架构编译**

```bash
./ci/build.sh all
```

- [ ] **Step 2: 全量审计**

```bash
ci/audit.sh full
```

- [ ] **Step 3: host-tests**

```bash
make test-host
```

- [ ] **Step 4: 更新 services/wasm/mod.rs 文档**

- [ ] **Step 5: 更新 docs/plan/future-roadmap.md WASM 相关描述**

- [ ] **Step 6: Commit**

```bash
git add -A
git commit -m "docs(wasm): WASI preview1 集成完成，更新文档"
```

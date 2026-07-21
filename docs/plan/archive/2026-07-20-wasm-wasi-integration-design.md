# WASM WASI preview1 接入设计规格

> **For agentic workers:** REQUIRED SUB-SKILL: Use compose:subagent (recommended) or compose:execute to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** 实现 WASI snapshot_preview1 标准接口，使 QueenX 可运行 WASI 编译的 WASM 模块

**Architecture:** 独立 WASI 适配层 + 复用底层 POSIX 服务，WASM 解释器通过名称解析的 host function 机制接入

**Tech Stack:** Rust, WASM 1.0, WASI snapshot_preview1, QueenX framekernel

---

## [S1] WASI preview1 函数清单

WASI snapshot_preview1 定义 50 个函数，分 5 个模块：

| 模块 | 函数数 | 代表函数 |
|------|--------|---------|
| `wasi_snapshot_preview1` (核心) | 36 | `fd_read`, `fd_write`, `fd_close`, `fd_seek`, `path_open` |
| `wasi_snapshot_preview1` (时钟/随机) | 3 | `clock_time_get`, `random_get`, `proc_exit` |
| `wasi_snapshot_preview1` (preopen) | 2 | `fd_prestat_get`, `fd_prestat_dir_name` |
| `wasi_snapshot_preview1` (sock) | 4 | `sock_accept`, `sock_connect`, `sock_recv`, `sock_send` |
| `wasi_snapshot_preview1` (其余) | 5 | `environ_get`, `environ_sizes_get`, `args_get`, `args_sizes_get`, `sched_yield` |

Phase 1 实现全部 50 个函数。

---

## [S2] 架构设计

### 分层架构

```
WASM 模块
  │ import "wasi_snapshot_preview1" "fd_read" ...
  │
  ▼
WASI 适配层 (services/wasm/wasi/)
  │ WasiContext { fd_table, args, env, clock }
  │ fn wasi_fd_read() → 查 fd_table → read_bytes()
  │ fn wasi_path_open() → 路径解析 → open_file()
  │ fn wasi_clock_time_get() → clock_gettime()
  │
  ▼
现有 POSIX 服务层
  services::fs (VFS/open/read/write)
  services::mm (mmap)
  services::proc (exit/sched_yield)
  framework::timer (clock_gettime)
```

### 核心类型

```rust
// WASI 文件描述符表 (独立于 POSIX fd 表)
pub struct WasiFdTable {
    entries: Vec<Option<WasiFdEntry>>,
}

pub struct WasiFdEntry {
    pub file_type: WasiFileType,
    pub rights: WasiRights,
    pub inner_fd: u32,
    pub path: Option<String>,
}

// WASI 运行时上下文 (每个 WASM 实例一个)
pub struct WasiContext {
    pub fd_table: WasiFdTable,
    pub args: Vec<String>,
    pub env: Vec<(String, String)>,
    pub clock: WasiClock,
}
```

---

## [S3] WASM 解释器集成

### 增强: 名称解析 host function 注册

```rust
impl Interpreter {
    /// 注册名称匹配的 host function
    pub fn register_named_host_function(
        &mut self,
        module: &str,
        name: &str,
        f: Box<dyn Fn(&mut Interpreter) -> Result<(), WasmError>>,
    );

    /// 自动注册 WASI 函数 (根据 WASM 模块 import section)
    pub fn auto_register_wasi(&mut self, wasi_ctx: &mut WasiContext);
}
```

`auto_register_wasi` 流程:
1. 遍历 `module.imports`，找出 `(module="wasi_snapshot_preview1", desc=Function)` 的 import
2. 根据 `name` 查找 WASI 函数实现
3. 按 import 顺序注册到 `host_functions`

### WASI 函数实现模式

```rust
pub fn wasi_fd_read(ctx: &mut WasiContext, interp: &mut Interpreter) -> Result<(), WasmError> {
    let fd = interp.stack.pop_i32()? as u32;
    let iovs_ptr = interp.stack.pop_i32()? as u32;
    let iovs_len = interp.stack.pop_i32()? as u32;
    let nread_ptr = interp.stack.pop_i32()? as u32;

    let iovecs = read_iovec_from_memory(interp, iovs_ptr, iovs_len)?;
    let entry = ctx.fd_table.get(fd)?;

    let mut total = 0u32;
    for iov in &iovecs {
        let buf = interp.memory.get_slice_mut(iov.buf, iov.len)?;
        let n = read_bytes(entry.inner_fd, buf)?;
        total += n;
    }

    write_u32_to_memory(interp, nread_ptr, total);
    interp.stack.push(Value::I32(0))?;
    Ok(())
}
```

---

## [S4] WASI 函数实现分组

| 组 | 函数 | 依赖 | 预计行数 |
|----|------|------|---------|
| G1: 进程控制 | `proc_exit`, `sched_yield` | services::proc | ~50 |
| G2: 时钟/随机 | `clock_time_get`, `random_get` | framework::timer, rand | ~80 |
| G3: 环境/参数 | `environ_get`, `environ_sizes_get`, `args_get`, `args_sizes_get` | WasiContext | ~100 |
| G4: FD 管理 | `fd_close`, `fd_seek`, `fd_tell`, `fd_sync`, `fd_prestat_get`, `fd_prestat_dir_name`, `fd_stat_get` | WasiFdTable | ~200 |
| G5: FD I/O | `fd_read`, `fd_write`, `fd_pread`, `fd_pwrite`, `fd_allocate`, `fd_advise` | WasiFdTable + VFS | ~250 |
| G6: 路径操作 | `path_open`, `path_create_directory`, `path_remove_directory`, `path_unlink_file`, `path_rename`, `path_symlink`, `path_readlink`, `path_filestat_get`, `path_filestat_set_times`, `path_link` | VFS + WasiFdTable | ~400 |
| G7: 高级 FD | `fd_renumber`, `fd_dup`, `fd_readdir` | WasiFdTable | ~150 |
| G8: Socket | `sock_accept`, `sock_connect`, `sock_recv`, `sock_send` | services::net | ~200 |

总计 ~1,500 行 WASI 适配层代码。

---

## [S5] 文件结构

```
services/wasm/
├── mod.rs                    # 模块声明 (已有)
├── interpreter.rs            # 解释器 (需增强 register_named_host_function)
├── types.rs                  # 类型定义 (已有)
├── module.rs                 # 模块解析 (已有)
├── runtime.rs                # 运行时 (已有)
├── leb128.rs                 # LEB128 (已有)
├── wasi/                     # 新增: WASI 适配层
│   ├── mod.rs                # WasiContext + WasiFdTable + 公共 API
│   ├── fd_table.rs           # 独立 fd 表管理
│   ├── fd_ops.rs             # G4+G5: fd_read/write/seek/stat
│   ├── path_ops.rs           # G6: path_open/rename/unlink
│   ├── env_args.rs           # G3: environ/args
│   ├── clock_random.rs       # G2: clock_time_get/random_get
│   ├── process.rs            # G1: proc_exit/sched_yield
│   ├── sock.rs               # G8: sock_accept/connect/recv/send
│   └── errno.rs              # WASI errno 映射
```

总计新增 ~1,600 行 (wasi/ 目录)，修改 ~100 行 (interpreter.rs 增强)。

---

## [S6] 实施顺序

| Phase | 内容 | 依赖 | 预计工时 |
|-------|------|------|---------|
| P1 | 解释器增强 (名称解析 + auto_register_wasi) | 无 | 1 天 |
| P2 | WasiFdTable + WasiContext + errno | P1 | 1 天 |
| P3 | G1 进程控制 + G2 时钟/随机 + G3 环境/参数 | P2 | 1 天 |
| P4 | G4 FD 管理 + G5 FD I/O | P2 | 2 天 |
| P5 | G6 路径操作 | P4 | 2 天 |
| P6 | G7 高级 FD + G8 Socket | P4 | 1 天 |
| P7 | 集成测试 + WASI 合规验证 | P6 | 1 天 |

**总预估: 8 人天**

# 上层功能调用实现计划

> **目标**: 实现缺失的上层功能，使已定义的底层函数被调用，消除 dead_code 警告
>
> **背景**: 此前实现的 11 项底层函数中，大部分缺少调用者，导致编译警告

---

## 一、分类总览

| 功能 | 底层函数 | 调用者位置 | 工作量 |
|------|----------|------------|--------|
| 键盘初始化 | ps2_self_test, keyboard_reset | keyboard.rs init() | 0.5 天 |
| 进程状态查询 | is_running, is_exited | scheduler.rs | 0.5 天 |
| PCI 端口 I/O | outb, outw, inb, inw | 删除 (重复) | 0.1 天 |
| 缓存引用计数 | ref_dec, is_ref_zero, get_ref_count | dcache.rs 驱逐逻辑 | 0.5 天 |
| inotify fd | fd, is_fd_valid | inotify.rs 事件分发 | 0.5 天 |
| Unix 套接字 | find_free_socket, has_free_socket, used_socket_count | unix.rs socket() | 0.5 天 |

**总预估**: 2.5 天

---

## 二、详细任务

### Task 1: 键盘初始化流程

**文件**: `src/kernel/framework/driver/input/keyboard.rs`

**改动**:
1. 在 `KeyboardDriver::init()` 中调用 `ps2_self_test()`
2. 添加初始化失败时调用 `keyboard_reset()` 的重试逻辑

```rust
fn init(&mut self) -> DriverResult<()> {
    // 1. 清空输出缓冲区
    let _ = keyboard_read_data();

    // 2. PS/2 控制器自检
    if let Err(_) = ps2_self_test() {
        // 自检失败, 尝试重置
        let _ = keyboard_reset();
        // 重置后再次自检
        if let Err(_) = ps2_self_test() {
            return Err(DriverError::HardwareError);
        }
    }

    // 3. 发送 SET LED 命令设置初始 LED 状态
    update_leds(&self.modifiers);

    // 4. 清空缓冲区
    self.buffer.clear();

    self.initialized = true;
    Ok(())
}
```

**验证**: 编译通过, dead_code 警告减少

---

### Task 2: 进程状态查询调用

**文件**: `src/kernel/framework/proc/user_proc.rs`, `src/kernel/framework/proc/scheduler.rs`

**改动**:
1. 在 `scheduler.rs` 的调度循环中添加进程状态检查
2. 使用 `is_running()` 和 `is_exited()` 判断进程状态

```rust
// 在调度循环中
fn schedule_tick(&mut self) {
    for proc in self.processes.iter() {
        if proc.is_exited() {
            // 处理已退出的进程
            self.cleanup_process(proc);
        } else if proc.is_running() {
            // 处理正在运行的进程
            self.update_runtime(proc);
        }
    }
}
```

**验证**: 编译通过, dead_code 警告减少

---

### Task 3: PCI 端口 I/O 删除

**文件**: `src/kernel/framework/pci/mod.rs`

**改动**:
1. 删除 `port_io` 模块中的 `outb`, `outw`, `inb`, `inw` 函数
2. 这些函数与 `framework::driver` 中的端口 I/O 原语重复

**验证**: 编译通过, dead_code 警告减少

---

### Task 4: 缓存引用计数管理

**文件**: `src/kernel/services/fs/dcache.rs`

**改动**:
1. 在 `ICache` 的缓存驱逐逻辑中调用 `ref_dec()`
2. 在缓存条目释放前调用 `is_ref_zero()` 检查
3. 在诊断函数中调用 `get_ref_count()`

```rust
fn evict_entry(&mut self, ino: u32) {
    self.ref_dec(ino);
    if self.is_ref_zero(ino) {
        // 可以安全释放
        self.invalidate(ino);
    }
}
```

**验证**: 编译通过, dead_code 警告减少

---

### Task 5: inotify 文件描述符校验

**文件**: `src/kernel/services/fs/inotify.rs`

**改动**:
1. 在 `push_event()` 中添加 `is_fd_valid()` 检查
2. 在事件分发路径中使用 `fd()` 获取文件描述符

```rust
fn push_event(&mut self, event: InotifyEvent) {
    if !self.is_fd_valid() {
        return; // fd 无效, 丢弃事件
    }
    // ... 事件入队逻辑
}
```

**验证**: 编译通过, dead_code 警告减少

---

### Task 6: Unix 域套接字管理

**文件**: `src/kernel/services/net/unix.rs`

**改动**:
1. 在 `socket()` 系统调用实现中调用 `has_free_socket()`
2. 在资源限制检查中调用 `used_socket_count()`

```rust
fn socket_create(...) -> Result<u32> {
    if !self.has_free_socket() {
        return Err(ENOMEM);
    }
    let count = self.used_socket_count();
    if count >= MAX_UNIX_SOCKETS {
        return Err(EMFILE);
    }
    // ... 创建套接字逻辑
}
```

**验证**: 编译通过, dead_code 警告减少

---

## 三、实施顺序

| 阶段 | Task | 工作量 | 说明 |
|------|------|--------|------|
| Phase 1 | Task 3 (PCI 删除) | 0.1 天 | 最简单, 直接删除 |
| Phase 2 | Task 1 (键盘初始化) | 0.5 天 | 独立功能 |
| Phase 3 | Task 2 (进程状态) | 0.5 天 | 调度器相关 |
| Phase 4 | Task 4 (缓存引用) | 0.5 天 | 文件系统相关 |
| Phase 5 | Task 5 (inotify) | 0.5 天 | 文件系统相关 |
| Phase 6 | Task 6 (Unix 套接字) | 0.5 天 | 网络栈相关 |

---

## 四、验证标准

每个 Task 完成后:
1. 双架构编译 0 warning 0 error
2. `audit_dead_code.py` 违规数递减
3. host-tests 通过

---

**文档状态**: [X] 已完成 (源码审计确认 6/6 任务实装)
**完成 commit**: `e340febd` (2026-07-18)
**创建时间**: 2026-07-17

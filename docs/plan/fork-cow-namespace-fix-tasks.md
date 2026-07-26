# fork COW + Namespace 继承修复任务

> fork() 共享 CR3 已可用 (X→Y 正常), 但 COW 内存隔离和 namespace 继承因 KPTI 交互问题阻塞.
> 本文档记录三个独立但关联的问题, 供后续开发者按优先级逐个修复.

## 已修复项 (无需再改)

- **KPTI USER_PML4 清除循环**: 已移除无安全收益的低半区清除 (commit `0c461631`)
- **sys_wait4 阻塞**: 已修复为 scheduler_yield 循环等待子进程 Zombie
- **VMM lock 可重入**: 单核安全, 允许 page fault handler 在 COW 持锁期间重入
- **child_ctx.cr3**: 子进程上下文现在使用 `child_cr3` (COW 克隆), 而非 `parent_cr3`
- **fork 基础功能**: 共享 CR3 + new_init namespace, X→Y 正常工作

---

## 问题 1: COW page fault handler 在 KPTI 下映射到错误页表

- **描述**: `clone_user_page_table_cow` 本身能正常完成 (标记 1→G 全部出现). 但 fork 返回后, 父进程/子进程写入共享页触发 page fault 时, handler 无法正确处理 COW 故障.
- **根因**: KPTI 开启时, page fault handler 在内核态执行 (CR3=内核页表). `get_current_pml4()` 返回内核 PML4. 所有页表修改 (映射新页) 写入内核 PML4 而非用户页表. 返回用户态时加载用户 PML4, 新映射无效, 导致无限 page fault 循环.
- **影响范围**: 所有 page fault 路径 (`page_fault.rs` 中 8 处 `get_current_pml4()` 调用). 不仅影响 COW, 影响所有按需分页/栈扩展/swap-in.
- **状态**: []
- **方案**: 实现 `get_user_pml4()`, 通过 `process_get_cr3(current_pid)` 获取进程自己的用户页表. **关键约束**: page fault handler 在中断上下文中执行, 不能获取任何可能被其他上下文持有的锁. `process_get_cr3` 内部获取 `PROCESS_TABLE.lock()`, 如果该锁在其他路径 (如 syscall) 中被持有, 会导致死锁.
- **详情**:

  ```
  # 调用链
  #PF → exception_handler → PageFaultHandler::handle
    → is_user_mode() == true
    → handle_user_page_fault(pf_info)
    → get_current_pml4()  // 返回内核 PML4, 应返回用户 PML4
    → map_page_in_table(pml4, ...)  // 写入内核 PML4, 用户态看不到
  ```

  **已验证的失败尝试**:
  1. `process_get_cr3()` 实现: 编译通过, 但导致 reboot 循环. 推测: PROCESS_TABLE.lock() 在中断上下文与其他锁 (scheduler, VMM) 产生死锁.
  2. 跳过锁直接读 Process.cr3: 未尝试, 但 InterruptFrame 中无 current_pid 信息.

  **建议方向**:
  - 在 per-CPU 结构中缓存当前进程的 user cr3 (syscall 入口时更新, 无锁读取)
  - 或在 InterruptFrame / context switch 时将 user cr3 写入固定 per-CPU 槽位
  - 或使用 TSS.RSP0 附近的固定偏移存储 user cr3 (中断进入时 CPU 自动切到内核栈, 可从 TSS 恢复 user cr3)

---

## 问题 2: Namespace fork_from 在 with_process 内 triple fault

- **描述**: 在 `PROCESS_TABLE.with_process()` 闭包内调用 `NamespaceSet::fork_from()` 或手动 `Arc::clone` × 7 个字段, 导致 init 进入 Ring 3 后立即 triple fault 重启. 不打印 X, 系统在 boot→Ring3→reboot 循环.
- **根因**: 未知. 已排除的假设:
  - `NamespaceSet::fork_from` 函数本身有问题 → 排除: 函数内部仅 7 个 `Arc::clone`, 无锁操作
  - 锁顺序死锁 (PROCESS_TABLE.lock + namespaces.lock) → 排除: 单核+中断禁用, 无竞争
  - `Arc::clone` 不兼容 → 排除: 同样手动 `Arc::clone` 也 triple fault
- **影响范围**: 仅影响 namespace 继承. 当前用 `NamespaceSet::new_init()` 回避, 子进程不继承父进程 namespace.
- **状态**: []
- **方案**: 需进一步排查. 可能方向:
  - `alloc_process` 创建的 Process 结构体中 `namespaces: Mutex<NamespaceSet>` 的内存布局问题 — 手动构造 `NamespaceSet` 后写入可能覆盖相邻字段
  - `PROCESS_TABLE.with_process` 闭包内 `*child.namespaces.lock() = ...` 的 drop 语义问题 — 旧 NamespaceSet (7 个 Arc) 被 drop 时触发 triple fault
  - 中断上下文中 `Arc` 的 drop 路径触发了 page fault (kmalloc free 时访问已映射为只读的页)
- **详情**:

  ```rust
  // 复现代码 (proc_ops.rs:675-705)
  if let Some(parent_ns) = PROCESS_TABLE.with_process(parent_pid, |p| {
      let guard = p.namespaces.lock();
      super::NamespaceSet {
          uts: alloc::sync::Arc::clone(&guard.uts),
          // ... 6 more Arc::clone
      }
  }) {
      *child.namespaces.lock() = parent_ns;  // drop 旧 NamespaceSet → triple fault
  }

  // 对比: new_init() 正常工作
  *child.namespaces.lock() = super::NamespaceSet::new_init();
  ```

  **调试建议**:
  - 检查 `Process` 结构体内存布局: `namespaces` 字段前后是什么字段? 写入是否越界?
  - 在 `*child.namespaces.lock() = parent_ns` 前后加串口标记, 确认 crash 精确位置
  - 尝试: 不 drop 旧 NamespaceSet, 直接替换为 parent_ns (需要 `ManuallyDrop`)
  - 尝试: 在 `with_process` 外构造 NamespaceSet, 避免嵌套锁

---

## 问题 3: COW 仅子进程标记只读 (简化方案)

- **描述**: 经典 COW 需要同时修改父进程和子进程的 PTE 为只读. 但 KPTI 下父进程 page fault 无法正确处理 (问题 1). 可用简化方案: 只修改子进程 PTE 为只读, 父进程保持可写.
- **根因**: 设计权衡, 非 bug. 简化方案的 tradeoff:
  - 父进程写入 → 无 page fault → 原始物理页被修改 → 子进程看到父进程的修改 (COW 不完整)
  - 子进程写入 → page fault → 分配新页 → 独立副本 (子进程侧 COW 正确)
- **状态**: []
- **方案**: 作为问题 1 的过渡方案, 在 `clone_user_page_table_cow_inner` 中去掉 `parent_pt_virt.add(l).write_volatile(pte)` (清除父进程 PTE WRITABLE 的行). 子进程 PTE 仍然清除 WRITABLE. 这样:
  - fork 后父进程无需 page fault, 正常运行
  - 子进程写入触发 COW fault → 但需要问题 1 修复才能正确处理
  - 结论: **必须先修复问题 1, 此方案才有意义**
- **详情**: 修改点在 `cow.rs:244-249`, 删除 6 行代码即可. 无需 TLB flush.

---

## 依赖关系

```text
问题 1 (page fault KPTI)  ← 必须先修
  └── 问题 3 (简化 COW)    ← 问题 1 修复后可实施
问题 2 (namespace triple fault) ← 独立, 可并行排查
```

## 验证方法

```bash
# 验证 fork 基础功能
timeout 15 qemu-system-x86_64 -m 512 -nic none -kernel build/kernel.flat -nographic 2>/dev/null | grep "X\|Y"
# 预期: 输出 X 和 Y (无 reboot 循环)

# 验证无 reboot 循环
timeout 10 qemu-system-x86_64 -m 512 -nic none -no-reboot -kernel build/kernel.flat -nographic 2>/dev/null | grep -c "Booting from ROM"
# 预期: 1 (不重启)

# 验证 COW 后无 reboot
# 启用 COW (proc_ops.rs 中恢复 clone_user_page_table_cow 调用) 后:
timeout 10 qemu-system-x86_64 -m 512 -nic none -no-reboot -kernel build/kernel.flat -nographic 2>/dev/null | grep -c "Booting from ROM"
# 预期: 1
```

## 相关源文件

| 文件 | 作用 |
|---|---|
| `src/kernel/framework/mm/cow.rs` | COW 页表克隆 + fault 处理 |
| `src/kernel/framework/mm/page_fault.rs` | page fault handler, `get_user_pml4()` 待实现 |
| `src/kernel/framework/proc/proc_ops.rs:670-735` | `sys_fork()` 实现 |
| `src/kernel/services/proc/namespace.rs:517-527` | `NamespaceSet::fork_from()` |
| `src/kernel/framework/proc/process.rs:206` | `namespaces: Mutex<NamespaceSet>` 字段定义 |
| `src/kernel/framework/mm/vmm_x86_64.rs:1553` | `get_current_pml4()` |

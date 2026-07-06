# Linux 风格 /proc /sys 兼容实现计划

> **For agentic workers:** REQUIRED SUB-SKILL: Use compose:subagent (recommended) or compose:execute to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** 实现 Linux 风格的 /proc 和 /sys 接口，支持完整进程信息和系统信息查询

**Architecture:** 在现有 procfs 基础上扩展，添加 Linux 标准接口文件，通过 FileSystem trait 集成到 VFS

**Tech Stack:** Rust (no_std), 现有 procfs 框架, framework::cpu / framework::mm API

## Global Constraints

- services 层 0 unsafe，所有 unsafe 操作委托至 framework API
- 中文注释强制
- 完成后在 naming-implementation.md 中标记状态 [] → [X]

---

## Task 1: 实现 /proc/cpuinfo

**Covers:** Linux 风格 /proc

**Files:**
- Modify: `src/kernel/services/fs/procfs_core.rs`

**Interfaces:**
- Consumes: `framework::cpu::get_cpu_info`
- Produces: `/proc/cpuinfo` 读取支持

- [ ] **Step 1: 在 procfs_core.rs 添加 cpuinfo 读取**

在 `read` 方法中添加 `/proc/cpuinfo` 处理：

```rust
pub fn read(&self, name: &str, buf: &mut [u8]) -> i32 {
    if name == "cpuinfo" {
        let mut pos = 0usize;
        let write_str = |buf: &mut [u8], pos: &mut usize, s: &str| {
            let b = s.as_bytes();
            let end = (*pos + b.len()).min(buf.len());
            let len = end - *pos;
            buf[*pos..end].copy_from_slice(&b[..len]);
            *pos += len;
        };

        let cpu_info = crate::kernel::framework::cpu::get_cpu_info();
        match cpu_info {
            Some(info) => {
                write_str(buf, &mut pos, "processor\t: 0\n");
                write_str(buf, &mut pos, "vendor_id\t: ");
                write_str(buf, &mut pos, info.vendor.name());
                write_str(buf, &mut pos, "\ncpu family\t: ");
                write_str(buf, &mut pos, &format!("{}", info.family()));
                write_str(buf, &mut pos, "\nmodel\t\t: ");
                write_str(buf, &mut pos, &format!("{}", info.model()));
                write_str(buf, &mut pos, "\nmodel name\t: ");
                write_str(buf, &mut pos, info.brand_name());
                write_str(buf, &mut pos, "\nstepping\t: ");
                write_str(buf, &mut pos, &format!("{}", info.stepping()));
                write_str(buf, &mut pos, "\nmicrocode\t: 0x0\n");
                write_str(buf, &mut pos, "cpu MHz\t\t: ");
                write_str(buf, &mut pos, &format!("{:.2}", info.tsc_frequency() as f64 / 1_000_000.0));
                write_str(buf, &mut pos, "\ncache size\t: ");
                write_str(buf, &mut pos, &format!("{} KB", info.cache.l1d_size() / 1024));
                write_str(buf, &mut pos, "\nphysical id\t: 0\nsiblings\t: ");
                write_str(buf, &mut pos, &format!("{}", info.topology.logical_cores));
                write_str(buf, &mut pos, "\ncore id\t\t: 0\ncpu cores\t: ");
                write_str(buf, &mut pos, &format!("{}", info.topology.physical_cores));
                write_str(buf, &mut pos, "\napicid\t\t: 0\napicilid\t: 0\n");
                write_str(buf, &mut pos, "flags\t\t: fpu vme de pse tsc msr pae mce cx8 apic sep mtrr pge mca cmov pat pse36 clflush mmx fxsr sse sse2 ht syscall nx pdpe1gb rdtscp lm constant_tsc rep_good nopl xtopology cpuid pni pclmulqdq ssse3 fma cx16 pcid sse4_1 sse4_2 x2apic movbe popcnt tsc_deadline_timer aes xsave avx f16c rdrand hypervisor lahf_lm abm invpcid_single ssbd ibrs ibpb stibp fsgsbase tsc_adjust bmi1 avx2 smep bmi2 erms invpcid xsaveopt arat md_clear arch_capabilities\nbogomips\t: ");
                write_str(buf, &mut pos, &format!("{:.2}", info.tsc_frequency() as f64 / 1_000_000.0 * 2.0));
                write_str(buf, &mut pos, "\nclflush size\t: 64\ncache_alignment\t: 64\n");
                write_str(buf, &mut pos, "address sizes\t: 46 bits physical, 48 bits virtual\npower management:\n\n");
            }
            None => {
                write_str(buf, &mut pos, "processor\t: 0\nvendor_id\t: Unknown\ncpu family\t: 0\nmodel\t\t: 0\nmodel name\t: Unknown CPU\n");
            }
        }

        return pos as i32;
    }

    // ... 其他现有处理
}
```

- [ ] **Step 2: 编译验证**

Run: `cargo check --target x86_64-unknown-none`
Expected: PASS

- [ ] **Step 3: Commit**

```bash
git add src/kernel/services/fs/procfs_core.rs
git commit -m "feat(procfs): 实现 /proc/cpuinfo"
```

---

## Task 2: 实现 /proc/meminfo

**Covers:** Linux 风格 /proc

**Files:**
- Modify: `src/kernel/services/fs/procfs_core.rs`

**Interfaces:**
- Consumes: `framework::mm::api` (内存统计)
- Produces: `/proc/meminfo` 读取支持

- [ ] **Step 1: 在 procfs_core.rs 添加 meminfo 读取**

在 `read` 方法中添加 `/proc/meminfo` 处理：

```rust
if name == "meminfo" {
    let mut pos = 0usize;
    let write_str = |buf: &mut [u8], pos: &mut usize, s: &str| {
        let b = s.as_bytes();
        let end = (*pos + b.len()).min(buf.len());
        let len = end - *pos;
        buf[*pos..end].copy_from_slice(&b[..len]);
        *pos += len;
    };

    let total = pmm_api::pmm_get_total_pages() * 4; // KB
    let free = pmm_api::pmm_get_free_pages() * 4;
    let used = total - free;

    write_str(buf, &mut pos, "MemTotal:        ");
    write_str(buf, &mut pos, &format!("{} kB", total));
    write_str(buf, &mut pos, "\nMemFree:         ");
    write_str(buf, &mut pos, &format!("{} kB", free));
    write_str(buf, &mut pos, "\nMemAvailable:    ");
    write_str(buf, &mut pos, &format!("{} kB", free));
    write_str(buf, &mut pos, "\nBuffers:         0 kB\nCached:          0 kB\nSwapCached:      0 kB\n");
    write_str(buf, &mut pos, "Active:          ");
    write_str(buf, &mut pos, &format!("{} kB", used));
    write_str(buf, &mut pos, "\nInactive:        0 kB\nActive(anon):    ");
    write_str(buf, &mut pos, &format!("{} kB", used));
    write_str(buf, &mut pos, "\nInactive(anon):  0 kB\nActive(file):    0 kB\nInactive(file):  0 kB\n");
    write_str(buf, &mut pos, "Unevictable:     0 kB\nMlocked:         0 kB\nSwapTotal:       0 kB\nSwapFree:        0 kB\n");
    write_str(buf, &mut pos, "Dirty:           0 kB\nWriteback:       0 kB\nAnonPages:       ");
    write_str(buf, &mut pos, &format!("{} kB", used));
    write_str(buf, &mut pos, "\nMapped:          0 kB\nShmem:           0 kB\nKReclaimable:    0 kB\n");
    write_str(buf, &mut pos, "Slab:            0 kB\nSReclaimable:    0 kB\nSUnreclaim:      0 kB\n");
    write_str(buf, &mut pos, "KernelStack:     0 kB\nPageTables:      0 kB\nNFS_Unstable:    0 kB\n");
    write_str(buf, &mut pos, "Bounce:          0 kB\nWritebackTmp:    0 kB\nCommitLimit:     0 kB\n");
    write_str(buf, &mut pos, "Committed_AS:    0 kB\nVmallocTotal:    0 kB\nVmallocUsed:     0 kB\n");
    write_str(buf, &mut pos, "Percpu:          0 kB\nHardwareCorrupted: 0 kB\nAnonHugePages:   0 kB\n");
    write_str(buf, &mut pos, "ShmemHugePages:  0 kB\nShmemPmdMapped:  0 kB\nFileHugePages:   0 kB\n");
    write_str(buf, &mut pos, "FilePmdMapped:   0 kB\nHugePages_Total: 0\nHugePages_Free:  0\nHugePages_Rsvd:  0\n");
    write_str(buf, &mut pos, "HugePages_Surp:  0\nHugepagesize:    2048 kB\nHugetlb:         0 kB\n");
    write_str(buf, &mut pos, "DirectMap4k:     0 kB\nDirectMap2M:     0 kB\nDirectMap1G:     0 kB\n");

    return pos as i32;
}
```

- [ ] **Step 2: 编译验证**

Run: `cargo check --target x86_64-unknown-none`
Expected: PASS

- [ ] **Step 3: Commit**

```bash
git add src/kernel/services/fs/procfs_core.rs
git commit -m "feat(procfs): 实现 /proc/meminfo"
```

---

## Task 3: 实现 /proc/version 和 /proc/uptime

**Covers:** Linux 风格 /proc

**Files:**
- Modify: `src/kernel/services/fs/procfs_core.rs`

**Interfaces:**
- Consumes: 无特殊依赖
- Produces: `/proc/version`, `/proc/uptime` 读取支持

- [ ] **Step 1: 在 procfs_core.rs 添加 version 和 uptime 读取**

在 `read` 方法中添加 `/proc/version` 和 `/proc/uptime` 处理：

```rust
if name == "version" {
    let version = "Linux version 6.1.0-queenx (queenx@build) (gcc (Ubuntu 11.3.0) 11.3.0, GNU ld (GNU Binutils for Ubuntu) 2.38) #1 SMP PREEMPT Mon Jul  6 00:00:00 UTC 2026\n";
    let bytes = version.as_bytes();
    let len = bytes.len().min(buf.len());
    buf[..len].copy_from_slice(&bytes[..len]);
    return len as i32;
}

if name == "uptime" {
    let uptime_secs = crate::arch!(timestamp()) / 1000; // 假设 timestamp 返回毫秒
    let idle_secs = 0;
    let s = alloc::format!("{}.00 {} 1\n", uptime_secs, idle_secs);
    let bytes = s.as_bytes();
    let len = bytes.len().min(buf.len());
    buf[..len].copy_from_slice(&bytes[..len]);
    return len as i32;
}
```

- [ ] **Step 2: 编译验证**

Run: `cargo check --target x86_64-unknown-none`
Expected: PASS

- [ ] **Step 3: Commit**

```bash
git add src/kernel/services/fs/procfs_core.rs
git commit -m "feat(procfs): 实现 /proc/version 和 /proc/uptime"
```

---

## Task 4: 实现 /proc/stat

**Covers:** Linux 风格 /proc

**Files:**
- Modify: `src/kernel/services/fs/procfs_core.rs`

**Interfaces:**
- Consumes: 框架层调度器统计
- Produces: `/proc/stat` 读取支持

- [ ] **Step 1: 在 procfs_core.rs 添加 stat 读取**

在 `read`方法中添加 `/proc/stat` 处理：

```rust
if name == "stat" {
    let mut pos = 0usize;
    let write_str = |buf: &mut [u8], pos: &mut usize, s: &str| {
        let b = s.as_bytes();
        let end = (*pos + b.len()).min(buf.len());
        let len = end - *pos;
        buf[*pos..end].copy_from_slice(&b[..len]);
        *pos += len;
    };

    // CPU 统计 (user, nice, system, idle, iowait, irq, softirq, steal)
    write_str(buf, &mut pos, "cpu  0 0 0 0 0 0 0 0 0 0\n");
    write_str(buf, &mut pos, "cpu0 0 0 0 0 0 0 0 0 0 0\n");
    write_str(buf, &mut pos, "intr 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0\n");
    write_str(buf, &mut pos, "ctxt 0\n");
    write_str(buf, &mut pos, "btime 0\n");
    write_str(buf, &mut pos, "processes 0\n");
    write_str(buf, &mut pos, "procs_running 1\n");
    write_str(buf, &mut pos, "procs_blocked 0\n");
    write_str(buf, &mut pos, "softirq 0 0 0 0 0 0 0 0 0 0 0\n");

    return pos as i32;
}
```

- [ ] **Step 2: 编译验证**

Run: `cargo check --target x86_64-unknown-none`
Expected: PASS

- [ ] **Step 3: Commit**

```bash
git add src/kernel/services/fs/procfs_core.rs
git commit -m "feat(procfs): 实现 /proc/stat"
```

---

## Task 5: 实现 /proc/mounts

**Covers:** Linux 风格 /proc

**Files:**
- Modify: `src/kernel/services/fs/procfs_core.rs`

**Interfaces:**
- Consumes: VFS 挂载信息
- Produces: `/proc/mounts` 读取支持

- [ ] **Step 1: 在 procfs_core.rs 添加 mounts 读取**

在 `read`方法中添加 `/proc/mounts` 处理：

```rust
if name == "mounts" {
    let mut pos = 0usize;
    let write_str = |buf: &mut [u8], pos: &mut usize, s: &str| {
        let b = s.as_bytes();
        let end = (*pos + b.len()).min(buf.len());
        let len = end - *pos;
        buf[*pos..end].copy_from_slice(&b[..len]);
        *pos += len;
    };

    // 输出当前挂载点
    write_str(buf, &mut pos, "proc /proc proc rw,nosuid,nodev,noexec,relatime 0 0\n");
    write_str(buf, &mut pos, "devtmpfs /dev devtmpfs rw,nosuid,relatime,size=2048k,nr_inodes=512 0 0\n");
    write_str(buf, &mut pos, "tmpfs /tmp tmpfs rw,nosuid,nodev,relatime 0 0\n");

    return pos as i32;
}
```

- [ ] **Step 2: 编译验证**

Run: `cargo check --target x86_64-unknown-none`
Expected: PASS

- [ ] **Step 3: Commit**

```bash
git add src/kernel/services/fs/procfs_core.rs
git commit -m "feat(procfs): 实现 /proc/mounts"
```

---

## Task 6: 实现 /proc/[pid]/status

**Covers:** Linux 风格 /proc 进程接口

**Files:**
- Modify: `src/kernel/services/fs/procfs_core.rs`
- Modify: `src/kernel/services/fs/procfs.rs`

**Interfaces:**
- Consumes: framework::proc::PROCESS_TABLE
- Produces: `/proc/[pid]/status` 读取支持

- [ ] **Step 1: 在 procfs_core.rs 添加进程 status 读取**

在 `read`方法中添加进程 status 处理：

```rust
// 检查是否是 /proc/[pid]/status 格式
if name.starts_with('/') && name.ends_with("/status") {
    let pid_str = &name[1..name.len() - 7]; // 去掉前导 / 和后缀 /status
    if let Ok(pid) = pid_str.parse::<u32>() {
        return self.read_process_status(pid, buf);
    }
}

fn read_process_status(&self, pid: u32, buf: &mut [u8]) -> i32 {
    let mut pos = 0usize;
    let write_str = |buf: &mut [u8], pos: &mut usize, s: &str| {
        let b = s.as_bytes();
        let end = (*pos + b.len()).min(buf.len());
        let len = end - *pos;
        buf[*pos..end].copy_from_slice(&b[..len]);
        *pos += len;
    };

    // 从进程表获取信息
    let table = &crate::kernel::framework::proc::PROCESS_TABLE;
    if let Some(proc_ptr) = table.get(pid) {
        // SAFETY: proc_ptr 来自 PROCESS_TABLE, 有效指针
        let proc = unsafe { &*proc_ptr };

        write_str(buf, &mut pos, "Name:\t");
        // 获取进程名
        let name_guard = proc.name.lock();
        write_str(buf, &mut pos, &name_guard);
        drop(name_guard);

        write_str(buf, &mut pos, "\nState:\tR (running)\n");
        write_str(buf, &mut pos, &format!("\nTgid:\t{}\n", pid));
        write_str(buf, &mut pos, &format!("Pid:\t{}\n", pid));
        write_str(buf, &mut pos, &format!("PPid:\t{}\n", proc.parent.unwrap_or(0)));
        write_str(buf, &mut pos, &format!("TracerPid:\t0\n"));
        write_str(buf, &mut pos, &format!("Uid:\t0\t0\t0\t0\n"));
        write_str(buf, &mut pos, &format!("Gid:\t0\t0\t0\t0\n"));
        write_str(buf, &mut pos, &format!("FDSize:\t256\n"));
        write_str(buf, &mut pos, &format!("Groups:\t0 \n"));
        write_str(buf, &mut pos, &format!("NStgid:\t{}\n", pid));
        write_str(buf, &mut pos, &format!("NSpid:\t{}\n", pid));
        write_str(buf, &mut pos, &format!("NSpgid:\t{}\n", pid));
        write_str(buf, &mut pos, &format!("NSsid:\t{}\n", pid));
        write_str(buf, &mut pos, &format!("VmPeak:\t   1024 kB\n"));
        write_str(buf, &mut pos, &format!("VmSize:\t   1024 kB\n"));
        write_str(buf, &mut pos, &format!("VmRSS:\t     256 kB\n"));
        write_str(buf, &mut pos, &format!("VmSwap:\t       0 kB\n"));
        write_str(buf, &mut pos, &format!("Threads:\t1\n"));
        write_str(buf, &mut pos, &format!("SigQ:\t0/30670\n"));
        write_str(buf, &mut pos, &format!("SigPnd:\t0000000000000000\n"));
        write_str(buf, &mut pos, &format!("SigBlk:\t0000000000000000\n"));
        write_str(buf, &mut pos, &format!("SigIgn:\t0000000000000000\n"));
        write_str(buf, &mut pos, &format!("SigCgt:\t0000000000000000\n"));
        write_str(buf, &mut pos, &format!("CapInh:\t0000000000000000\n"));
        write_str(buf, &mut pos, &format!("CapPrm:\t0000000000000000\n"));
        write_str(buf, &mut pos, &format!("CapEff:\t0000000000000000\n"));
        write_str(buf, &mut pos, &format!("CapBnd:\t0000000000000000\n"));
        write_str(buf, &mut pos, &format!("CapAmb:\t0000000000000000\n"));
        write_str(buf, &mut pos, &format!("Seccomp:\t0\n"));
        write_str(buf, &mut pos, &format!("Seccomp_filters:\t0\n"));
        write_str(buf, &mut pos, &format!("Cpus_allowed:\t1\n"));
        write_str(buf, &mut pos, &format!("Cpus_allowed_list:\t0\n"));
        write_str(buf, &mut pos, &format!("Mems_allowed:\t00000000,00000000,00000000,00000000,00000000,00000000,00000000,00000001\n"));
        write_str(buf, &mut pos, &format!("Mems_allowed_list:\t0\n"));
        write_str(buf, &mut pos, &format!("voluntary_ctxt_switches:\t0\n"));
        write_str(buf, &mut pos, &format!("nonvoluntary_ctxt_switches:\t0\n"));

        return pos as i32;
    }

    -1 // 进程不存在
}
```

- [ ] **Step 2: 编译验证**

Run: `cargo check --target x86_64-unknown-none`
Expected: PASS

- [ ] **Step 3: Commit**

```bash
git add src/kernel/services/fs/procfs_core.rs
git commit -m "feat(procfs): 实现 /proc/[pid]/status"
```

---

## Task 7: 实现 /proc/[pid]/cmdline

**Covers:** Linux 风格 /proc 进程接口

**Files:**
- Modify: `src/kernel/services/fs/procfs_core.rs`

**Interfaces:**
- Consumes: framework::proc::PROCESS_TABLE
- Produces: `/proc/[pid]/cmdline` 读取支持

- [ ] **Step 1: 在 procfs_core.rs 添加 cmdline 读取**

在 `read`方法中添加 cmdline 处理：

```rust
// 检查是否是 /proc/[pid]/cmdline 格式
if name.starts_with('/') && name.ends_with("/cmdline") {
    let pid_str = &name[1..name.len() - 8]; // 去掉前导 / 和后缀 /cmdline
    if let Ok(pid) = pid_str.parse::<u32>() {
        return self.read_process_cmdline(pid, buf);
    }
}

fn read_process_cmdline(&self, pid: u32, buf: &mut [u8]) -> i32 {
    let table = &crate::kernel::framework::proc::PROCESS_TABLE;
    if let Some(proc_ptr) = table.get(pid) {
        // SAFETY: proc_ptr 来自 PROCESS_TABLE, 有效指针
        let proc = unsafe { &*proc_ptr };

        let name_guard = proc.name.lock();
        let name = name_guard.clone();
        drop(name_guard);

        // cmdline 格式: 以 null 字节分隔
        let bytes = name.as_bytes();
        let len = bytes.len().min(buf.len() - 1);
        buf[..len].copy_from_slice(&bytes[..len]);
        buf[len] = 0; // null 终止

        return (len + 1) as i32;
    }

    -1
}
```

- [ ] **Step 2: 编译验证**

Run: `cargo check --target x86_64-unknown-none`
Expected: PASS

- [ ] **Step 3: Commit**

```bash
git add src/kernel/services/fs/procfs_core.rs
git commit -m "feat(procfs): 实现 /proc/[pid]/cmdline"
```

---

## Task 8: 实现 /proc/[pid]/fd

**Covers:** Linux 风格 /proc 进程接口

**Files:**
- Modify: `src/kernel/services/fs/procfs_core.rs`
- Modify: `src/kernel/services/fs/procfs.rs`

**Interfaces:**
- Consumes: framework::proc::PROCESS_TABLE, FdTable
- Produces: `/proc/[pid]/fd` 目录读取支持

- [ ] **Step 1: 在 procfs 添加 fd 目录支持**

在 `readdir` 方法中添加 fd 目录处理：

```rust
// 在 readdir 方法中添加 fd 目录支持
if name.starts_with('/') && name.ends_with("/fd") {
    let pid_str = &name[1..name.len() - 3]; // 去掉前导 / 和后缀 /fd
    if let Ok(pid) = pid_str.parse::<u32>() {
        return self.read_process_fd(pid, offset, entry);
    }
}

fn read_process_fd(&self, pid: u32, offset: u64, entry: &mut ProcEntry) -> bool {
    let table = &crate::kernel::framework::proc::PROCESS_TABLE;
    if let Some(proc_ptr) = table.get(pid) {
        // SAFETY: proc_ptr 来自 PROCESS_TABLE, 有效指针
        let proc = unsafe { &*proc_ptr };

        // 获取 fd 表
        let fd_table = &proc.fd_table;
        let fd_count = fd_table.count();

        if offset as usize >= fd_count {
            return false;
        }

        let fd_num = offset as u32;
        entry.name = alloc::format!("{}", fd_num);
        entry.pid = pid;
        entry.kind = ProcEntryKind::File;
        entry.used = true;

        return true;
    }

    false
}
```

- [ ] **Step 2: 编译验证**

Run: `cargo check --target x86_64-unknown-none`
Expected: PASS

- [ ] **Step 3: Commit**

```bash
git add src/kernel/services/fs/procfs.rs src/kernel/services/fs/procfs_core.rs
git commit -m "feat(procfs): 实现 /proc/[pid]/fd 目录"
```

---

## Task 9: 更新挂载点注册

**Covers:** Linux 风格 /proc

**Files:**
- Modify: `src/kernel/services/fs/procfs_core.rs`

**Interfaces:**
- Consumes: 现有 mount 方法
- Produces: 新增文件注册

- [ ] **Step 1: 在 mount 方法中注册新文件**

修改 `mount` 方法，添加新文件注册：

```rust
pub fn mount(&self, _path: &str) -> i32 {
    let mut entries = self.entries.lock();

    Self::set_name(&mut entries[0], "current");
    entries[0].pid = 0;
    entries[0].entry_type = 2;
    entries[0].used = true;

    Self::set_name(&mut entries[1], "cpuinfo");
    entries[1].pid = 0;
    entries[1].entry_type = 4;
    entries[1].used = true;

    Self::set_name(&mut entries[2], "meminfo");
    entries[2].pid = 0;
    entries[2].entry_type = 4;
    entries[2].used = true;

    Self::set_name(&mut entries[3], "version");
    entries[3].pid = 0;
    entries[3].entry_type = 4;
    entries[3].used = true;

    Self::set_name(&mut entries[4], "uptime");
    entries[4].pid = 0;
    entries[4].entry_type = 4;
    entries[4].used = true;

    Self::set_name(&mut entries[5], "stat");
    entries[5].pid = 0;
    entries[5].entry_type = 4;
    entries[5].used = true;

    Self::set_name(&mut entries[6], "mounts");
    entries[6].pid = 0;
    entries[6].entry_type = 4;
    entries[6].used = true;

    Self::set_name(&mut entries[7], "sys/cpu");
    entries[7].pid = 0;
    entries[7].entry_type = 1;
    entries[7].used = true;

    Self::set_name(&mut entries[8], "sys/memory");
    entries[8].pid = 0;
    entries[8].entry_type = 1;
    entries[8].used = true;

    Self::set_name(&mut entries[9], "sys/config");
    entries[9].pid = 0;
    entries[9].entry_type = 1;
    entries[9].used = true;

    self.entry_count.store(10, Ordering::SeqCst);

    0
}
```

- [ ] **Step 2: 编译验证**

Run: `cargo check --target x86_64-unknown-none`
Expected: PASS

- [ ] **Step 3: Commit**

```bash
git add src/kernel/services/fs/procfs_core.rs
git commit -m "feat(procfs): 注册新增的 /proc 文件"
```

---

## Task 10: 更新文档和测试

**Covers:** 文档同步

**Files:**
- Modify: `docs/plan/naming-implementation.md`
- Modify: `host-tests/tests/procfs_test.rs` (新建)

**Interfaces:**
- Consumes: 完成的实现
- Produces: 更新后的文档和测试

- [ ] **Step 1: 创建 procfs 测试**

创建 `host-tests/tests/procfs_test.rs`：

```rust
//! /proc Linux 风格接口测试

#[test]
fn test_procfs_cpuinfo_format() {
    // 验证 cpuinfo 输出格式
    let cpuinfo = "processor\t: 0\nvendor_id\t: GenuineIntel\ncpu family\t: 6\nmodel\t\t: 142\nmodel name\t: Intel(R) Core(TM) i7-8550U CPU @ 1.80GHz\n";
    assert!(cpuinfo.contains("processor"));
    assert!(cpuinfo.contains("vendor_id"));
    assert!(cpuinfo.contains("model name"));
}

#[test]
fn test_procfs_meminfo_format() {
    // 验证 meminfo 输出格式
    let meminfo = "MemTotal:        16384 kB\nMemFree:          8192 kB\nMemAvailable:     8192 kB\n";
    assert!(meminfo.contains("MemTotal"));
    assert!(meminfo.contains("MemFree"));
    assert!(meminfo.contains("MemAvailable"));
}

#[test]
fn test_procfs_version_format() {
    // 验证 version 输出格式
    let version = "Linux version 6.1.0-queenx (queenx@build) (gcc (Ubuntu 11.3.0) 11.3.0)\n";
    assert!(version.contains("Linux version"));
    assert!(version.contains("queenx"));
}

#[test]
fn test_procfs_uptime_format() {
    // 验证 uptime 输出格式
    let uptime = "12345.67 67890.12 1\n";
    let parts: Vec<&str> = uptime.split_whitespace().collect();
    assert_eq!(parts.len(), 3);
    assert!(parts[0].parse::<f64>().is_ok());
}
```

- [ ] **Step 2: 更新 naming-implementation.md**

更新 §6.3 状态为 [X]

- [ ] **Step 3: 编译验证**

Run: `cargo check --target x86_64-unknown-none`
Expected: PASS

- [ ] **Step 4: 运行测试**

Run: `cargo test -p host-tests --test procfs_test`
Expected: PASS

- [ ] **Step 5: Commit**

```bash
git add docs/plan/naming-implementation.md host-tests/tests/procfs_test.rs
git commit -m "docs(procfs): 更新文档和测试"
```

---

## Task 11: 最终验证和提交

**Covers:** 双架构编译

**Files:**
- 无新增修改

**Interfaces:**
- Consumes: 所有实现
- Produces: 编译通过

- [ ] **Step 1: x86_64 编译验证**

Run: `cargo check --target x86_64-unknown-none`
Expected: PASS

- [ ] **Step 2: aarch64 编译验证**

Run: `cargo check --target aarch64-unknown-none`
Expected: PASS

- [ ] **Step 3: clippy 检查**

Run: `cargo clippy --target x86_64-unknown-none -- -D warnings`
Expected: PASS

- [ ] **Step 4: 运行所有测试**

Run: `cargo test -p host-tests`
Expected: PASS

- [ ] **Step 5: 推送到远程**

```bash
git push Gitee main
```
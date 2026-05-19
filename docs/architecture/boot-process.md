# 启动流程

> 从BIOS到用户态Shell的完整启动过程

---

## 🚀 启动流程概览

```
BIOS/UEFI (实模式)
    ↓
GRUB2 Bootloader
    ↓
Multiboot2 Header
    ↓
boot.asm (实模式 → 保护模式)
    ↓
entry.asm (长模式设置)
    ↓
kernel_main() (C初始化)
    ↓
Rust初始化 (核心子系统)
    ↓
启动用户态init进程
    ↓
用户态Shell (axsh)
```

---

## 📋 详细启动步骤

### 1. BIOS/UEFI阶段

**职责**: 硬件自检、加载引导程序

**流程**:
```
上电
    ↓
POST (加电自检)
    ↓
检测内存、CPU、设备
    ↓
读取引导设备
    ↓
加载MBR (主引导记录)
    ↓
跳转到GRUB2
```

**关键点**:
- CPU处于实模式（16位）
- 内存限制：1MB可访问
- 中断向量表在0000:0000

---

### 2. GRUB2阶段

**职责**: 加载内核镜像、提供启动菜单

**配置文件**: `/boot/grub/grub.cfg`

```cfg
set timeout=0
set default=0

menuentry "AntX" {
    multiboot2 /boot/kernel.bin
    module /bin/init
    module /bin/axsh
}
```

**Multiboot2协议**:
- 内核镜像加载到内存
- 传递引导信息（内存映射、命令行）
- 设置多引导信息结构

**内存布局**:
```
0x00000000 - 0x000FFFFF: 实模式内存
0x00100000 - 0xXXXXXXXX: 内核镜像
0xXXXXXXXX - 0xXXXXXXXX: 多引导信息
```

---

### 3. boot.asm阶段

**文件**: `src/kernel/boot/boot.asm`

**职责**: 实模式到保护模式转换

**关键步骤**:

```asm
; 1. 检查Multiboot2魔数
cmp eax, 0x36D76289
jne .error

; 2. 保存多引导信息指针
mov [multiboot_info], ebx

; 3. 设置临时GDT
lgdt [gdt_descriptor]

; 4. 切换到保护模式
mov eax, cr0
or eax, 1
mov cr0, eax

; 5. 远跳转到32位代码
jmp 0x08:protected_mode
```

**GDT设置**:
```
空描述符 (0x00)
代码段描述符 (0x08): 基址=0, 界限=4GB, DPL=0
数据段描述符 (0x10): 基址=0, 界限=4GB, DPL=0
```

---

### 4. entry.asm阶段

**文件**: `src/kernel/boot/entry.asm`

**职责**: 保护模式到长模式转换

**关键步骤**:

```asm
; 1. 设置页表（临时映射）
mov eax, page_table
mov cr3, eax

; 2. 启用PAE (物理地址扩展)
mov eax, cr4
or eax, (1 << 5)
mov cr4, eax

; 3. 启用长模式
mov ecx, 0xC0000080
rdmsr
or eax, (1 << 8)
wrmsr

; 4. 启用分页
mov eax, cr0
or eax, (1 << 31)
mov cr0, eax

; 5. 远跳转到64位代码
jmp 0x08:long_mode_start
```

**页表设置**:
- PML4 (页映射级别4)
- PDPT (页目录指针表)
- PD (页目录)
- PT (页表)

**映射**:
- 0x00000000 → 0x00000000 (恒等映射)
- 0xFFFFFFFF80000000 → 0x00000000 (内核映射)

---

### 5. kernel_main()阶段

**文件**: `src/kernel/main.c`

**职责**: C环境初始化、调用Rust初始化

**初始化顺序**:

```c
void kernel_main(multiboot_info_t *mbi) {
    // 1. 清屏
    vga_clear();
    
    // 2. 初始化串口（日志输出）
    serial_init();
    
    // 3. 初始化GDT
    gdt_init();
    
    // 4. 初始化IDT
    idt_init();
    
    // 5. 初始化PIC
    pic_init();
    
    // 6. 初始化定时器
    timer_init();
    
    // 7. 初始化键盘
    keyboard_init();
    
    // 8. 调用Rust初始化
    antx_init(mbi);
}
```

**关键点**:
- 此时中断已禁用
- 使用临时堆栈
- VGA用于早期输出

---

### 6. Rust初始化阶段

**文件**: `src/rust/src/lib.rs`

**职责**: 核心子系统初始化

**初始化顺序**:

```rust
#[no_mangle]
pub extern "C" fn antx_init(mbi: *const MultibootInfo) {
    // 1. 解析多引导信息
    let boot_info = parse_multiboot_info(mbi);
    
    // 2. 初始化PMM (物理内存管理器)
    pmm::init(&boot_info.memory_map);
    klog!("PMM initialized");
    
    // 3. 初始化VMM (虚拟内存管理器)
    vmm::init();
    klog!("VMM initialized");
    
    // 4. 初始化堆管理器
    heap::init();
    klog!("Heap initialized");
    
    // 5. 初始化IDT (中断描述符表)
    idt::init();
    klog!("IDT initialized");
    
    // 6. 初始化栏栈恢复
    barrier::init();
    klog!("Barrier initialized");
    
    // 7. 初始化VFS (虚拟文件系统)
    vfs::init();
    klog!("VFS initialized");
    
    // 8. 挂载文件系统
    mount_filesystems();
    klog!("Filesystems mounted");
    
    // 9. 初始化PWID (安全子系统)
    pwid::init();
    klog!("PWID initialized");
    
    // 10. 初始化进程管理器
    process::init();
    klog!("Process manager initialized");
    
    // 11. 初始化调度器
    scheduler::init();
    klog!("Scheduler initialized");
    
    // 12. 启用中断
    unsafe { asm!("sti"); }
    
    // 13. 启动用户态init进程
    start_user_init();
}
```

---

### 7. 文件系统挂载

**挂载顺序**:

```rust
fn mount_filesystems() {
    // 1. 挂载RamFS为根文件系统
    vfs::mount("/", FsType::RamFs);
    
    // 2. 挂载DevFS
    vfs::mount("/dev", FsType::DevFs);
    
    // 3. 挂载ProcFS
    vfs::mount("/proc", FsType::ProcFs);
    
    // 4. 尝试挂载HvFS (磁盘文件系统)
    if hvfs::check_disk() {
        vfs::mount("/mnt/disk", FsType::HvFs);
    }
}
```

**创建设备节点**:
```
/dev/null   - 空设备
/dev/zero   - 零设备
/dev/console - 控制台
/dev/tty0   - 终端
```

---

### 8. 启动用户态init进程

**文件**: `src/user/rust/src/bin/init.rs`

**流程**:

```c
void start_user_init() {
    // 1. 加载init ELF文件
    elf_t *init_elf = load_elf("/bin/init");
    
    // 2. 创建init进程
    process_t *init = process_create("init", init_elf);
    
    // 3. 设置进程属性
    init->pid = 1;
    init->pwid = PWID_SYSTEM;
    init->priority = 0;
    
    // 4. 设置页表
    setup_user_page_table(init);
    
    // 5. 设置用户栈
    setup_user_stack(init);
    
    // 6. 切换到用户态
    switch_to_user_mode(init);
}
```

**init进程职责**:
- 启动系统服务
- 启动Shell
- 处理孤儿进程

---

### 9. 用户态Shell阶段

**文件**: `src/user/rust/src/bin/shell.rs`

**启动流程**:

```c
int main(int argc, char **argv) {
    // 1. 初始化Shell环境
    shell_init();
    
    // 2. 显示欢迎信息
    print_welcome();
    
    // 3. 主循环
    while (running) {
        // 显示提示符
        print_prompt();
        
        // 读取命令
        char *cmd = read_command();
        
        // 解析并执行
        execute_command(cmd);
    }
    
    return 0;
}
```

**支持的命令**:
- `help` - 显示帮助
- `ls` - 列出文件
- `cd` - 切换目录
- `cat` - 显示文件内容
- `mkdir` - 创建目录
- `rm` - 删除文件
- `login` - 登录
- `logout` - 登出

---

## 📊 启动时间分析

| 阶段 | 时间 | 说明 |
|------|------|------|
| BIOS/UEFI | ~100ms | 硬件自检 |
| GRUB2 | ~50ms | 加载内核 |
| boot.asm | < 1ms | 模式切换 |
| entry.asm | < 1ms | 长模式设置 |
| kernel_main() | ~10ms | C初始化 |
| Rust初始化 | ~100ms | 子系统初始化 |
| 用户态启动 | ~50ms | init + Shell |
| **总计** | **~300ms** | QEMU虚拟环境 |

---

## 🔧 启动参数

### 内核命令行参数

通过GRUB配置传递：

```
multiboot2 /boot/kernel.bin --param1=value1 --param2=value2
```

**支持的参数**:

| 参数 | 说明 | 默认值 |
|------|------|--------|
| `root=` | 根文件系统设备 | `/dev/ram0` |
| `init=` | init进程路径 | `/bin/init` |
| `console=` | 控制台设备 | `/dev/console` |
| `loglevel=` | 日志级别 | `INFO` |
| `mem=` | 内存限制 | 全部内存 |

---

## 🐛 启动调试

### 早期调试（内核启动前）

**使用串口输出**:

```asm
; 在boot.asm中
mov al, 'A'
mov dx, 0x3F8    ; COM1端口
out dx, al
```

### 内核调试

**启用调试输出**:

```rust
#[cfg(feature = "debug_boot")]
klog_debug!("Boot stage: {}", stage);
```

**查看启动日志**:
```bash
make run 2>&1 | tee boot.log
```

### 常见启动问题

1. **内核未加载**
   - 检查GRUB配置
   - 检查内核镜像路径

2. **长模式设置失败**
   - CPU不支持长模式
   - 页表设置错误

3. **内存初始化失败**
   - 多引导信息解析错误
   - 内存映射损坏

4. **文件系统挂载失败**
   - 磁盘未格式化
   - 驱动未初始化

5. **init进程启动失败**
   - ELF文件损坏
   - 内存不足

---

## 📝 启动日志示例

```
[0.000000] [INFO] [BOOT] QueenX starting...
[0.000010] [INFO] [BOOT] Multiboot2 magic: 0x36D76289
[0.000020] [INFO] [BOOT] Memory: 512MB available
[0.000030] [INFO] [BOOT] Switching to long mode...
[0.000040] [INFO] [BOOT] Long mode enabled
[0.000050] [INFO] [BOOT] GDT initialized
[0.000060] [INFO] [BOOT] IDT initialized
[0.000070] [INFO] [BOOT] PIC initialized
[0.000080] [INFO] [BOOT] Timer initialized (100Hz)
[0.000090] [INFO] [BOOT] Keyboard initialized
[0.000100] [INFO] [BOOT] Entering Rust initialization...
[0.000110] [INFO] [PMM] Initialized (131072 pages available)
[0.000120] [INFO] [VMM] Initialized
[0.000130] [INFO] [HEAP] Initialized (16MB heap)
[0.000140] [INFO] [IDT] Rust handlers registered
[0.000150] [INFO] [BARRIER] Initialized (4 recovery domains)
[0.000160] [INFO] [VFS] Initialized
[0.000170] [INFO] [VFS] Mounted RamFS at /
[0.000180] [INFO] [VFS] Mounted DevFS at /dev
[0.000190] [INFO] [VFS] Mounted ProcFS at /proc
[0.000200] [INFO] [PWID] Initialized
[0.000210] [INFO] [PROCESS] Initialized
[0.000220] [INFO] [SCHEDULER] Initialized
[0.000230] [INFO] [BOOT] Interrupts enabled
[0.000240] [INFO] [BOOT] Starting user init...
[0.000250] [INFO] [PROCESS] Created init process (PID=1)
[0.000260] [INFO] [BOOT] Switching to user mode...
[0.000270] [INFO] [INIT] Starting...
[0.000280] [INFO] [INIT] Mounting /tmp
[0.000290] [INFO] [INIT] Starting axsh...
[0.000300] [INFO] [AXSH] Welcome to AntX!
```

---

## 🔮 未来改进

### 计划中的改进

1. **快速启动**
   - 并行初始化子系统
   - 延迟加载非关键模块
   - 目标：< 100ms

2. **UEFI支持**
   - 直接UEFI启动
   - 无需GRUB
   - 安全启动支持

3. **休眠支持**
   - 内核休眠到磁盘
   - 快速恢复
   - 状态保留

4. **启动验证**
   - 内核签名验证
   - 完整性检查
   - 安全启动

---

**最后更新**: 2026-05-18

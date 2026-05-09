#include "kernel.h"
#include "vfs.h"
#include "user_proc.h"
#include "smart_mount.h"
#include "timer.h"
#include "module_check.h"
#include "klog.h"
#include "proc_ffi.h"
#include "hvfs_ffi.h"
#include "kmalloc.h"
#include "version_registry.h"
#include "cpu.h"
#include "pci.h"
#include "dma.h"
#include "serial.h"
#include "ata.h"
#include "keyboard.h"
#include "pwid.h"
#include "hvfs.h"
#include "syscall.h"
#ifdef KERNEL_TEST
#include "kernel_test.h"
#endif

void ramfs_init(void);
void diskfs_init(void);
void devfs_init(void);
void procfs_init(void);

extern unsigned char build_user_init_bin[];
extern unsigned int build_user_init_bin_len;

extern char _kernel_end[];
extern char _kernel_end_phys[];

void panic(const char *msg) {
    klog_kern_crit("PANIC: %s", msg);
    klog_dump();
    klog_kern_crit("System halted");
    
    while (1) {
        __asm__ volatile ("hlt");
    }
}

void enable_interrupts(void) {
    __asm__ volatile ("sti");
}

void disable_interrupts(void) {
    __asm__ volatile ("cli");
}

void interrupt_idle(void) {
    if (proc_has_runnable()) {
        extern uint32_t sched_schedule_internal(void);
        uint32_t pid = sched_schedule_internal();
        if (pid > 0) {
            extern int user_proc_enter_by_pid(uint32_t pid);
            user_proc_enter_by_pid(pid);
        }
        return;
    }
    
    __asm__ volatile (
        "sti\n"
        "hlt\n"
        "cli\n"
        ::: "memory"
    );
}

static void start_user_init(void) {
    klog_boot("[INIT] Starting user-space init process...");

    int pid = user_proc_load_elf_from_memory(build_user_init_bin, build_user_init_bin_len, 0);

    if (pid < 0) {
        klog_init_err("Failed to load init process! (pid=%d)", pid);
        klog_kern_crit("============================================");
        klog_kern_crit("  FATAL: User-space init failed to load");
        klog_kern_crit("  System is running in kernel-only mode");
        klog_kern_crit("  Check: init binary size / VMM / RamFS");
        klog_kern_crit("============================================");
        return;
    }
    
    klog_boot("[INIT] Init process loaded with PID: %d", pid);
    
    sched_add_internal((uint32_t)pid);
    
    klog_init_msg("Init process started with PID: %d", pid);
}

void kernel_main(void) {
    serial_init(SERIAL_COM1);
    klog_init();
    
    klog_boot("");
    klog_boot("AntX Operating System");
    klog_boot("Copyright (c) 2026 Anfer's AntX Project");
    klog_boot("========================================");
    
    klog_boot("Initializing kernel...");
    
    MODULE_CHECK("GDT", gdt_init);
    MODULE_CHECK("IDT", idt_init);
    
    /* 初始化 AMD64 CPU 驱动 (特性检测/MSR/缓存信息) */
    if (cpu_init() != 0) {
        klog_kern_warn("CPU driver initialization failed, continuing...");
    } else {
        klog_kern("CPU driver initialized successfully");
    }
    pmm_init(MEMORY_SIZE, (uint64_t)_kernel_end_phys);
    klog_mem("PMM basic init complete");
    
    /* 初始化内核堆 (Rust kmalloc) */
    /* 堆起始地址：内核结束后的下一个页边界 */
    /* 初始大小：16 MB (可根据需要调整) */
    {
        uint64_t heap_start = ((uint64_t)_kernel_end + PAGE_SIZE - 1) & ~(PAGE_SIZE - 1);
        uint64_t heap_initial_size = 32 * 1024 * 1024;  /* 32 MB */
        kmalloc_init(heap_start, heap_initial_size);
    }
    klog_mem("Kernel heap initialized");
    
    pmm_init_bitmap(32 * 1024 * 1024);
    if (pmm_get_free_pages() == 0) {
        panic("PMM initialization failed: no free pages");
    }
    {
        uint64_t free_pages = pmm_get_free_pages();
        klog_mem("PMM: %d pages free (%d MB)", free_pages, free_pages * 4 / 1024);
    }
    
    MODULE_CHECK_VOID("VMM", vmm_init);
    
    MODULE_CHECK_VOID("Process Manager", process_init);
    MODULE_CHECK_VOID("Session Manager", session_init);
    MODULE_CHECK_VOID("Scheduler", scheduler_init);
    MODULE_CHECK_VOID("Scheduler", kernel_init);
    MODULE_CHECK_VOID("User Process Manager", user_proc_init);
    
    MODULE_CHECK_VOID("PWID Manager", pwid_init);
    
    MODULE_CHECK_VOID("ATA Driver", ata_init);
    
    MODULE_CHECK_VOID("HvFS", hvfs_init);
    
    MODULE_CHECK_VOID("VFS Layer", vfs_init);
    
    MODULE_CHECK_VOID("RamFS", ramfs_init);
    MODULE_CHECK_VOID("DiskFS", diskfs_init);
    MODULE_CHECK_VOID("DevFS", devfs_init);
    MODULE_CHECK_VOID("ProcFS", procfs_init);
    
    /* Smart Persistent Storage Mount */
    smart_mount_root();
    
    klog_fs("Root filesystem mounted (smart mode: %c)", get_persistent_mode());
    
    MODULE_CHECK_VOID("Syscall", syscall_init);
    MODULE_CHECK_VOID("Keyboard", keyboard_init);
    MODULE_CHECK_VOID("Timer", timer_init);
    
    extern void pwid_try_load(void);
    extern int pwid_any_identity_exists(void);
    pwid_try_load();
    
    if (!pwid_any_identity_exists()) {
        klog_kern("============================================");
        klog_kern("  GENESIS MODE: No identities found");
        klog_kern("  First boot detected — identity table empty");
        klog_kern("  Attempting automatic bootstrap...");
        klog_kern("============================================");
        
        /* Try Genesis with default password --- env var or compile-time default */
        extern int pwid_try_genesis(const char *password);
        const char *default_pw = "antx1234";
        int gen_rc = pwid_try_genesis(default_pw);
        if (gen_rc == 0) {
            klog_init_msg("Genesis root identity created (pw: %s)", default_pw);
            klog_init_msg("CHANGE THIS PASSWORD ON FIRST LOGIN!");
        } else if (gen_rc == 1) {
            klog_init_msg("Genesis: identity already exists (loaded from disk)");
        } else {
            klog_init_msg("Genesis failed — init will prompt for manual creation");
        }
    }
    
    klog_boot("System initialized");
    klog_boot("AntX is ready");

    /* ================================================================= */
    /*                    注册核心模块版本信息                          */
    /* ================================================================= */
    /*
     * 示例: 如何为新模块注册版本信息
     *
     * 格式: VERSION_REGISTER("模块名", 主版本, 次版本, 补丁版本, "描述", 类型)
     *
     * 模块类型:
     *   - MODULE_TYPE_CORE: 核心模块 (进程/内存/调度)
     *   - MODULE_TYPE_FS:   文件系统 (RamFS/HvFS/DiskFS)
     *   - MODULE_TYPE_DRIVER: 设备驱动 (E1000/键盘/VGA)
     *   - MODULE_TYPE_NET:  网络协议 (TCP/IP/UDP/lwIP)
     *   - MODULE_TYPE_SECURITY: 安全模块 (PWID/认证)
     *   - MODULE_TYPE_LIB:  库/框架 (IPC/VFS/syscall)
     *
     * 未来新增模块时，只需在初始化函数中添加一行即可:
     *   VERSION_REGISTER("lwIP", 2, 1, 0, "Lightweight TCP/IP Stack", MODULE_TYPE_NET);
     */

    version_register("QueenX", 0, 1, 0, "AntX Kernel Core", MODULE_TYPE_CORE);
    version_register("KLog", 1, 0, 0, "Kernel Logging System", MODULE_TYPE_LIB);
    version_register("VFS", 1, 0, 0, "Virtual File System Layer", MODULE_TYPE_LIB);
    version_register("RamFS", 1, 0, 0, "RAM-based File System", MODULE_TYPE_FS);
    version_register("HvFS", 2, 0, 0, "Hybrid Virtual File System", MODULE_TYPE_FS);
    version_register("PWID", 1, 0, 0, "Permission & Identity System", MODULE_TYPE_SECURITY);
    version_register("MLFQ", 1, 0, 0, "Multi-Level Feedback Queue Scheduler", MODULE_TYPE_CORE);
    version_register("DMA", 1, 0, 0, "Direct Memory Access Engine (Rust)", MODULE_TYPE_LIB);
    version_register("PCI", 1, 0, 0, "PCI Bus Driver", MODULE_TYPE_DRIVER);
    version_register("lwIP", 2, 2, 1, "Lightweight TCP/IP Stack", MODULE_TYPE_NET);
    version_register("E1000", 1, 0, 0, "Intel 82540EM NIC Driver", MODULE_TYPE_DRIVER);

    klog_init_msg("Module versions registered: %d modules", version_get_registered_count());
    
    klog_boot("[MAIN] Enabling interrupts...");
    enable_interrupts();
    klog_boot("[MAIN] Interrupts enabled");

#ifdef KERNEL_TEST
    klog_boot("[TEST MODE] Running kernel tests...");
    run_kernel_tests();
    klog_boot("[TEST MODE] Tests completed.");
    
    /* 如果有用户进程在调度队列中，进入 idle 循环让它们运行 */
    extern int proc_has_runnable(void);
    if (proc_has_runnable()) {
        klog_boot("[TEST MODE] Entering user-mode idle loop...");
        while (1) {
            interrupt_idle();
        }
    }
    
    klog_boot("[TEST MODE] No runnable user processes. Halting.");
    __asm__ volatile ("cli");
    while (1) {
        __asm__ volatile ("hlt");
    }
#else
    /* PCI init 在 Rust FFI 路径有已知崩溃，跳过;
     * e1000_probe() 自主执行直接 PCI 扫描 */
    /* MODULE_CHECK_VOID("PCI Bus", pci_init); */
    MODULE_CHECK_VOID("DMA Engine", dma_init);
    
    extern void qx_net_init(void);
    MODULE_CHECK_VOID("Network Stack", qx_net_init);

#ifdef CONFIG_SMP
    extern int smp_init(void);
    int smp_cpus = smp_init();
    if (smp_cpus <= 0) {
        klog_init_msg("SMP: single-core mode (no additional CPUs detected)");
    } else {
        klog_init_msg("SMP: %d CPUs online", smp_cpus + 1);
    }
#endif
    
    start_user_init();
    
    uint64_t idle_ticks = 0;
    while (1) {
        extern void e1000_poll(void);
        e1000_poll();

        /* Periodic PWID cleanup: expire tokens + trust entries */
        if (++idle_ticks % 1000 == 0) {
            extern void pwid_periodic_cleanup(void);
            pwid_periodic_cleanup();
        }

        interrupt_idle();
    }
#endif
}

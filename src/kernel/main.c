#include "kernel.h"
#include "vfs.h"
#include "user_proc.h"
#include "timer.h"
#include "module_check.h"
#include "klog.h"
#include "proc_ffi.h"
#include "hvfs_ffi.h"
#include "kmalloc.h"
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
    pr_crit("\n========================================\n");
    pr_crit("PANIC: %s\n", msg);
    pr_crit("========================================\n");
    
    klog_dump();
    
    pr_crit("\nSystem halted.\n");
    
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
    klog_init_msg("Starting user-space init process...\n");
    
    int pid = user_proc_load_elf_from_memory(build_user_init_bin, build_user_init_bin_len, 0);
    
    if (pid < 0) {
        pr_err("Failed to load init process!\n");
        return;
    }
    
    sched_add_internal((uint32_t)pid);
    
    klog_init_msg("Init process started with PID: %d\n", pid);
}

void kernel_main(void) {
    serial_init(SERIAL_COM1);
    klog_init();
    
    printk("\n");
    printk("AntX Operating System\n");
    printk("Copyright (c) 2026 Anfer's AntX Project\n");
    printk("========================================\n");
    
    klog_boot("Initializing kernel...\n");
    
    MODULE_CHECK("GDT", gdt_init);
    MODULE_CHECK("IDT", idt_init);
    
    pmm_init(MEMORY_SIZE, (uint64_t)_kernel_end_phys);
    klog_mem("PMM basic init complete\n");
    
    kmalloc_init();
    klog_mem("Kernel heap initialized\n");
    
    pmm_init_bitmap();
    if (pmm_get_free_pages() == 0) {
        panic("PMM initialization failed: no free pages");
    }
    {
        uint64_t free_pages = pmm_get_free_pages();
        klog_mem("PMM: %d pages free (%d MB)\n", free_pages, free_pages * 4 / 1024);
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
    
    klog_fs("Mounting root filesystem...\n");
    
    if (vfs_mount("/", "diskfs") != 0) {
        pr_warn("Using RamFS for root\n");
        if (vfs_mount("/", "ramfs") != 0) {
            panic("Failed to mount root filesystem");
        }
    }
    
    klog_fs("Root filesystem mounted\n");
    
    MODULE_CHECK_VOID("Syscall", syscall_init);
    MODULE_CHECK_VOID("Keyboard", keyboard_init);
    MODULE_CHECK_VOID("Timer", timer_init);
    
    extern void pwid_try_load(void);
    pwid_try_load();
    
    printk("\n");
    klog_init_msg("System initialized\n");
    printk("AntX is ready.\n");
    printk("\n");
    
    enable_interrupts();
    
#ifdef KERNEL_TEST
    printk("\n[TEST MODE] Running kernel tests...\n");
    run_kernel_tests();
    printk("\n[TEST MODE] Tests completed. Halting.\n");
    while (1) {
        __asm__ volatile ("hlt");
    }
#else
    start_user_init();
    
    while (1) {
        interrupt_idle();
    }
#endif
}

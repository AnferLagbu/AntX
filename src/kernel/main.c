#include "kernel.h"
#include "vfs.h"
#include "user_proc.h"
#include "timer.h"
#include "module_check.h"
#include "log_buffer.h"
#include "proc_rust.h"
#include "hvfs_rust.h"
#include "kmalloc.h"
#ifdef KERNEL_TEST
#include "kernel_test.h"
#endif

void rust_ramfs_init(void);
void rust_diskfs_init(void);
void rust_devfs_init(void);
void rust_procfs_init(void);

extern unsigned char build_user_init_bin[];
extern unsigned int build_user_init_bin_len;

extern char _kernel_end[];
extern char _kernel_end_phys[];

void panic(const char *msg) {
    serial_puts(SERIAL_COM1, "\n\n");
    serial_puts(SERIAL_COM1, "========================================\n");
    serial_puts(SERIAL_COM1, "PANIC: ");
    serial_puts(SERIAL_COM1, msg);
    serial_puts(SERIAL_COM1, "\n");
    serial_puts(SERIAL_COM1, "========================================\n");
    
    log_dump_all();
    
    serial_puts(SERIAL_COM1, "\nSystem halted.\n");
    
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
        scheduler_schedule();
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
    serial_puts(SERIAL_COM1, "[INIT] Starting user-space init process...\n");
    
    int pid = user_proc_load_elf_from_memory(build_user_init_bin, build_user_init_bin_len, 0);
    
    if (pid < 0) {
        serial_puts(SERIAL_COM1, "[INIT] Failed to load init process!\n");
        return;
    }
    
    serial_puts(SERIAL_COM1, "[INIT] Init process started with PID: ");
    serial_put_dec(SERIAL_COM1, pid);
    serial_puts(SERIAL_COM1, "\n");
}

void kernel_main(void) {
    __asm__ volatile (
        "mov $0x3F8, %%dx\n"
        "mov $'A', %%al\n"
        "out %%al, %%dx\n"
        "mov $'B', %%al\n"
        "out %%al, %%dx\n"
        "mov $'C', %%al\n"
        "out %%al, %%dx\n"
        "mov $'\\n', %%al\n"
        "out %%al, %%dx\n"
        : : : "ax", "dx"
    );
    
    serial_puts(SERIAL_COM1, "[DEBUG] kernel_main started\n");
    serial_puts(SERIAL_COM1, "[DEBUG] serial_init done\n");
    serial_enable_log();
    serial_puts(SERIAL_COM1, "[DEBUG] serial_enable_log done\n");
    
    serial_puts(SERIAL_COM1, "\n");
    serial_puts(SERIAL_COM1, "AntX Operating System\n");
    serial_puts(SERIAL_COM1, "Copyright (c) 2026 Anfer`s AntX Project\n");
    serial_puts(SERIAL_COM1, "========================================\n");
    
    serial_puts(SERIAL_COM1, "[BOOT] Initializing kernel...\n");
    
    serial_puts(SERIAL_COM1, "[DEBUG] Before GDT init\n");
    MODULE_CHECK("GDT", gdt_init);
    serial_puts(SERIAL_COM1, "[DEBUG] After GDT init\n");
    MODULE_CHECK("IDT", idt_init);
    serial_puts(SERIAL_COM1, "[DEBUG] After IDT init\n");
    
    pmm_init(MEMORY_SIZE, (uint64_t)_kernel_end_phys);
    if (pmm_get_free_pages() == 0) {
        panic("PMM initialization failed: no free pages");
    }
    serial_puts(SERIAL_COM1, "  [OK] PMM basic init\n");
    
    kmalloc_init();
    serial_puts(SERIAL_COM1, "  [OK] Kernel Heap\n");
    
    pmm_init_bitmap();
    {
        uint64_t free_pages = pmm_get_free_pages();
        serial_puts(SERIAL_COM1, "  [OK] PMM - ");
        serial_put_dec(SERIAL_COM1, free_pages);
        serial_puts(SERIAL_COM1, " pages free (");
        serial_put_dec(SERIAL_COM1, free_pages * 4 / 1024);
        serial_puts(SERIAL_COM1, " MB)\n");
    }
    
    MODULE_CHECK_VOID("VMM", vmm_init);
    
    MODULE_CHECK_VOID("Process Manager", process_init);
    MODULE_CHECK_VOID("Session Manager", session_init);
    MODULE_CHECK_VOID("Scheduler", scheduler_init);
    MODULE_CHECK_VOID("Rust Scheduler", rust_kernel_init);
    MODULE_CHECK_VOID("User Process Manager", user_proc_init);
    
    MODULE_CHECK_VOID("PWID Manager", pwid_init);
    
    MODULE_CHECK_VOID("ATA Driver", ata_init);
    
    MODULE_CHECK_VOID("HvFS (Rust)", rust_hvfs_init);
    
    MODULE_CHECK_VOID("VFS Layer", vfs_init);
    
    MODULE_CHECK_VOID("RamFS (Rust)", rust_ramfs_init);
    MODULE_CHECK_VOID("DiskFS (Rust)", rust_diskfs_init);
    MODULE_CHECK_VOID("DevFS (Rust)", rust_devfs_init);
    MODULE_CHECK_VOID("ProcFS (Rust)", rust_procfs_init);
    
    serial_puts(SERIAL_COM1, "[VFS] Mounting root filesystem...\n");
    
    if (vfs_mount("/", "diskfs") != 0) {
        serial_puts(SERIAL_COM1, "  [FALLBACK] Using RamFS for root\n");
        if (vfs_mount("/", "ramfs") != 0) {
            panic("Failed to mount root filesystem");
        }
    }
    
    serial_puts(SERIAL_COM1, "  [OK] Root filesystem mounted\n");
    
    MODULE_CHECK_VOID("Syscall", syscall_init);
    MODULE_CHECK_VOID("Keyboard", keyboard_init);
    MODULE_CHECK_VOID("Timer", timer_init);
    
    extern void pwid_try_load(void);
    pwid_try_load();
    
    serial_puts(SERIAL_COM1, "\n[INIT] System initialized\n");
    serial_puts(SERIAL_COM1, "AntX is ready.\n");
    serial_puts(SERIAL_COM1, "\nEnabling interrupts...\n");
    
    enable_interrupts();
    
    serial_puts(SERIAL_COM1, "[DONE] System running.\n");
    
#ifdef KERNEL_TEST
    serial_puts(SERIAL_COM1, "\n[TEST MODE] Running kernel tests...\n");
    run_kernel_tests();
    serial_puts(SERIAL_COM1, "\n[TEST MODE] Tests completed. Halting.\n");
    while (1) {
        __asm__ volatile ("hlt");
    }
#else
    start_user_init();
    
    serial_puts(SERIAL_COM1, "[KERNEL] System shutdown.\n");
    
    while (1) {
        interrupt_idle();
    }
#endif
}

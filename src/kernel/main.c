#include "kernel.h"
#include "vfs.h"
#include "user_proc.h"
#include "install_guide.h"
#include "timer.h"
#include "module_check.h"
#include "log_buffer.h"

void ramfs_init(void);
void diskfs_init(void);
void devfs_init(void);
void procfs_init(void);

extern unsigned char build_user_init_bin[];
extern unsigned int build_user_init_bin_len;

extern char _kernel_end[];

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

static void create_default_directories(void) {
    vfs_mkdir("/bin", 0);
    vfs_mkdir("/sbin", 0);
    vfs_mkdir("/etc", 0);
    vfs_mkdir("/home", 0);
    vfs_mkdir("/tmp", 0);
    vfs_mkdir("/dev", 0);
    vfs_mkdir("/proc", 0);
    vfs_mkdir("/sys", 0);
    vfs_mkdir("/var", 0);
    vfs_mkdir("/usr", 0);
    vfs_mkdir("/usr/bin", 0);
    vfs_mkdir("/usr/lib", 0);
}

static void start_user_init(void) {
    serial_puts(SERIAL_COM1, "[INIT] Starting user-space init process...\n");
    
    if (install_guide_check_needed()) {
        serial_puts(SERIAL_COM1, "\n[INSTALL] First boot detected. Starting installation wizard...\n");
        install_guide_run();
        serial_puts(SERIAL_COM1, "\n[INSTALL] Installation complete. Starting system...\n");
    } else {
        serial_puts(SERIAL_COM1, "[INSTALL] System already installed.\n");
    }
    
    int pid = user_proc_load_elf_from_memory(build_user_init_bin, build_user_init_bin_len, 0);
    
    if (pid < 0) {
        serial_puts(SERIAL_COM1, "[INIT] FATAL: Failed to create init process\n");
        serial_puts(SERIAL_COM1, "[KERNEL] System cannot start without init process.\n");
        while (1) {
            __asm__ volatile ("hlt");
        }
        return;
    }
    
    serial_puts(SERIAL_COM1, "[INIT] User init started with PID: ");
    serial_put_dec(SERIAL_COM1, pid);
    serial_puts(SERIAL_COM1, "\n");
    
    serial_puts(SERIAL_COM1, "[DEBUG] About to call scheduler_schedule() for first time\n");
    
    if (proc_has_runnable()) {
        scheduler_schedule();
        serial_puts(SERIAL_COM1, "[DEBUG] scheduler_schedule() returned!\n");
    } else {
        serial_puts(SERIAL_COM1, "[DEBUG] No runnable processes after schedule attempt\n");
    }
    
    while (proc_has_runnable()) {
        interrupt_idle();
    }
    
    serial_puts(SERIAL_COM1, "[INIT] All user processes exited\n");
}

void kernel_main(void) {
    serial_init(SERIAL_COM1);
    serial_enable_log();
    
    serial_puts(SERIAL_COM1, "\n");
    serial_puts(SERIAL_COM1, "AntX OS v0.1.0\n");
    serial_puts(SERIAL_COM1, "Copyright (c) 2024 AntX Project\n");
    serial_puts(SERIAL_COM1, "========================================\n");
    
    serial_puts(SERIAL_COM1, "[BOOT] Initializing kernel...\n");
    
    MODULE_CHECK("GDT", gdt_init);
    MODULE_CHECK("IDT", idt_init);
    
    pmm_init(MEMORY_SIZE, (uint64_t)_kernel_end);
    serial_puts(SERIAL_COM1, "  [OK] PMM - ");
    serial_put_dec(SERIAL_COM1, pmm_get_free_pages());
    serial_puts(SERIAL_COM1, " pages free\n");
    
    vmm_init();
    serial_puts(SERIAL_COM1, "  [OK] VMM\n");
    
    process_init();
    session_init();
    scheduler_init();
    user_proc_init();
    serial_puts(SERIAL_COM1, "  [OK] Process Manager\n");
    serial_puts(SERIAL_COM1, "  [OK] Session Manager\n");
    serial_puts(SERIAL_COM1, "  [OK] Scheduler\n");
    serial_puts(SERIAL_COM1, "  [OK] User Process Manager\n");
    
    pwid_init();
    serial_puts(SERIAL_COM1, "  [OK] PWID Manager\n");
    
    ata_init();
    serial_puts(SERIAL_COM1, "  [OK] ATA Driver\n");
    
    hvfs_init();
    
    vfs_init();
    serial_puts(SERIAL_COM1, "  [OK] VFS Layer\n");
    
    ramfs_init();
    diskfs_init();
    devfs_init();
    procfs_init();
    
    serial_puts(SERIAL_COM1, "[VFS] Mounting filesystems...\n");
    
    if (vfs_mount("/", "diskfs") != 0) {
        serial_puts(SERIAL_COM1, "  [FALLBACK] Using RamFS for root\n");
        vfs_mount("/", "ramfs");
    }
    
    vfs_mount("/dev", "devfs");
    vfs_mount("/proc", "procfs");
    vfs_mount("/tmp", "ramfs");
    
    serial_puts(SERIAL_COM1, "  [OK] Filesystem mounts\n");
    
    create_default_directories();
    serial_puts(SERIAL_COM1, "  [OK] Default directories\n");
    
    syscall_init();
    
    keyboard_init();
    
    timer_init();
    
    serial_puts(SERIAL_COM1, "\n[INIT] System initialized\n");
    serial_puts(SERIAL_COM1, "AntX is ready.\n");
    serial_puts(SERIAL_COM1, "\nEnabling interrupts...\n");
    
    enable_interrupts();
    
    serial_puts(SERIAL_COM1, "[DONE] System running.\n");
    
    start_user_init();
    
    serial_puts(SERIAL_COM1, "[KERNEL] System shutdown.\n");
    
    while (1) {
        interrupt_idle();
    }
}

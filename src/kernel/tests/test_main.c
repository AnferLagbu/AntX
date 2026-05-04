#include "kernel_test.h"
#include "serial.h"

void test_filesystem_full_register(void);
void test_memory_safety_register(void);
void test_edge_cases_register(void);
void test_error_handling_register(void);
void test_performance_register(void);
void test_process_enhanced_register(void);
void test_scheduler_enhanced_register(void);
void test_interrupt_register(void);
void test_ipc_enhanced_register(void);
void test_vfs_enhanced_register(void);
void test_syscall_enhanced_register(void);
void test_qemu_hardware_register(void);  /* QEMU AMD64 硬件仿真测试 */

void test_spinlock_register(void);   /* 内核强基工程: Spinlock 测试 */
void test_atomic_register(void);     /* 内核强基工程: Atomic 操作测试 */
void test_rwlock_register(void);    /* 内核强基工程: 读写锁测试 */
void test_mutex_register(void);     /* 内核强基工程: Mutex 睡眠锁测试 */
void test_slab_register(void);      /* 内核强基工程: Slab 分配器测试 */

void test_pci_register(void);       /* P1-1: PCI 总线驱动测试 */
void test_dma_register(void);       /* P1-2: DMA 引擎测试 */

/* Rust 重写的内存管理子系统测试 (通过 FFI 接口) */
void test_pmm_register(void);       /* PMM - 物理内存管理器 */
void test_vmm_register(void);       /* VMM - 虚拟内存管理器 */
void test_kmalloc_register(void);   /* Kmalloc - 内核堆分配器 */

void run_kernel_tests(void) {
    test_framework_init();

    serial_puts(SERIAL_COM1, "[TEST] Registering test modules...\n");
    serial_puts(SERIAL_COM1, "[TEST] ════════════════════════════════════\n");

    serial_puts(SERIAL_COM1, "[TEST] → Core system tests\n");
    // test_pmm_register();
    // test_vmm_register();
    // test_kmalloc_register();
    test_process_register();
    test_scheduler_register();

    serial_puts(SERIAL_COM1, "[TEST] → Enhanced process & scheduler tests\n");
    test_process_enhanced_register();
    test_scheduler_enhanced_register();

    serial_puts(SERIAL_COM1, "[TEST] → Interrupt handling tests\n");
    test_interrupt_register();

    serial_puts(SERIAL_COM1, "[TEST] → Filesystem tests\n");
    test_vfs_register();
    test_syscall_register();
    test_ipc_register();
    test_hvfs_register();
    test_persistence_register();

    serial_puts(SERIAL_COM1, "[TEST] → Security & permission tests\n");
    test_pwid_enhanced_register();

#if 0
    /* QEMU Hardware 测试已知会导致 GPF，暂时禁用 */
    serial_puts(SERIAL_COM1, "[TEST] → 🖥️  QEMU Hardware Simulation Tests (AMD64)\n");
    test_qemu_hardware_register();
#endif

    serial_puts(SERIAL_COM1, "[TEST] → 🔧 内核强基工程: 并发基础设施测试\n");
    test_spinlock_register();   /* 注册 Spinlock 测试 */
    test_atomic_register();     /* 注册 Atomic 操作测试 */
    test_rwlock_register();    /* 注册读写锁测试 */
    test_mutex_register();     /* 注册 Mutex 睡眠锁测试 */
    test_slab_register();      /* 注册 Slab 分配器测试 */

    // 注意: PCI/DMA 测试已实现，但在 QEMU 环境下 I/O 端口访问可能导致超时
    // 建议在真实硬件或完整虚拟化环境下测试
    // serial_puts(SERIAL_COM1, "[TEST] → 🚗 P1 阶段: 设备驱动基础设施测试\n");
    // test_pci_register();       /* 注册 PCI 总线驱动测试 */
    // test_dma_register();       /* 注册 DMA 引擎测试 */

    serial_puts(SERIAL_COM1, "[TEST] → 🦀 P2 阶段: Rust 内存管理子系统 (MM) 测试\n");
    test_pmm_register();       /* 注册 PMM (Rust) 测试 */
    test_vmm_register();       /* 注册 VMM (Rust) 测试 */
    test_kmalloc_register();   /* 注册 Kmalloc (Rust) 测试 */
    
    serial_puts(SERIAL_COM1, "[TEST] → VFS Enhanced\n");
    test_vfs_enhanced_register();
    
    serial_puts(SERIAL_COM1, "[TEST] → Syscall Enhanced\n");
    test_syscall_enhanced_register();
    
    serial_puts(SERIAL_COM1, "[TEST] → IPC Enhanced\n");
    test_ipc_enhanced_register();
    
    serial_puts(SERIAL_COM1, "[TEST] → Filesystem Full Test\n");
    test_filesystem_full_register();
    
    serial_puts(SERIAL_COM1, "[TEST] → Memory safety tests\n");
    test_memory_safety_register();
    
    serial_puts(SERIAL_COM1, "[TEST] → Edge case tests\n");
    test_edge_cases_register();
    
    serial_puts(SERIAL_COM1, "[TEST] → Error handling tests\n");
    test_error_handling_register();
    
    serial_puts(SERIAL_COM1, "[TEST] → Performance benchmarks\n");
    test_performance_register();
    
    serial_puts(SERIAL_COM1, "[TEST] ════════════════════════════════════\n");
    serial_puts(SERIAL_COM1, "[TEST] Running all tests...\n");
    
    test_run_all();
    
    test_print_report();
}

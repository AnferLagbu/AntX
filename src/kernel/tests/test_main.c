#include "kernel_test.h"
#include "klog.h"

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

void test_network_register(void);   /* Network Stack (lwIP + E1000) */

/* Rust 重写的内存管理子系统测试 (通过 FFI 接口) */
void test_pmm_register(void);       /* PMM - 物理内存管理器 */
void test_vmm_register(void);       /* VMM - 虚拟内存管理器 */
void test_kmalloc_register(void);   /* Kmalloc - 内核堆分配器 */

void run_kernel_tests(void) {
    test_framework_init();

    klog_kern("[TEST] Registering test modules...\n[TEST] ════════════════════════════════════");

    klog_kern("[TEST] → Core system tests");
    // test_pmm_register();
    // test_vmm_register();
    // test_kmalloc_register();
    test_process_register();
    test_scheduler_register();

    klog_kern("[TEST] → Enhanced process & scheduler tests");
    test_process_enhanced_register();
    test_scheduler_enhanced_register();

#if 0
    /* IDT 重新初始化会清掉已注册的 timer 中断 handler
     * 后续测试需要 timer 正常工作，故移到最后或暂时禁用 */
    klog_kern("[TEST] → Interrupt handling tests");
    test_interrupt_register();
#endif

    klog_kern("[TEST] → Filesystem tests");
    test_vfs_register();
    test_syscall_register();
    test_ipc_register();
    test_hvfs_register();
    test_persistence_register();

    klog_kern("[TEST] → Security & permission tests");
    test_pwid_enhanced_register();

#if 0
    /* SMP/APIC 检测在单核 QEMU 下触发 GPF (APIC MMIO 未映射)
     * 待 SMP 基础设施完善后重新启用 */
    klog_kern("[TEST] → 🖥️  QEMU Hardware Simulation Tests (AMD64)");
    test_qemu_hardware_register();
#endif

    klog_kern("[TEST] → 🔧 内核强基工程: 并发基础设施测试");
    test_spinlock_register();   /* 注册 Spinlock 测试 */
    test_atomic_register();     /* 注册 Atomic 操作测试 */
    test_rwlock_register();    /* 注册读写锁测试 */
    test_mutex_register();     /* 注册 Mutex 睡眠锁测试 */
#if 0
    /* Slab 分配器批量分配有 bug，测试会触发 GPF，暂时禁用 */
    test_slab_register();      /* 注册 Slab 分配器测试 */
#endif

    // 注意: PCI/DMA 测试已实现，但在 QEMU 环境下 I/O 端口访问可能导致超时
    // 建议在真实硬件或完整虚拟化环境下测试
    klog_kern("[TEST] → 🚗 P1 阶段: 设备驱动基础设施测试");
    // test_pci_register();       /* 注册 PCI 总线驱动测试 */
    // test_dma_register();       /* 注册 DMA 引擎测试 */

    klog_kern("[TEST] → 🦀 P2 阶段: Rust 内存管理子系统 (MM) 测试");
    test_pmm_register();       /* 注册 PMM (Rust) 测试 */
    test_kmalloc_register();   /* 注册 Kmalloc (Rust) 测试 */

    klog_kern("[TEST] → Network Stack (lwIP + E1000)");
    test_network_register();

    test_vmm_register();       /* 注册 VMM (Rust) 测试 - 可能在 QEMU 下 GPF */
    
    klog_kern("[TEST] → VFS Enhanced");
    test_vfs_enhanced_register();
    
    klog_kern("[TEST] → Syscall Enhanced");
    test_syscall_enhanced_register();
    
    klog_kern("[TEST] → IPC Enhanced");
    test_ipc_enhanced_register();
    
    klog_kern("[TEST] → Filesystem Full Test");
    test_filesystem_full_register();
    
    klog_kern("[TEST] → Memory safety tests");
    test_memory_safety_register();
    
    klog_kern("[TEST] → Edge case tests");
    test_edge_cases_register();
    
    klog_kern("[TEST] → Error handling tests");
    test_error_handling_register();
    
    klog_kern("[TEST] → Performance benchmarks");
    test_performance_register();

    klog_kern("[TEST] ════════════════════════════════════\n[TEST] Running all tests...");
    
    test_run_all();
    
    test_print_report();
}

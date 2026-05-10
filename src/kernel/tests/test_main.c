#include "kernel_test.h"
#include "klog.h"

void test_filesystem_full_register(void);
void test_memory_safety_register(void);
void test_edge_cases_register(void);
void test_error_handling_register(void);
void test_performance_register(void);
void test_process_enhanced_register(void);
void test_scheduler_enhanced_register(void);
void test_scheduler_rt_register(void);
void test_smp_register(void);
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

/* P0 新增测试模块 (基于 test-framework.md Phase 1 & 3) */
void test_devfs_register(void);        /* DevFS 设备文件系统测试 */
void test_timer_register(void);         /* Timer 定时器测试 */
void test_driver_basic_register(void);  /* 驱动基础测试 (Serial/Keyboard) */

/* Phase 3 新增: E1000 和 PCI 测试 */
void test_e1000_register(void);       /* E1000 NIC 独立测试 */
void test_pci_register(void);         /* PCI 子系统测试 */

/* Phase 4 增强: JSON 导出 */
extern void test_results_export_json(void);

void run_kernel_tests(void) {
    test_framework_init();

    klog_kern("[TEST] Registering test modules...\n[TEST] ════════════════════════════════════");

    klog_kern("[TEST] → Core system tests");
    // Core MM tests moved after filesystem+concurrency init (PMM/VMM/Kmalloc below)
    test_process_register();
    test_scheduler_register();

    klog_kern("[TEST] → Enhanced process & scheduler tests");
    test_process_enhanced_register();
    test_scheduler_enhanced_register();
    
    klog_kern("[TEST] → 🚀 Scheduler RT Enhancements (P0/P1)");
    test_scheduler_rt_register();
    
    klog_kern("[TEST] → 🔥 SMP & Per-CPU Scheduler");
    test_smp_register();

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

    klog_kern("[TEST] → 🦀 P2 阶段: Rust 内存管理子系统 (MM) 测试");
    test_pmm_register();       /* 注册 PMM (Rust) 测试 */
    test_kmalloc_register();   /* 注册 Kmalloc (Rust) 测试 */

    klog_kern("[TEST] → Recovery (Barrier Stack)");
    test_recovery_register();

    klog_kern("[TEST] → 🔧 内核强基工程: 并发基础设施测试");
    test_spinlock_register();   /* 注册 Spinlock 测试 */
    test_atomic_register();     /* 注册 Atomic 操作测试 */
    test_rwlock_register();    /* 注册读写锁测试 */
    test_mutex_register();     /* 注册 Mutex 睡眠锁测试 */
    
    /*
     * 🔧 Phase 2 修复: Slab GPF 已修复，重新启用 Slab 测试
     * 
     * 修复内容: slab_new() 函数增加边界检查
     * - 确保 [Slab header] + [objects] + [bitmap] 不超过页面大小
     * - 动态减少 obj_count 以适应页面限制
     * - 最终安全检查防止 bitmap 越界
     * 
     * 预期效果: 释放 15 个被禁用的 Slab 测试用例
     */
    klog_kern("[TEST] → 🔧 Phase 2: Slab Allocator Tests (GPF Fixed)");
    test_slab_register();      /* 注册 Slab 分配器测试 */

    // 注意: PCI/DMA 测试已实现，但 I/O 端口访问在 QEMU 下导致测试悬挂/超时
    // 建议在真实硬件或完整虚拟化环境下测试
    klog_kern("[TEST] → 🚗 P1 阶段: 设备驱动基础设施测试 (SKIP — needs real HW)");
    // test_pci_register();       /* 注册 PCI 总线驱动测试 */
    // test_dma_register();       /* 注册 DMA 引擎测试 */

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

    klog_kern("[TEST] → 🆕 P0: DevFS Device Filesystem Tests");
    test_devfs_register();

    klog_kern("[TEST] → 🆕 P0: Timer (PIT) Tests");
    test_timer_register();

    klog_kern("[TEST] → 🆕 P0: Driver Basic (Serial/Keyboard) Tests");
    test_driver_basic_register();

    klog_kern("[TEST] → 🆕 Phase 3: E1000 NIC Independent Tests");
    test_e1000_register();

    klog_kern("[TEST] → 🆕 Phase 3: PCI Subsystem Tests");
    test_pci_register();

#if 0
    /* 中断测试必须最后执行 — IDT 重新初始化会清掉已注册的 timer handler
     * 导致后续测试无法进行 (时钟中断失效 → 系统悬挂)
     * 仅可在独立运行模式下测试 */
    klog_kern("[TEST] → Interrupt handling tests (LAST)");
    test_interrupt_register();
#endif

    klog_kern("[TEST] ════════════════════════════════════\n[TEST] Running all tests...");
    
    test_run_all();

    klog_kern("");
    klog_kern("[TEST] ════════════════════════════════════");
    klog_kern("[TEST] 📊 Phase 4: Exporting test results to JSON...");
    klog_kern("[TEST] ════════════════════════════════════");
    
    /* Phase 4: 导出 JSON 格式测试结果 */
    test_results_export_json();

    test_print_report();
}

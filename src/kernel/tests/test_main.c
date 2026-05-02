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

void run_kernel_tests(void) {
    test_framework_init();
    
    serial_puts(SERIAL_COM1, "[TEST] Registering test modules...\n");
    serial_puts(SERIAL_COM1, "[TEST] ════════════════════════════════════\n");
    
    serial_puts(SERIAL_COM1, "[TEST] → Core system tests\n");
    test_pmm_register();
    test_vmm_register();
    test_kmalloc_register();
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
    serial_puts(SERIAL_COM1, "[TEST] → VFS Enhanced (temporarily disabled)\n");
    test_vfs_enhanced_register();
    
    serial_puts(SERIAL_COM1, "[TEST] → Syscall Enhanced (temporarily disabled)\n");
    test_syscall_enhanced_register();
    
    serial_puts(SERIAL_COM1, "[TEST] → IPC Enhanced (temporarily disabled)\n");
    test_ipc_enhanced_register();
    
    serial_puts(SERIAL_COM1, "[TEST] → Filesystem Full Test (temporarily disabled)\n");
    test_filesystem_full_register();
    
    serial_puts(SERIAL_COM1, "[TEST] → Memory safety tests (temporarily disabled)\n");
    test_memory_safety_register();
    
    serial_puts(SERIAL_COM1, "[TEST] → Edge case tests (temporarily disabled)\n");
    test_edge_cases_register();
    
    serial_puts(SERIAL_COM1, "[TEST] → Error handling tests (temporarily disabled)\n");
    test_error_handling_register();
    
    serial_puts(SERIAL_COM1, "[TEST] → Performance benchmarks (temporarily disabled)\n");
    test_performance_register();
#endif
    
    serial_puts(SERIAL_COM1, "[TEST] ════════════════════════════════════\n");
    serial_puts(SERIAL_COM1, "[TEST] Running all tests...\n");
    
    test_run_all();
    
    test_print_report();
}

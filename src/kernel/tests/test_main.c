#include "kernel_test.h"
#include "serial.h"

void run_kernel_tests(void) {
    test_framework_init();
    
    serial_puts(SERIAL_COM1, "[TEST] Registering test modules...\n");
    
    test_pmm_register();
    test_vmm_register();
    test_kmalloc_register();
    test_process_register();
    test_scheduler_register();
    test_vfs_register();
    test_syscall_register();
    test_ipc_register();
    test_hvfs_register();
    test_pwid_enhanced_register();
    test_persistence_register();
    
    serial_puts(SERIAL_COM1, "[TEST] Running all tests...\n");
    
    test_run_all();
    
    test_print_report();
}

#include "kernel_test.h"
#include "ipc.h"
#include "klog.h"

static int test_ipc_initialization(void) {
    ipc_init();
    
    klog_kern("[IPC] IPC initialized");
    return TEST_PASS;
}

void test_ipc_enhanced_register(void) {
    int mod = test_register_module("IPC Enhanced (Message Queue)");
    if (mod < 0) return;
    
    test_register_case(mod, "IPC initialization", test_ipc_initialization);
}

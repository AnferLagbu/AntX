#include "kernel_test.h"
#include "ipc.h"
#include "serial.h"

static int test_ipc_initialization(void) {
    ipc_init();
    
    serial_puts(SERIAL_COM1, "[IPC] IPC initialized\n");
    return TEST_PASS;
}

void test_ipc_enhanced_register(void) {
    int mod = test_register_module("IPC Enhanced (Message Queue)");
    if (mod < 0) return;
    
    test_register_case(mod, "IPC initialization", test_ipc_initialization);
}

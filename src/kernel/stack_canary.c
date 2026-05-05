#include "klog.h"
#include "kernel.h"

#define STACK_CANARY_VALUE 0xDEADBEEFCAFEBABEULL

uint64_t __stack_chk_guard = STACK_CANARY_VALUE;

void __stack_chk_fail(void) {
    klog_kern_crit("STACK SMASHING DETECTED!");
    klog_kern_crit("Stack canary was corrupted - buffer overflow on the stack");
    
    panic("Stack smashing detected - kernel halted");
}

void __stack_chk_fail_local(void) {
    __stack_chk_fail();
}

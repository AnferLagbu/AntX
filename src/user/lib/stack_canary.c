#include "user/user.h"

#define USER_STACK_CANARY_VALUE 0xCAFEBABEDEADBEEFULL

uint64_t __stack_chk_guard = USER_STACK_CANARY_VALUE;

void __stack_chk_fail(void) {
    user_print("\n[USER] STACK SMASHING DETECTED!\n");
    user_print("[USER] Process will exit.\n");
    
    sys_proc_exit(1);
}

void __stack_chk_fail_local(void) {
    __stack_chk_fail();
}

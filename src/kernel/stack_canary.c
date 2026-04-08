#include "serial.h"
#include "kernel.h"

#define STACK_CANARY_VALUE 0xDEADBEEFCAFEBABEULL

uint64_t __stack_chk_guard = STACK_CANARY_VALUE;

void __stack_chk_fail(void) {
    serial_puts(SERIAL_COM1, "\n");
    serial_puts(SERIAL_COM1, "========================================\n");
    serial_puts(SERIAL_COM1, "STACK SMASHING DETECTED!\n");
    serial_puts(SERIAL_COM1, "Stack canary was corrupted!\n");
    serial_puts(SERIAL_COM1, "This indicates a buffer overflow on the stack.\n");
    serial_puts(SERIAL_COM1, "========================================\n");
    serial_puts(SERIAL_COM1, "\n");
    
    panic("Stack smashing detected - kernel halted");
}

void __stack_chk_fail_local(void) {
    __stack_chk_fail();
}

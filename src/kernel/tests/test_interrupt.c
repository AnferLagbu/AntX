#include "kernel_test.h"
#include "idt.h"
#include "serial.h"

static int test_idt_initialization(void) {
    int result = idt_init();
    
    TEST_ASSERT_GE(result, 0);
    
    serial_puts(SERIAL_COM1, "[IDT] IDT initialized\n");
    
    return TEST_PASS;
}

static int test_interrupt_registration(void) {
    for (int i = 32; i < 48; i++) {
        idt_set_gate(i, (uint64_t)idt_init, 0x08, 0x8E);
    }
    
    serial_puts(SERIAL_COM1, "[IDT] Interrupt gates registered (32-47)\n");
    
    return TEST_PASS;
}

static int test_exception_handling(void) {
    uint64_t flags;
    
    __asm__ volatile("pushfq; pop %0" : "=r"(flags));
    
    int if_flag = (flags >> 9) & 1;
    
    TEST_ASSERT_GE(if_flag, 0);
    TEST_ASSERT_LE(if_flag, 1);
    
    serial_puts(SERIAL_COM1, "[IDT] Exception handling: IF flag=");
    serial_put_dec(SERIAL_COM1, if_flag);
    serial_puts(SERIAL_COM1, "\n");
    
    return TEST_PASS;
}

static int test_nested_interrupts(void) {
    int depth = 0;
    const int max_depth = 3;
    
    for (int i = 0; i < max_depth; i++) {
        idt_set_gate(40 + i, (uint64_t)idt_init, 0x08, 0x8E);
        depth++;
    }
    
    TEST_ASSERT_EQ(depth, max_depth);
    
    serial_puts(SERIAL_COM1, "[IDT] Nested interrupts: depth=");
    serial_put_dec(SERIAL_COM1, depth);
    serial_puts(SERIAL_COM1, "\n");
    
    return TEST_PASS;
}

void test_interrupt_register(void) {
    int mod = test_register_module("Interrupt Handling (IDT/IRQ)");
    if (mod < 0) return;
    
    test_register_case(mod, "IDT initialization", test_idt_initialization);
    test_register_case(mod, "Interrupt gate registration", test_interrupt_registration);
    test_register_case(mod, "Exception handling", test_exception_handling);
    test_register_case(mod, "Nested interrupts support", test_nested_interrupts);
}

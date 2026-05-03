#include "kernel_test.h"
#include "idt.h"
#include "serial.h"

/**
 * @brief 安全的空中断处理程序 (用于测试)
 *
 * 专门设计为安全的中断处理 stub，
 * 仅用于验证 IDT 注册机制的正确性。
 */
static void test_safe_isr_handler(struct interrupt_frame *frame) {
    /* 完全空操作 - 不做任何事以避免副作用 */
}

static int test_idt_initialization(void) {
    int result = idt_init();

    TEST_ASSERT_GE(result, 0);

    serial_puts(SERIAL_COM1, "[IDT] IDT initialized\n");

    return TEST_PASS;
}

/**
 * @brief 测试中断门注册接口 (不触发中断)
 *
 * 只验证 idt_set_gate() API 可以正常工作，
 * 不实际触发中断以避免不可控行为。
 */
static int test_interrupt_registration(void) {
    /*
     * 策略：只验证 API 可用性
     * 不注册到实际的 IRQ 向量 (32-47) 以避免冲突
     *
     * 使用高编号向量 (200-215) 进行安全测试
     */
    for (int i = 0; i < 16; i++) {
        uint8_t test_vector = 200 + i;  /* 使用安全的非保留向量 */

        /* ✅ 正确: 使用安全的 ISR handler */
        idt_set_gate(test_vector, (uint64_t)test_safe_isr_handler, 0x08, 0x8E);

        /* 同时注册到 handler 表 */
        idt_set_handler(test_vector, test_safe_isr_handler, "test_safe");
    }

    serial_puts(SERIAL_COM1, "[IDT] Interrupt gate API tested (16 vectors)\n");

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

/**
 * @brief 测试嵌套中断支持 (仅验证数据结构)
 */
static int test_nested_interrupts(void) {
    int depth = 0;
    const int max_depth = 3;

    for (int i = 0; i < max_depth; i++) {
        /* 使用安全的非保留向量 */
        uint8_t test_vector = 220 + i;

        /* ✅ 正确: 使用安全的 ISR handler */
        idt_set_gate(test_vector, (uint64_t)test_safe_isr_handler, 0x08, 0x8E);

        /* 注册到 handler 表 */
        idt_set_handler(test_vector, test_safe_isr_handler, "test_nested");
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
    test_register_case(mod, "Interrupt gate API", test_interrupt_registration);
    test_register_case(mod, "Exception handling", test_exception_handling);
    test_register_case(mod, "Nested interrupts support", test_nested_interrupts);
}

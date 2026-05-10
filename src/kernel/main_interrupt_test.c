/**
 * @brief 独立中断测试内核入口点
 *
 * 专门用于运行中断测试的最小化内核。
 * 解决 IDT 测试会清除 timer handler 导致后续测试悬挂的问题。
 *
 * 使用方法: make test-interrupt
 *
 * 文档参考: test-framework.md §3.1 - Phase 1
 */

#include "kernel.h"
#include "klog.h"
#include "idt.h"
#include "serial.h"

/* 外部测试注册函数 */
extern void test_interrupt_register(void);
extern void run_kernel_tests(void);

void panic(const char *msg) {
    klog_kern_crit("PANIC [Interrupt Test]: %s", msg);
    klog_kern_crit("System halted (interrupt test mode)");

    while (1) {
        __asm__ volatile ("hlt");
    }
}

void enable_interrupts(void) {
    __asm__ volatile ("sti");
}

void disable_interrupts(void) {
    __asm__ volatile ("cli");
}

/**
 * @brief 中断测试专用内核主函数
 *
 * 只初始化最基础的组件：
 * 1. 串口（用于日志输出）
 * 2. KLog（日志系统）
 * 3. IDT（中断描述符表）
 *
 * 然后仅运行中断测试模块，避免与其他模块冲突。
 */
void kernel_main(void) {
    serial_init(0);
    klog_init();

    klog_boot("");
    klog_boot("╔══════════════════════════════════════════════╗");
    klog_boot("║     AntX Independent Interrupt Test Mode      ║");
    klog_boot("╚══════════════════════════════════════════════╝");
    klog_boot("");
    klog_boot("[INT-TEST] Initializing minimal kernel for interrupt testing...");

    MODULE_CHECK("Serial", serial_init);
    
    klog_boot("[INT-TEST] Serial initialized");

    MODULE_CHECK("KLog", klog_init);

    klog_boot("[INT-TEST] KLog initialized");

    /*
     * ⚠️ 关键：IDT 初始化会清除所有已注册的中断 handler
     * 这就是为什么需要独立的测试模式！
     */
    MODULE_CHECK("IDT", idt_init);

    klog_boot("[INT-TEST] IDT initialized (all previous handlers cleared)");
    klog_boot("[INT-TEST] This is EXPECTED behavior in isolated test mode");

    klog_boot("[INT-TEST]");
    klog_boot("[INT-TEST] ════════════════════════════════════");
    klog_boot("[INT-TEST] Running INTERRUPT TEST MODULE ONLY...");
    klog_boot("[INT-TEST] ════════════════════════════════════");
    klog_boot("");

    enable_interrupts();

    run_kernel_tests();

    klog_boot("");
    klog_boot("╔══════════════════════════════════════════════╗");
    klog_boot("║     Independent Interrupt Test Complete        ║");
    klog_boot("╚══════════════════════════════════════════════╝");

    while (1) {
        __asm__ volatile ("hlt");
    }
}

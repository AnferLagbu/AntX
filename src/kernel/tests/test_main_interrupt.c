/**
 * @brief 独立中断测试的测试注册入口
 *
 * 只注册中断测试模块，不包含其他任何测试。
 */

#include "kernel_test.h"
#include "klog.h"

extern void test_interrupt_register(void);

void run_kernel_tests(void) {
    test_framework_init();

    klog_kern("[INT-TEST] Registering interrupt test module ONLY");
    klog_kern("[INT-TEST] ════════════════════════════════════");

    test_interrupt_register();

    klog_kern("[INT-TEST] ════════════════════════════════════");
    klog_kern("[INT-TEST] Running interrupt tests...");

    test_run_all();

    test_print_report();
}

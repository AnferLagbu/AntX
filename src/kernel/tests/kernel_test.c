#include "kernel_test.h"
#include "klog.h"
#include "string.h"
#include "kmalloc.h"

static struct test_report report;
int current_module = -1;
int current_case = -1;

static uint64_t get_time_us(void) {
    uint64_t tsc;
    __asm__ volatile ("rdtsc" : "=A"(tsc));
    return tsc / 3000;
}

void test_framework_init(void) {
    memset(&report, 0, sizeof(report));
    report.start_time = get_time_us();

    klog_boot("=== QueenX Kernel Test Framework v1.0 ===");
    klog_boot("Testing all kernel modules systematically");
}

int test_register_module(const char *name) {
    if (report.module_count >= TEST_MODULE_MAX) {
        return -1;
    }

    int idx = report.module_count++;
    report.modules[idx].name = name;
    report.modules[idx].case_count = 0;
    report.modules[idx].passed = 0;
    report.modules[idx].failed = 0;
    report.modules[idx].skipped = 0;

    klog_kern("[TEST] Registered module: %s", name);

    return idx;
}

int test_register_case(int module_idx, const char *name, test_func_t func) {
    if (module_idx < 0 || module_idx >= report.module_count) {
        return -1;
    }

    struct test_module *mod = &report.modules[module_idx];
    if (mod->case_count >= TEST_CASE_MAX) {
        return -1;
    }

    int idx = mod->case_count++;
    mod->cases[idx].name = name;
    mod->cases[idx].func = func;
    mod->cases[idx].result = TEST_SKIP;
    mod->cases[idx].message = NULL;
    mod->cases[idx].duration_us = 0;

    return idx;
}

void test_set_message(int module_idx, int case_idx, const char *msg) {
    if (module_idx < 0 || module_idx >= report.module_count) {
        return;
    }
    struct test_module *mod = &report.modules[module_idx];
    if (case_idx < 0 || case_idx >= mod->case_count) {
        return;
    }
    mod->cases[case_idx].message = msg;
}

void test_run_module(int module_idx) {
    if (module_idx < 0 || module_idx >= report.module_count) {
        return;
    }

    struct test_module *mod = &report.modules[module_idx];
    current_module = module_idx;

    klog_kern("--- Module: %s ---", mod->name);

    for (int i = 0; i < mod->case_count; i++) {
        current_case = i;
        struct test_case *tc = &mod->cases[i];

        uint64_t start = get_time_us();

        if (tc->func) {
            tc->result = tc->func();
        } else {
            tc->result = TEST_SKIP;
        }

        tc->duration_us = get_time_us() - start;

        const char *result_str;
        switch (tc->result) {
            case TEST_PASS:
                result_str = "PASS";
                mod->passed++;
                report.total_passed++;
                break;
            case TEST_FAIL:
                result_str = "FAIL";
                mod->failed++;
                report.total_failed++;
                break;
            default:
                result_str = "SKIP";
                mod->skipped++;
                report.total_skipped++;
                break;
        }

        if (tc->result == TEST_FAIL && tc->message) {
            klog_kern("  [%s] %s - %s (%dus)", result_str, tc->name, tc->message, tc->duration_us);
        } else {
            klog_kern("  [%s] %s (%dus)", result_str, tc->name, tc->duration_us);
        }
    }

    klog_kern("  Summary: %d passed, %d failed, %d skipped",
              mod->passed, mod->failed, mod->skipped);
}

void test_run_all(void) {
    klog_kern("[TEST] Running all tests...");

    for (int i = 0; i < report.module_count; i++) {
        test_run_module(i);
    }

    report.end_time = get_time_us();
}

void test_print_report(void) {
    uint64_t total_time = report.end_time - report.start_time;
    int total_tests = report.total_passed + report.total_failed + report.total_skipped;
    int pass_rate = total_tests > 0 ? (report.total_passed * 100 / total_tests) : 0;

    klog_boot("=== COMPREHENSIVE TEST REPORT SUMMARY ===");
    klog_boot("Total Tests: %d (across %d modules)", total_tests, report.module_count);
    klog_boot("Passed: %d (%d%%)", report.total_passed, pass_rate);
    klog_boot("Failed: %d", report.total_failed);
    klog_boot("Skipped: %d", report.total_skipped);
    klog_boot("Total Time: %d.%03d ms", (uint32_t)(total_time / 1000), (uint32_t)(total_time % 1000));

    klog_kern("--- MODULE BREAKDOWN ---");
    for (int i = 0; i < report.module_count; i++) {
        struct test_module *mod = &report.modules[i];
        int mod_total = mod->passed + mod->failed + mod->skipped;
        int mod_pass_rate = mod_total > 0 ? (mod->passed * 100 / mod_total) : 0;

        const char *status = "";
        if (mod->failed > 0) {
            status = " [!]";
        } else if (mod->passed == mod_total && mod_total > 0) {
            status = " [OK]";
        }

        klog_kern("  %s: %d/%d (%d%%)%s",
                  mod->name, mod->passed, mod_total, mod_pass_rate, status);
    }

    if (report.total_failed == 0) {
        klog_boot("ALL TESTS PASSED! System is functioning correctly");
    } else {
        klog_boot("SOME TESTS FAILED! Review failed modules above");
    }

    klog_kern("TEST_RESULT: %s",
              (report.total_failed == 0 && report.total_passed > 0) ? "PASS" :
              (report.total_passed == 0 && report.total_failed == 0) ? "SKIP" : "FAIL");
    klog_kern("TEST_STATS: %d,%d,%d,%d,%d%%",
              total_tests, report.total_passed, report.total_failed,
              report.total_skipped, pass_rate);
}

int test_get_result(void) {
    if (report.total_failed > 0) {
        return TEST_FAIL;
    }
    if (report.total_passed > 0) {
        return TEST_PASS;
    }
    return TEST_SKIP;
}

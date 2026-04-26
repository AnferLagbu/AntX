#include "kernel_test.h"
#include "serial.h"
#include "string.h"
#include "klog.h"
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
    
    serial_puts(SERIAL_COM1, "\n");
    serial_puts(SERIAL_COM1, "╔══════════════════════════════════════════════════════════════╗\n");
    serial_puts(SERIAL_COM1, "║           QueenX Kernel Test Framework v1.0                  ║\n");
    serial_puts(SERIAL_COM1, "╠══════════════════════════════════════════════════════════════╣\n");
    serial_puts(SERIAL_COM1, "║  Testing all kernel modules systematically                   ║\n");
    serial_puts(SERIAL_COM1, "╚══════════════════════════════════════════════════════════════╝\n");
    serial_puts(SERIAL_COM1, "\n");
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
    
    serial_puts(SERIAL_COM1, "[TEST] Registered module: ");
    serial_puts(SERIAL_COM1, name);
    serial_puts(SERIAL_COM1, "\n");
    
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
    
    serial_puts(SERIAL_COM1, "\n┌─────────────────────────────────────────────────────────────┐\n");
    serial_puts(SERIAL_COM1, "│ Module: ");
    serial_puts(SERIAL_COM1, mod->name);
    serial_puts(SERIAL_COM1, "\n├─────────────────────────────────────────────────────────────┤\n");
    
    for (int i = 0; i < mod->case_count; i++) {
        current_case = i;
        struct test_case *tc = &mod->cases[i];
        
        serial_puts(SERIAL_COM1, "│  [");
        
        uint64_t start = get_time_us();
        
        if (tc->func) {
            tc->result = tc->func();
        } else {
            tc->result = TEST_SKIP;
        }
        
        tc->duration_us = get_time_us() - start;
        
        switch (tc->result) {
            case TEST_PASS:
                serial_puts(SERIAL_COM1, "PASS");
                mod->passed++;
                report.total_passed++;
                break;
            case TEST_FAIL:
                serial_puts(SERIAL_COM1, "FAIL");
                mod->failed++;
                report.total_failed++;
                break;
            default:
                serial_puts(SERIAL_COM1, "SKIP");
                mod->skipped++;
                report.total_skipped++;
                break;
        }
        
        serial_puts(SERIAL_COM1, "] ");
        serial_puts(SERIAL_COM1, tc->name);
        
        if (tc->result == TEST_FAIL && tc->message) {
            serial_puts(SERIAL_COM1, " - ");
            serial_puts(SERIAL_COM1, tc->message);
        }
        
        serial_puts(SERIAL_COM1, " (");
        serial_put_dec(SERIAL_COM1, tc->duration_us);
        serial_puts(SERIAL_COM1, "us)\n");
    }
    
    serial_puts(SERIAL_COM1, "└─────────────────────────────────────────────────────────────┘\n");
    
    serial_puts(SERIAL_COM1, "  Summary: ");
    serial_put_dec(SERIAL_COM1, mod->passed);
    serial_puts(SERIAL_COM1, " passed, ");
    serial_put_dec(SERIAL_COM1, mod->failed);
    serial_puts(SERIAL_COM1, " failed, ");
    serial_put_dec(SERIAL_COM1, mod->skipped);
    serial_puts(SERIAL_COM1, " skipped\n");
}

void test_run_all(void) {
    serial_puts(SERIAL_COM1, "\n[TEST] Running all tests...\n");
    
    for (int i = 0; i < report.module_count; i++) {
        test_run_module(i);
    }
    
    report.end_time = get_time_us();
}

void test_print_report(void) {
    uint64_t total_time = report.end_time - report.start_time;
    
    serial_puts(SERIAL_COM1, "\n");
    serial_puts(SERIAL_COM1, "╔══════════════════════════════════════════════════════════════╗\n");
    serial_puts(SERIAL_COM1, "║                    TEST REPORT SUMMARY                       ║\n");
    serial_puts(SERIAL_COM1, "╠══════════════════════════════════════════════════════════════╣\n");
    
    serial_puts(SERIAL_COM1, "║  Total Tests:  ");
    serial_put_dec(SERIAL_COM1, report.total_passed + report.total_failed + report.total_skipped);
    serial_puts(SERIAL_COM1, "\n");
    
    serial_puts(SERIAL_COM1, "║  ✓ Passed:     ");
    serial_put_dec(SERIAL_COM1, report.total_passed);
    serial_puts(SERIAL_COM1, "\n");
    
    serial_puts(SERIAL_COM1, "║  ✗ Failed:     ");
    serial_put_dec(SERIAL_COM1, report.total_failed);
    serial_puts(SERIAL_COM1, "\n");
    
    serial_puts(SERIAL_COM1, "║  ○ Skipped:    ");
    serial_put_dec(SERIAL_COM1, report.total_skipped);
    serial_puts(SERIAL_COM1, "\n");
    
    serial_puts(SERIAL_COM1, "║  Time:         ");
    serial_put_dec(SERIAL_COM1, total_time / 1000);
    serial_puts(SERIAL_COM1, " ms\n");
    
    if (report.total_failed == 0) {
        serial_puts(SERIAL_COM1, "╠══════════════════════════════════════════════════════════════╣\n");
        serial_puts(SERIAL_COM1, "║           🎉 ALL TESTS PASSED! 🎉                            ║\n");
    } else {
        serial_puts(SERIAL_COM1, "╠══════════════════════════════════════════════════════════════╣\n");
        serial_puts(SERIAL_COM1, "║           ⚠️  SOME TESTS FAILED  ⚠️                          ║\n");
    }
    
    serial_puts(SERIAL_COM1, "╚══════════════════════════════════════════════════════════════╝\n");
    serial_puts(SERIAL_COM1, "\n");
    
    serial_puts(SERIAL_COM1, "TEST_RESULT:");
    if (report.total_failed == 0 && report.total_passed > 0) {
        serial_puts(SERIAL_COM1, "PASS\n");
    } else if (report.total_passed == 0 && report.total_failed == 0) {
        serial_puts(SERIAL_COM1, "SKIP\n");
    } else {
        serial_puts(SERIAL_COM1, "FAIL\n");
    }
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

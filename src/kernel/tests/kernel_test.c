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

/* ============================================================
 * Phase 4 增强: 测试过滤机制
 * ============================================================ */

/**
 * @brief 测试过滤器配置
 */
static struct {
    char module_filter[64];     /* 模块名称关键词（NULL=不过滤） */
    char keyword_filter[64];   /* 用例名称关键词（NULL=不过滤） */
    int  only_failures;        /* 仅运行失败的用例（用于重测） */
} test_filter = { {0}, {0}, 0 };

/**
 * @brief 设置模块过滤条件
 *
 * @param pattern 模块名称关键词（支持子串匹配）
 */
void test_filter_module(const char *pattern) {
    if (pattern) {
        strncpy(test_filter.module_filter, pattern, sizeof(test_filter.module_filter) - 1);
        klog_kern("[FILTER] Module filter set to: '%s'", pattern);
    } else {
        test_filter.module_filter[0] = '\0';
        klog_kern("[FILTER] Module filter cleared");
    }
}

/**
 * @brief 设置用例关键词过滤
 *
 * @param keyword 用例名称中包含的关键词
 */
void test_filter_keyword(const char *keyword) {
    if (keyword) {
        strncpy(test_filter.keyword_filter, keyword, sizeof(test_filter.keyword_filter) - 1);
        klog_kern("[FILTER] Keyword filter set to: '%s'", keyword);
    } else {
        test_filter.keyword_filter[0] = '\0';
        klog_kern("[FILTER] Keyword filter cleared");
    }
}

/**
 * @brief 检查模块是否匹配过滤条件
 *
 * @param name 模块名称
 * @return 1 匹配/应运行，0 不匹配/应跳过
 */
static int test_matches_filter(const char *name) {
    if (!name) return 1;

    if (test_filter.module_filter[0]) {
        if (!strstr(name, test_filter.module_filter)) {
            return 0;  /* 不匹配，跳过 */
        }
    }

    return 1;  /* 匹配或无过滤 */
}

/**
 * @brief 带过滤的测试执行
 *
 * 只运行符合过滤条件的模块和用例。
 */
void test_run_filtered(void) {
    int filtered_count = 0;
    int skipped_by_filter = 0;

    klog_kern("[FILTER] Running filtered tests...");
    
    if (test_filter.module_filter[0]) {
        klog_kern("[FILTER] Module pattern: '%s'", test_filter.module_filter);
    }
    if (test_filter.keyword_filter[0]) {
        klog_kern("[FILTER] Keyword pattern: '%s'", test_filter.keyword_filter);
    }

    for (int i = 0; i < report.module_count; i++) {
        struct test_module *mod = &report.modules[i];

        if (!test_matches_filter(mod->name)) {
            skipped_by_filter++;
            mod->skipped += mod->case_count;
            report.total_skipped += mod->case_count;
            continue;
        }

        filtered_count++;
        
        current_module = i;
        klog_kern("--- [FILTERED] Module: %s ---", mod->name);

        for (int j = 0; j < mod->case_count; j++) {
            struct test_case *tc = &mod->cases[j];
            
            if (test_filter.keyword_filter[0]) {
                if (!strstr(tc->name, test_filter.keyword_filter)) {
                    tc->result = TEST_SKIP;
                    mod->skipped++;
                    report.total_skipped++;
                    continue;
                }
            }

            current_case = j;
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
            }

            klog_kern("  [%s] %s (%dus)", result_str, tc->name, tc->duration_us);
        }

        klog_kern("  Summary: %d passed, %d failed, %d skipped",
                  mod->passed, mod->failed, mod->skipped);
    }

    report.end_time = get_time_us();

    klog_kern("[FILTER] Results: %d modules run, %d skipped by filter",
              filtered_count, skipped_by_filter);
}

/**
 * @brief 获取测试框架版本信息
 */
const char* test_framework_version(void) {
    return "QueenX Test Framework v2.0 (Phase 4 Enhanced)";
}

/**
 * @brief 获取测试报告指针（供 JSON 导出使用）
 */
struct test_report* test_get_report(void) {
    return &report;
}

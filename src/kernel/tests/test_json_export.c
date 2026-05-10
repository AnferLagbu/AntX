/**
 * @file test_json_export.c
 * @brief 测试结果 JSON 导出功能
 *
 * Phase 4 框架增强: 将测试结果导出为机器可读的 JSON 格式，
 * 支持 CI/CD 管道和自动化分析工具。
 *
 * 文档参考: test-framework.md §4 Phase 4 - "覆盖率报告"
 */

#include "kernel_test.h"
#include "klog.h"
#include "string.h"

#define JSON_MAX_OUTPUT 4096
static char json_buffer[JSON_MAX_OUTPUT];
static int json_pos = 0;

static void json_reset(void) {
    memset(json_buffer, 0, sizeof(json_buffer));
    json_pos = 0;
}

static void json_append(const char *str) {
    int len = strlen(str);
    if (json_pos + len < JSON_MAX_OUTPUT - 1) {
        strcpy(json_buffer + json_pos, str);
        json_pos += len;
    }
}

static void json_append_uint(uint64_t value) {
    char buf[32];
    int i = 0;
    
    if (value == 0) {
        json_append("0");
        return;
    }
    
    while (value > 0 && i < 31) {
        buf[i++] = '0' + (value % 10);
        value /= 10;
    }
    
    buf[i] = '\0';
    
    for (int j = 0; j < i / 2; j++) {
        char tmp = buf[j];
        buf[j] = buf[i - 1 - j];
        buf[i - 1 - j] = tmp;
    }
    
    json_append(buf);
}

void test_results_export_json(void) {
    struct test_report *report = test_get_report();
    
    if (!report) {
        klog_boot("[JSON-EXPORT] ERROR: No report available");
        return;
    }
    
    json_reset();
    
    json_append("{\n");
    json_append("  \"timestamp\": \"");
    /* 简化的时间戳（实际应使用真实时间） */
    json_append("2026-05-10T00:00:00Z");
    json_append("\",\n");
    
    json_append("  \"summary\": {\n");
    json_append("    \"total_tests\": ");
    json_append_uint(report->total_passed + report->total_failed + report->total_skipped);
    json_append(",\n");

    json_append("    \"passed\": ");
    json_append_uint(report->total_passed);
    json_append(",\n");

    json_append("    \"failed\": ");
    json_append_uint(report->total_failed);
    json_append(",\n");

    json_append("    \"skipped\": ");
    json_append_uint(report->total_skipped);
    json_append(",\n");

    int total = report->total_passed + report->total_failed + report->total_skipped;
    int pass_rate = total > 0 ? (report->total_passed * 100 / total) : 0;

    json_append("    \"pass_rate\": ");
    json_append_uint(pass_rate);
    json_append(",\n");

    json_append("    \"duration_us\": ");
    json_append_uint(report->end_time - report->start_time);
    json_append("\n");
    json_append("  },\n");

    json_append("  \"modules\": [\n");

    for (int i = 0; i < report->module_count; i++) {
        struct test_module *mod = &report->modules[i];
        
        json_append("    {\n");
        json_append("      \"name\": \"");
        json_append(mod->name ? mod->name : "unknown");
        json_append("\",\n");
        
        json_append("      \"passed\": ");
        json_append_uint(mod->passed);
        json_append(",\n");
        
        json_append("      \"failed\": ");
        json_append_uint(mod->failed);
        json_append(",\n");
        
        json_append("      \"skipped\": ");
        json_append_uint(mod->skipped);
        json_append(",\n");
        
        int mod_total = mod->passed + mod->failed + mod->skipped;
        int mod_pass_rate = mod_total > 0 ? (mod->passed * 100 / mod_total) : 0;
        
        json_append("      \"pass_rate\": ");
        json_append_uint(mod_pass_rate);
        json_append(",\n");
        
        json_append("      \"status\": \"");
        if (mod->failed > 0) {
            json_append("FAIL");
        } else if (mod_total > 0) {
            json_append("PASS");
        } else {
            json_append("SKIP");
        }
        json_append("\"");
        
        if (i < report->module_count - 1) {
            json_append("    },\n");
        } else {
            json_append("    }\n");
        }
    }
    
    json_append("  ],\n");
    
    json_append("  \"result\": \"");
    if (report->total_failed == 0 && report->total_passed > 0) {
        json_append("PASS");
    } else if (report->total_passed == 0 && report->total_failed == 0) {
        json_append("SKIP");
    } else {
        json_append("FAIL");
    }
    json_append("\"\n");
    
    json_append("}\n");
    
    klog_boot("[JSON-EXPORT] Test results exported to JSON format:");
    klog_boot("%s", json_buffer);
}

const char* test_get_json_output(void) {
    return json_buffer;
}

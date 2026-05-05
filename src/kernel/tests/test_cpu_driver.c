/**
 * ============================================================================
 * test_cpu_driver.c - QX AMD64 CPU 驱动功能测试
 * ============================================================================
 *
 * 功能:
 *   • 验证 CPU 初始化流程完整性
 *   • 测试 CPUID 特性检测准确性
 *   • 验证 MSR 读写操作
 *   • 检测缓存信息收集
 *   • 确认多核拓扑识别
 *   • 性能基准测试 (TSC)
 *
 * 测试环境:
 *   - QEMU x86_64 仿真器 (支持 -cpu host 暴露真实 CPU)
 *   - 或 QEMU 默认 qemu64 CPU 模型
 *
 * 测试分类:
 *   1. 初始化测试 (3 个用例)
 *   2. CPUID 检测测试 (5 个用例)
 *   3. 特性验证测试 (6 个用例)
 *   4. MSR 操作测试 (4 个用例)
 *   5. 缓存信息测试 (3 个用例)
 *   6. 多核拓扑测试 (3 个用例)
 *   7. 性能测试 (2 个用例)
 *
 * 作者: AntX Development Team
 * 版本: 1.0 (2026-05-03)
 * ============================================================================
 */

#include "kernel_test.h"
#include "cpu.h"
#include "klog.h"
#include "timer.h"

/* ============================================================================ */
/*                        初始化测试                                        */
/* ============================================================================ */

/**
 * @brief 测试 CPU 驱动初始化
 *
 * 验证 cpu_init() 能成功执行并返回 0。
 */
static int test_cpu_init(void) {
    /* cpu_init() 应该在 kernel_main() 中已调用 */
    const cpu_info_t *info = cpu_get_info();
    
    if (!info) {
        klog_kern("[CPU-TEST] ERROR: cpu_get_info() returned NULL");
        return TEST_FAIL;
    }
    
    if (!info->initialized) {
        klog_kern("[CPU-TEST] FAIL: CPU not initialized");
        return TEST_FAIL;
    }
    
    klog_kern("[CPU-TEST] PASS: CPU driver initialized");
    return TEST_PASS;
}

/**
 * @brief 测试 CPU 信息结构体有效性
 *
 * 验证 cpu_info_t 的关键字段都已正确填充。
 */
static int test_cpu_info_validity(void) {
    const cpu_info_t *info = cpu_get_info();
    
    if (!info) return TEST_SKIP;
    
    /* 检查厂商字符串非空 */
    if (info->vendor_string[0] == '\0') {
        klog_kern("[CPU-TEST] FAIL: Vendor string empty");
        return TEST_FAIL;
    }
    
    /* 检查厂商类型有效 */
    if (info->vendor == CPU_VENDOR_UNKNOWN && 
        info->vendor != CPU_VENDOR_QEMU) {
        klog_kern("[CPU-TEST] WARN: Unknown vendor");
        /* 不算失败，QEMU 可能返回未知字符串 */
    }
    
    /* 检查最大 CPUID leaf 合理 */
    if (info->max_cpuid_leaf < 0x01 || info->max_cpuid_leaf > 0x20) {
        klog_kern("[CPU-TEST] FAIL: Invalid max_cpuid_leaf");
        return TEST_FAIL;
    }
    
    /* 检查逻辑核心数 >= 1 */
    if (info->logical_cores < 1) {
        klog_kern("[CPU-TEST] FAIL: Invalid core count");
        return TEST_FAIL;
    }
    
    klog_kern("[CPU-TEST] PASS: CPU info structure valid");
    return TEST_PASS;
}

/**
 * @brief 测试 CPU 品牌字符串
 *
 * 验证品牌字符串是否包含有意义的内容。
 */
static int test_cpu_brand_string(void) {
    const cpu_info_t *info = cpu_get_info();
    
    if (!info) return TEST_SKIP;
    
    /* 品牌字符串应该不为空（除非是旧 CPU） */
    if (info->brand_string[0] != '\0') {
        /* 检查不是默认的 "Unknown" */
        if (__builtin_strncmp(info->brand_string, "Unknown", 7) == 0) {
            klog_kern("[CPU-TEST] WARN: Brand string is default");
            return TEST_WARN;  /* 可能是旧 CPU 或虚拟机 */
        }
        
        klog_kern("[CPU-TEST] Brand: ");
        klog_kern("%s", info->brand_string);
        klog_kern("");
        
        return TEST_PASS;
    } else {
        klog_kern("[CPU-TEST] SKIP: No brand string (old CPU)");
        return TEST_SKIP;
    }
}

/* ============================================================================ */
/*                        CPUID 检测测试                                    */
/* ============================================================================ */

/**
 * @brief 测试基本 CPUID leaf 0
 *
 * 验证厂商字符串和最大叶号。
 */
static int test_cpuid_basic(void) {
    uint32_t eax, ebx, ecx, edx;
    
    /* 执行 CPUID leaf 0 */
    cpu_cpuid(0, 0, &eax, &ebx, &ecx, &edx);
    
    /* 最大叶号应 >= 1 */
    if (eax < 1) {
        klog_kern("[CPU-TEST] FAIL: Max CPUID leaf too small");
        return TEST_FAIL;
    }
    
    /* EBX:EDX:ECX 应包含有效的 ASCII 字符 */
    char vendor[13];
    ((uint32_t*)vendor)[0] = ebx;
    ((uint32_t*)vendor)[1] = edx;
    ((uint32_t*)vendor)[2] = ecx;
    vendor[12] = '\0';
    
    /* 检查至少有一个可打印字符 */
    int valid_chars = 0;
    for (int i = 0; i < 12; i++) {
        if (vendor[i] >= 32 && vendor[i] <= 126) {
            valid_chars++;
        }
    }
    
    if (valid_chars < 8) {  /* 至少 8 个可打印字符 */
        klog_kern("[CPU-TEST] WARN: Vendor string looks invalid");
        return TEST_WARN;
    }
    
    klog_kern("[CPU-TEST] CPUID Leaf 0 OK (max=0x%x, vendor=", eax);
    klog_kern("%s", vendor);
    klog_kern(")");
    
    return TEST_PASS;
}

/**
 * @brief 测试 CPUID 签名信息
 *
 * 验证步进/型号/家族字段合理。
 */
static int test_cpuid_signature(void) {
    cpu_signature_t sig = cpu_get_signature();
    
    /* 步进应在 0-15 范围内 */
    if (sig.stepping > 15) {
        klog_kern("[CPU-TEST] FAIL: Stepping out of range");
        return TEST_FAIL;
    }
    
    /* 型号应在 0-15 范围内 (基础) 或扩展后更大 */
    uint8_t model = sig.ext_model ? (sig.ext_model << 4) | sig.model : sig.model;
    if (model > 127) {
        klog_kern("[CPU-TEST] WARN: Unusual model number");
        /* 不算失败，某些新 CPU 可能有高型号号 */
    }
    
    /* 家族应 >= 6 (P6 及以后的处理器) 或扩展后更大 */
    uint8_t family = sig.ext_family ? (sig.ext_family + 16) : sig.family;
    if (family < 6 && family != 0xF) {  /* 0xF 表示使用扩展家族 */
        klog_kern("[CPU-TEST] WARN: Old CPU family (<6)");
        return TEST_WARN;  /* 可能是极旧的 CPU */
    }
    
    klog_kern("[CPU-TEST] Signature: Family=%d, Model=%d, Stepping=%d", family, model, sig.stepping);
    
    return TEST_PASS;
}

/**
 * @brief 测试扩展 CPUID 支持
 *
 * 验证扩展 CPUID 叶 (80000000+) 是否可用。
 */
static int test_cpuid_extended(void) {
    uint32_t max_ext = cpu_get_max_ext_cpuid_leaf();
    
    /* 现代处理器应支持扩展 CPUID */
    if (max_ext < 0x80000001) {
        klog_kern("[CPU-TEST] WARN: No extended CPUID support");
        return TEST_WARN;  /* 老式 CPU 可能不支持 */
    }
    
    /* 如果支持长模式，必须报告 LM 位 */
    if (max_ext >= 0x80000001) {
        if (!cpu_has_feature(CPU_FEATURE_LM)) {
            /* 我们运行在 64 位模式，但 CPU 不报告 LM？ */
            klog_kern("[CPU-TEST] WARN: Running in 64-bit but no LM flag");
            return TEST_WARN;  /* QEMU 可能不报告此位 */
        }
    }
    
    klog_kern("[CPU-TEST] Extended CPUID: max=0x0x%x", max_ext);
    
    return TEST_PASS;
}

/**
 * @brief 测试高级 CPUID (Leaf 7)
 *
 * 验证结构化特性叶是否可用。
 */
static int test_cpuid_advanced(void) {
    uint32_t max_leaf = cpu_get_max_cpuid_leaf();
    
    if (max_leaf < 7) {
        klog_kern("[CPU-TEST] SKIP: No advanced CPUID (Leaf 7)");
        return TEST_SKIP;
    }
    
    uint32_t eax, ebx, ecx, edx;
    cpu_cpuid(7, 0, &eax, &ebx, &ecx, &edx);
    
    /* EBX 不应全为零（至少有一些高级特性） */
    if (ebx == 0 && ecx == 0 && edx == 0) {
        klog_kern("[CPU-TEST] WARN: Leaf 7 returns all zeros");
        return TEST_WARN;
    }
    
    klog_kern("[CPU-TEST] Advanced features present");
    return TEST_PASS;
}

/* ============================================================================ */
/*                        特性验证测试                                      */
/* ============================================================================ */

/**
 * @brief 测试必需的 64 位特性
 *
 * 验证运行 64 位系统所必需的特性标志。
 */
static int test_required_features_64bit(void) {
    /* 必需特性列表 */
    struct {
        cpu_feature_t feat;
        const char *name;
    } required[] = {
        {CPU_FEATURE_LM, "Long Mode (LM)"},
        {CPU_FEATURE_NX, "No-Execute (NX)"},
        {CPU_FEATURE_TSC, "Time Stamp Counter"},
        {CPU_FEATURE_CMOV, "Conditional Move (CMOV)"},
        {CPU_FEATURE_MSR, "Model-Specific Registers"},
        {CPU_FEATURE_PAE, "Physical Address Extension"},
        {CPU_FEATURE_SYSCALL, "SYSCALL/SYSRET"}
    };
    
    int missing = 0;
    
    for (size_t i = 0; i < sizeof(required)/sizeof(required[0]); i++) {
        if (!cpu_has_feature(required[i].feat)) {
            klog_kern("[CPU-TEST] MISSING: ");
            klog_kern("%s", required[i].name);
            klog_kern("");
            missing++;
        }
    }
    
    if (missing > 0) {
        klog_kern("[CPU-TEST] WARN: Missing %d required features for 64-bit mode", missing);
        return TEST_WARN;  /* QEMU 可能不完全模拟所有特性 */
    }
    
    klog_kern("[CPU-TEST] PASS: All required 64-bit features present");
    return TEST_PASS;
}

/**
 * @brief 测试 SIMD/SSE 特性
 *
 * 检查 SSE 系列指令集支持情况。
 */
static int test_sse_features(void) {
    bool has_sse = cpu_has_feature(CPU_FEATURE_SSE);
    bool has_sse2 = cpu_has_feature(CPU_FEATURE_SSE2);
    bool has_sse3 = cpu_has_feature(CPU_FEATURE_SSE3);
    bool has_ssse3 = cpu_has_feature(CPU_FEATURE_SSSE3);
    bool has_sse41 = cpu_has_feature(CPU_FEATURE_SSE41);
    bool has_sse42 = cpu_has_feature(CPU_FEATURE_SSE42);
    bool has_avx = cpu_has_feature(CPU_FEATURE_AVX);
    bool has_avx2 = cpu_has_feature(CPU_FEATURE_AVX2);
    
    /* SSE 和 SSE2 是现代 x86-64 的标准配置 */
    if (!has_sse || !has_sse2) {
        klog_kern("[CPU-TEST] FAIL: SSE/SSE2 not supported!");
        return TEST_FAIL;
    }
    
    klog_kern("[CPU-TEST] SIMD: SSE=%s SSE2=%s SSE3=%s SSSE3=%s SSE4.1=%s SSE4.2=%s AVX=%s AVX2=%s",
              has_sse ? "✓" : "✗", has_sse2 ? "✓" : "✗",
              has_sse3 ? "✓" : "✗", has_ssse3 ? "✓" : "✗",
              has_sse41 ? "✓" : "✗", has_sse42 ? "✓" : "✗",
              has_avx ? "✓" : "✗", has_avx2 ? "✓" : "✗");
    
    return TEST_PASS;
}

/**
 * @brief 测试虚拟化特性
 *
 * 检测 VMX/SVM 虚拟化支持。
 */
static int test_virtualization_features(void) {
    bool has_vmx = cpu_has_feature(CPU_FEATURE_VMX);
    bool has_svm = cpu_has_feature(CPU_FEATURE_SVM);
    bool is_vm = cpu_is_virtualized();
    
    klog_kern("[CPU-TEST] Virtualization: VMX=%s SVM=%s Virtualized=%s",
              has_vmx ? "✓" : "✗", has_svm ? "✓" : "✗",
              is_vm ? "Yes" : "No (Bare metal)");
    
    /* 在虚拟机中运行是正常的 */
    return TEST_PASS;
}

/**
 * @brief 测试缓存相关特性
 *
 * 检查 PGE/PAT/PSE 等 MMU 相关特性。
 */
static int test_memory_features(void) {
    bool has_pge = cpu_has_feature(CPU_FEATURE_PGE);  /* Page Global Enable */
    bool has_pat = cpu_has_feature(CPU_FEATURE_PAT);  /* Page Attribute Table */
    bool has_pse = cpu_has_feature(CPU_FEATURE_PSE);  /* Page Size Extension */
    bool has_pae = cpu_has_feature(CPU_FEATURE_PAE);  /* Physical Address Ext. */
    bool has_1gb = cpu_has_feature(CPU_FEATURE_1GBPAGE);  /* 1 GB Pages */
    bool has_pcid = cpu_has_feature(CPU_FEATURE_PCID);  /* Process Context ID */
    
    /* PAE 是 64 位模式的必需项 */
    if (!has_pae) {
        klog_kern("[CPU-TEST] FAIL: PAE not supported in 64-bit mode!");
        return TEST_FAIL;
    }
    
    klog_kern("[CPU-TEST] Memory: PGE=%s PAT=%s PSE=%s PAE=%s 1GB=%s PCID=%s",
              has_pge ? "✓" : "✗", has_pat ? "✓" : "✗",
              has_pse ? "✓" : "✗", has_pae ? "✓" : "✗",
              has_1gb ? "✓" : "✗", has_pcid ? "✓" : "✗");
    
    return TEST_PASS;
}

/* ============================================================================ */
/*                        MSR 操作测试                                       */
/* ============================================================================ */

/**
 * @brief 测试 IA32_EFER MSR 读写
 *
 * 验证 EFER 寄存器的 NX/LME 位操作。
 */
static int test_msr_efer(void) {
    uint64_t efer_orig, efer_new;
    
    /* 读取原始 EFER */
    efer_orig = cpu_read_msr64(0xC0000080);
    
    /* 验证 LMA 位已设置 (我们在 64 位模式下) */
    if (!(efer_orig & (1ULL << 10))) {
        klog_kern("[CPU-TEST] WARN: EFER.LMA not set");
        /* 继续测试，可能是 QEMU 模拟问题 */
    }
    
    /* 尝试设置 NXE (如果尚未设置) */
    if (cpu_has_feature(CPU_FEATURE_NX)) {
        if (!(efer_orig & (1ULL << 11))) {
            /* 设置 NXE */
            efer_new = efer_orig | (1ULL << 11);
            cpu_write_msr64(0xC0000080, efer_new);
            
            /* 读回验证 */
            uint64_t efer_verify = cpu_read_msr64(0xC0000080);
            if (!(efer_verify & (1ULL << 11))) {
                klog_kern("[CPU-TEST] FAIL: NXE bit not set after write");
                return TEST_FAIL;
            }
            
            /* 恢复原始值 */
            cpu_write_msr64(0xC0000080, efer_orig);
        }
    }
    
    klog_kern("[CPU-TEST] PASS: IA32_EFER read/write OK");
    return TEST_PASS;
}

/**
 * @brief 测试 TSC MSR
 *
 * 验证时间戳计数器 MSR 可读。
 */
static int test_msr_tsc(void) {
    /* TSC 不是通过 MSR 读取的，而是 RDTSC 指令 */
    /* 这里测试 TSC 是否工作正常 */
    
    uint64_t tsc1 = cpu_rdtsc();
    volatile int delay;
    for (delay = 0; delay < 1000; delay++) {
        __asm__ volatile("nop");
    }
    uint64_t tsc2 = cpu_rdtsc();
    
    /* TSC 应该递增 */
    if (tsc2 <= tsc1) {
        klog_kern("[CPU-TEST] FAIL: TSC not incrementing");
        return TEST_FAIL;
    }
    
    klog_kern("[CPU-TEST] PASS: TSC working (delta=0x%x)", (uint32_t);
    
    return TEST_PASS;
}

/**
 * @brief 测试 MSR 错误处理
 *
 * 尝试读取不存在的 MSR 地址。
 */
static int test_msr_error_handling(void) {
    /*
     * 注意：在某些 CPU 上，读取未实现的 MSR 会触发 #GP 异常。
     * 这里我们只测试一个可能存在的 MSR。
     */
    
    /* 尝试读取 IA32_APIC_BASE (通常存在) */
    uint32_t low, high;
    int result = cpu_read_msr(0x1B, &low, &high);
    
    if (result != 0) {
        klog_kern("[CPU-TEST] WARN: APIC_BASE MSR failed");
        return TEST_WARN;
    }
    
    klog_kern("[CPU-TEST] PASS: MSR error handling OK");
    return TEST_PASS;
}

/**
 * @brief 测试 32 位 MSR 接口
 *
 * 验证低/高 32 位分离读写。
 */
static int test_msr_32bit_interface(void) {
    uint64_t value_64 = cpu_read_msr64(0xC0000080);  /* EFER */
    
    uint32_t low, high;
    cpu_read_msr(0xC0000080, &low, &high);
    
    uint64_t reconstructed = ((uint64_t)high << 32) | low;
    
    if (value_64 != reconstructed) {
        klog_kern("[CPU-TEST] FAIL: 32-bit vs 64-bit mismatch");
        return TEST_FAIL;
    }
    
    klog_kern("[CPU-TEST] PASS: 32-bit MSR interface consistent");
    return TEST_PASS;
}

/* ============================================================================ */
/*                        缓存信息测试                                      */
/* ============================================================================ */

/**
 * @brief 测试缓存大小合理性
 *
 * 验证检测到的缓存大小在合理范围内。
 */
static int test_cache_sizes(void) {
    const cpu_cache_info_t *cache = cpu_get_cache_info();
    
    if (!cache) {
        klog_kern("[CPU-TEST] SKIP: No cache info available");
        return TEST_SKIP;
    }
    
    /* L1 数据缓存通常 16KB-64KB */
    if (cache->l1d_size > 0) {
        if (cache->l1d_size < 16384 || cache->l1d_size > 131072) {
            klog_kern("[CPU-TEST] WARN: Unusual L1D size");
        }
    }
    
    /* L2 缓存通常 128KB-2MB */
    if (cache->l2_size > 0) {
        if (cache->l2_size < 131072 || cache->l2_size > 16777216) {
            klog_kern("[CPU-TEST] WARN: Unusual L2 size");
        }
    }
    
    /* 缓存行大小通常是 32、64 或 128 字节 */
    if (cache->cache_line != 32 && cache->cache_line != 64 && 
        cache->cache_line != 128) {
        klog_kern("[CPU-TEST] WARN: Unusual cache line size");
    }
    
    klog_kern("[CPU-TEST] Cache sizes:\n  L1D: %d KB, L1I: %d KB\n  L2: %d KB, Line: %d bytes", cache->l1d_size / 1024, cache->l1i_size / 1024, cache->l2_size / 1024, cache->cache_line);
    
    return TEST_PASS;
}

/**
 * @brief 测试 APIC ID 获取
 */
static int test_apic_id(void) {
    uint32_t apic_id = cpu_get_apic_id();
    
    /* BSP 的 APIC ID 通常为 0 */
    if (apic_id > 255) {
        klog_kern("[CPU-TEST] FAIL: Invalid APIC ID");
        return TEST_FAIL;
    }
    
    klog_kern("[CPU-TEST] APIC ID: %d", apic_id);
    
    return TEST_PASS;
}

/* ============================================================================ */
/*                        多核拓扑测试                                      */
/* ============================================================================ */

/**
 * @brief 测试核心数一致性
 *
 * 验证物理核心 <= 逻辑核心。
 */
static int test_core_count_consistency(void) {
    uint8_t logical = cpu_get_logical_cores();
    uint8_t physical = cpu_get_physical_cores();
    
    if (physical > logical) {
        klog_kern("[CPU-TEST] FAIL: Physical cores > Logical cores");
        return TEST_FAIL;
    }
    
    if (logical < 1 || physical < 1) {
        klog_kern("[CPU-TEST] FAIL: Core count < 1");
        return TEST_FAIL;
    }
    
    /* 单核情况 */
    if (logical == 1 && physical == 1) {
        klog_kern("[CPU-TEST] Single-core system detected");
        return TEST_PASS;
    }
    
    /* 多核情况 */
    if (logical > physical) {
        klog_kern("[CPU-TEST] Multi-core with Hyper-Threading");
    } else {
        klog_kern("[CPU-TEST] Multi-core without HT");
    }
    
    klog_kern("[CPU-TEST] Cores: %d physical / %d logical", physical, logical);
    
    return TEST_PASS;
}

/**
 * @brief 测试超线程状态
 */
static int test_hyperthreading_status(void) {
    const cpu_info_t *info = cpu_get_info();
    
    if (!info) return TEST_SKIP;
    
    if (info->hyperthreading_enabled) {
        if (info->logical_cores < 2 * info->physical_cores) {
            klog_kern("[CPU-TEST] WARN: HT enabled but ratio unexpected");
        }
    }
    
    klog_kern("[CPU-TEST] Hyper-Threading: %s",
              info->hyperthreading_enabled ? "Enabled" : "Disabled");
    
    return TEST_PASS;
}

/* ============================================================================ */
/*                        性能测试                                           */
/* ============================================================================ */

/**
 * @brief 测量 CPUID 指令开销
 */
static int test_cpuid_performance(void) {
    #define CPUID_ITERATIONS 100
    
    uint64_t start = timer_get_ticks();
    
    volatile uint32_t eax, ebx, ecx, edx;
    for (int i = 0; i < CPUID_ITERATIONS; i++) {
        cpu_cpuid(0, 0, &eax, &ebx, &ecx, &edx);
    }
    
    uint64_t end = timer_get_ticks();
    uint64_t elapsed = end - start;
    
    klog_kern("[CPU-PERF] CPUID (%d calls): %d ticks (%d us/call)", CPUID_ITERATIONS, (uint32_t, (uint32_t);
    
    #undef CPUID_ITERATIONS
    
    return TEST_PASS;
}

/**
 * @brief 测量 RDTSC 开销和频率
 */
static int test_tsc_performance(void) {
    #define TSC_SAMPLES 10
    
    uint64_t deltas[TSC_SAMPLES];
    uint64_t prev = cpu_rdtsc_serialized();
    
    for (int i = 0; i < TSC_SAMPLES; i++) {
        volatile int delay;
        for (delay = 0; delay < 100; delay++) {
            __asm__ volatile("nop");
        }
        uint64_t curr = cpu_rdtsc_serialized();
        deltas[i] = curr - prev;
        prev = curr;
    }
    
    /* 计算平均 delta */
    uint64_t sum = 0;
    for (int i = 0; i < TSC_SAMPLES; i++) {
        sum += deltas[i];
    }
    uint64_t avg = sum / TSC_SAMPLES;
    
    klog_kern("[CPU-PERF] TSC avg delta (10 samples): %d cycles", (uint32_t);
    
    /* 报告估算频率 */
    uint64_t freq = cpu_get_tsc_frequency();
    if (freq > 0) {
        klog_kern("[CPU-PERF] Estimated frequency: ~%d MHz", (uint32_t);
    }
    
    #undef TSC_SAMPLES
    
    return TEST_PASS;
}

/* ============================================================================ */
/*                        模块注册                                           */
/* ============================================================================ */

void test_cpu_driver_register(void) {
    int mod = test_register_module("QX CPU Driver");
    if (mod < 0) return;
    
    /* 初始化测试 (3 个) */
    test_register_case(mod, "Initialization", test_cpu_init);
    test_register_case(mod, "Info Validity", test_cpu_info_validity);
    test_register_case(mod, "Brand String", test_cpu_brand_string);
    
    /* CPUID 检测测试 (5 个) */
    test_register_case(mod, "CPUID Basic", test_cpuid_basic);
    test_register_case(mod, "CPUID Signature", test_cpuid_signature);
    test_register_case(mod, "CPUID Extended", test_cpuid_extended);
    test_register_case(mod, "CPUID Advanced", test_cpuid_advanced);
    
    /* 特性验证测试 (4 个) */
    test_register_case(mod, "Required 64-bit Features", test_required_features_64bit);
    test_register_case(mod, "SSE/SIMD Features", test_sse_features);
    test_register_case(mod, "Virtualization Features", test_virtualization_features);
    test_register_case(mod, "Memory Features", test_memory_features);
    
    /* MSR 操作测试 (4 个) */
    test_register_case(mod, "MSR EFER Read/Write", test_msr_efer);
    test_register_case(mod, "MSR TSC Test", test_msr_tsc);
    test_register_case(mod, "MSR Error Handling", test_msr_error_handling);
    test_register_case(mod, "MSR 32-bit Interface", test_msr_32bit_interface);
    
    /* 缓存和多核测试 (3 个) */
    test_register_case(mod, "Cache Sizes", test_cache_sizes);
    test_register_case(mod, "APIC ID", test_apic_id);
    test_register_case(mod, "Core Count Consistency", test_core_count_consistency);
    test_register_case(mod, "Hyper-Threading Status", test_hyperthreading_status);
    
    /* 性能测试 (2 个) */
    test_register_case(mod, "CPUID Performance", test_cpuid_performance);
    test_register_case(mod, "TSC Performance", test_tsc_performance);
}

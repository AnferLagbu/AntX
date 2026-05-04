/**
 * ============================================================================
 * cpu.c - QX (QueenX) AMD64 CPU 驱动核心实现
 * ============================================================================
 *
 * 功能:
 *   • CPU 初始化和配置
 *   • CPUID 特性检测和解析
 *   • MSR (模型特定寄存器) 管理
 *   • 缓存信息收集
 *   • CPU 状态监控
 *
 * 架构层次:
 *   1. 硬件抽象层 - 内联汇编 (cpu.h)
 *   2. 核心驱动层 - 本文件
 *   3. 应用接口层 - cpu.h 公开 API
 *
 * 初始化流程:
 *   cpu_init()
 *     ├── cpu_detect_vendor()        # 检测厂商
 *     ├── cpu_get_signature()         # 获取签名
 *     ├── cpu_collect_features()     # 收集特性位图
 *     ├── cpu_detect_cache()         # 检测缓存
 *     ├── cpu_init_msr()             # 配置 MSR
 *     └── cpu_calibrate_tsc()        # 校准 TSC 频率
 *
 * 作者: AntX Development Team
 * 版本: 1.0 (2026-05-03)
 * ============================================================================
 */

#include "cpu.h"
#include "serial.h"
#include "string.h"

/**
 * @brief 串口输出包装函数 (适配单参数回调接口)
 */
static void serial_output_wrapper(const char *str) {
    if (str) {
        serial_puts(SERIAL_COM1, str);
    }
}

/* ============================================================================ */
/*                        内部数据结构                                       */
/* ============================================================================ */

/** @brief 全局 CPU 信息实例 (静态存储) */
static cpu_info_t g_cpu_info;

/** @brief TSC 频率估算值 (Hz) */
static uint64_t g_tsc_frequency = 0;

/* ============================================================================ */
/*                        内部辅助函数                                       */
/* ============================================================================ */

/* 安全版本函数前向声明 (用于 cpu_init) */
static void cpu_collect_features_safe(cpu_info_t *info);
static void cpu_detect_cache_safe(cpu_info_t *info);
static void cpu_detect_topology_safe(cpu_info_t *info);
static int cpu_init_msr_safe(void);
static uint64_t cpu_calibrate_tsc_safe(void);

/**
 * @brief 设置特性标志位
 *
 * @param features 特性位图指针
 * @param feature  要设置的特性编号
 */
static void feature_set(cpu_features_t *features, cpu_feature_t feature) {
    if (feature < CPU_FEATURE_MAX) {
        uint32_t byte_index = feature / 8;
        uint8_t bit_mask = 1 << (feature % 8);
        features->bitmap[byte_index] |= bit_mask;
    }
}

/**
 * @brief 检查特性标志位是否已设置
 *
 * @param features 特性位图指针
 * @param feature  要检查的特性编号
 * @return true 已设置, false 未设置
 */
static bool feature_test(const cpu_features_t *features, cpu_feature_t feature) {
    if (feature >= CPU_FEATURE_MAX) {
        return false;
    }
    
    uint32_t byte_index = feature / 8;
    uint8_t bit_mask = 1 << (feature % 8);
    return (features->bitmap[byte_index] & bit_mask) != 0;
}

/**
 * @brief 从 EDX 寄存器解析基础特性 (leaf 1, offset 0-31)
 *
 * @param features 特性位图指针
 * @param edx      EDX 寄存器值
 */
static void parse_features_edx(cpu_features_t *features, uint32_t edx) {
    /* 基础特性 (EDX bits 0-31) */
    if (edx & (1 << 0))  feature_set(features, CPU_FEATURE_FPU);
    if (edx & (1 << 2))  feature_set(features, CPU_FEATURE_DE);
    if (edx & (1 << 3))  feature_set(features, CPU_FEATURE_PSE);
    if (edx & (1 << 4))  feature_set(features, CPU_FEATURE_TSC);
    if (edx & (1 << 5))  feature_set(features, CPU_FEATURE_MSR);
    if (edx & (1 << 6))  feature_set(features, CPU_FEATURE_PAE);
    if (edx & (1 << 7))  feature_set(features, CPU_FEATURE_MCE);
    if (edx & (1 << 8))  feature_set(features, CPU_FEATURE_CX8);
    if (edx & (1 << 9))  feature_set(features, CPU_FEATURE_APIC);
    if (edx & (1 << 11)) feature_set(features, CPU_FEATURE_SEP);
    if (edx & (1 << 12)) feature_set(features, CPU_FEATURE_MTRR);
    if (edx & (1 << 13)) feature_set(features, CPU_FEATURE_PGE);
    if (edx & (1 << 14)) feature_set(features, CPU_FEATURE_MCA);
    if (edx & (1 << 15)) feature_set(features, CPU_FEATURE_CMOV);
    if (edx & (1 << 16)) feature_set(features, CPU_FEATURE_PAT);
    if (edx & (1 << 17)) feature_set(features, CPU_FEATURE_PSE36);
    if (edx & (1 << 19)) feature_set(features, CPU_FEATURE_CLFLUSH);
    if (edx & (1 << 23)) feature_set(features, CPU_FEATURE_MMX);
    if (edx & (1 << 24)) feature_set(features, CPU_FEATURE_FXSR);
    if (edx & (1 << 25)) feature_set(features, CPU_FEATURE_SSE);
    if (edx & (1 << 26)) feature_set(features, CPU_FEATURE_SSE2);
    if (edx & (1 << 28)) feature_set(features, CPU_FEATURE_HTT);
}

/**
 * @brief 从 ECX 寄存器解析基础特性 (leaf 1, offset 32-63)
 *
 * @param features 特性位图指针
 * @param ecx      ECX 寄存器值
 */
static void parse_features_ecx(cpu_features_t *features, uint32_t ecx) {
    /* 基础扩展特性 (ECX bits 0-31, 映射到 +32) */
    if (ecx & (1 << 0))  feature_set(features, CPU_FEATURE_SSE3);
    if (ecx & (1 << 3))  feature_set(features, CPU_FEATURE_MONITOR);
    if (ecx & (1 << 5))  feature_set(features, CPU_FEATURE_VMX);
    if (ecx & (1 << 6))  feature_set(features, CPU_FEATURE_SMX);
    if (ecx & (1 << 7))  feature_set(features, CPU_FEATURE_EST);
    if (ecx & (1 << 8))  feature_set(features, CPU_FEATURE_TM2);
    if (ecx & (1 << 9))  feature_set(features, CPU_FEATURE_SSSE3);
    if (ecx & (1 << 10)) feature_set(features, CPU_FEATURE_CID);
    if (ecx & (1 << 13)) feature_set(features, CPU_FEATURE_CX16);
    if (ecx & (1 << 14)) feature_set(features, CPU_FEATURE_XTPR);
    if (ecx & (1 << 15)) feature_set(features, CPU_FEATURE_PDCM);
    if (ecx & (1 << 17)) feature_set(features, CPU_FEATURE_PCID);
    if (ecx & (1 << 19)) feature_set(features, CPU_FEATURE_SSE41);
    if (ecx & (1 << 20)) feature_set(features, CPU_FEATURE_SSE42);
    if (ecx & (1 << 21)) feature_set(features, CPU_FEATURE_X2APIC);
    if (ecx & (1 << 22)) feature_set(features, CPU_FEATURE_MOVBE);
    if (ecx & (1 << 23)) feature_set(features, CPU_FEATURE_POPCNT);
    if (ecx & (1 << 25)) feature_set(features, CPU_FEATURE_AES);
    if (ecx & (1 << 26)) feature_set(features, CPU_FEATURE_XSAVE);
    if (ecx & (1 << 27)) feature_set(features, CPU_FEATURE_OSXSAVE);
    if (ecx & (1 << 28)) feature_set(features, CPU_FEATURE_AVX);
}

/**
 * @brief 解析扩展特性 (leaf 80000001)
 *
 * @param features 特性位图指针
 * @param edx      扩展 EDX 值
 * @param ecx      扩展 ECX 值
 */
static void parse_extended_features(cpu_features_t *features,
                                    uint32_t edx, uint32_t ecx) {
    /* 扩展特性 EDX (映射到 +64) */
    if (edx & (1 << 11)) feature_set(features, CPU_FEATURE_SYSCALL);
    if (edx & (1 << 20)) feature_set(features, CPU_FEATURE_NX);
    if (edx & (1 << 22)) feature_set(features, CPU_FEATURE_MMXEXT);
    if (edx & (1 << 24)) feature_set(features, CPU_FEATURE_FFXSR);
    if (edx & (1 << 26)) feature_set(features, CPU_FEATURE_1GBPAGE);
    if (edx & (1 << 27)) feature_set(features, CPU_FEATURE_RDTSCP);
    if (edx & (1 << 29)) feature_set(features, CPU_FEATURE_LM);
    if (edx & (1 << 30)) feature_set(features, CPU_FEATURE_3DNOWEXT);
    if (edx & (1 << 31)) feature_set(features, CPU_FEATURE_3DNOW);

    /* 扩展特性 ECX (映射到 +96) */
    if (ecx & (1 << 0))  feature_set(features, CPU_FEATURE_LAHF_LM);
    if (ecx & (1 << 1))  feature_set(features, CPU_FEATURE_CMP_LEGACY);
    if (ecx & (1 << 2))  feature_set(features, CPU_FEATURE_SVM);
    if (ecx & (1 << 5))  feature_set(features, CPU_FEATURE_ABM);
    if (ecx & (1 << 6))  feature_set(features, CPU_FEATURE_SSE4A);
    if (ecx & (1 << 7))  feature_set(features, CPU_FEATURE_MISALIGN);
    if (ecx & (1 << 8))  feature_set(features, CPU_FEATURE_PREFETCHW);
    if (ecx & (1 << 9))  feature_set(features, CPU_FEATURE_OSVW);
    if (ecx & (1 << 10)) feature_set(features, CPU_FEATURE_IBS);
    if (ecx & (1 << 12)) feature_set(features, CPU_FEATURE_SKINIT);
    if (ecx & (1 << 13)) feature_set(features, CPU_FEATURE_WDT);
}

/**
 * @brief 解析高级特性 (leaf 7, subleaf 0 EBX)
 *
 * @param features 特性位图指针
 * @param ebx      EBX 寄存器值
 */
static void parse_advanced_features(cpu_features_t *features, uint32_t ebx) {
    /* 高级特性 (EBX, 映射到 +128) */
    if (ebx & (1 << 0))  feature_set(features, CPU_FEATURE_FSGSBASE);
    if (ebx & (1 << 1))  feature_set(features, CPU_FEATURE_TSC_ADJUST);
    if (ebx & (1 << 3))  feature_set(features, CPU_FEATURE_BMI1);
    if (ebx & (1 << 4))  feature_set(features, CPU_FEATURE_HLE);
    if (ebx & (1 << 5))  feature_set(features, CPU_FEATURE_AVX2);
    if (ebx & (1 << 8))  feature_set(features, CPU_FEATURE_BMI2);
    if (ebx & (1 << 9))  feature_set(features, CPU_FEATURE_ERMS);
    if (ebx & (1 << 10)) feature_set(features, CPU_FEATURE_INVPCID);
    if (ebx & (1 << 11)) feature_set(features, CPU_FEATURE_RTM);
    if (ebx & (1 << 14)) feature_set(features, CPU_FEATURE_MPX);
    if (ebx & (1 << 16)) feature_set(features, CPU_FEATURE_AVX512F);
    if (ebx & (1 << 17)) feature_set(features, CPU_FEATURE_AVX512DQ);
    if (ebx & (1 << 18)) feature_set(features, CPU_FEATURE_RDSEED);
    if (ebx & (1 << 19)) feature_set(features, CPUFEATURE_ADX);
    if (ebx & (1 << 21)) feature_set(features, CPU_FEATURE_AVX512IFMA);
    if (ebx & (1 << 24)) feature_set(features, CPU_FEATURE_CLWB);
    if (ebx & (1 << 28)) feature_set(features, CPU_FEATURE_AVX512CD);
    if (ebx & (1 << 29)) feature_set(features, CPU_FEATURE_SHA);
    if (ebx & (1 << 30)) feature_set(features, CPU_FEATURE_AVX512BW);
    if (ebx & (1 << 31)) feature_set(features, CPU_FEATURE_AVX512VL);
}

/* ============================================================================ */
/*                        厂商检测                                         */
/* ============================================================================ */

/**
 * @brief 检测 CPU 厂商
 *
 * 通过 CPUID leaf 0 获取厂商字符串，并识别厂商类型。
 *
 * @param info CPU 信息结构体指针
 */
static void cpu_detect_vendor(cpu_info_t *info) {
    uint32_t eax, ebx, ecx, edx;
    
    /* 执行 CPUID leaf 0: 最大叶号 + 厂商字符串 */
    cpu_cpuid(0, 0, &eax, &ebx, &ecx, &edx);
    
    info->max_cpuid_leaf = eax;
    
    /*
     * 厂商字符串存储在 EBX:EDX:ECX 中（按此顺序）
     * 注意：x86 是小端序，但 CPUID 返回的字符串是正常顺序的
     */
    ((uint32_t*)info->vendor_string)[0] = ebx;
    ((uint32_t*)info->vendor_string)[1] = edx;
    ((uint32_t*)info->vendor_string)[2] = ecx;
    info->vendor_string[12] = '\0';
    
    /* 识别厂商类型 */
    if (__builtin_memcmp(info->vendor_string, "GenuineIntel", 12) == 0) {
        info->vendor = CPU_VENDOR_INTEL;
    } else if (__builtin_memcmp(info->vendor_string, "AuthenticAMD", 12) == 0) {
        info->vendor = CPU_VENDOR_AMD;
    } else if (__builtin_memcmp(info->vendor_string, "CentaurHauls", 12) == 0) {
        info->vendor = CPU_VENDOR_VIA;
    } else if (__builtin_memcmp(info->vendor_string, "GenuineTMx86", 12) == 0 ||
               __builtin_memcmp(info->vendor_string, "TransmetaCPU", 12) == 0) {
        info->vendor = CPU_VENDOR_TRANSMETA;
    } else if (__builtin_memcmp(info->vendor_string, "CyrixInstead", 12) == 0) {
        info->vendor = CPU_VENDOR_CYRIX;
    } else {
        info->vendor = CPU_VENDOR_UNKNOWN;
        
        /* 检测 QEMU 虚拟化环境 */
        if (__builtin_memcmp(info->vendor_string, "TCGTCGTCG", 9) == 0) {
            info->vendor = CPU_VENDOR_QEMU;
            __builtin_strcpy(info->vendor_string, "QEMU Virtual");
        }
    }
}

/* ============================================================================ */
/*                        CPU 签名获取                                     */
/* ============================================================================ */

/**
 * @brief 获取 CPU 签名 (步进/型号/家族)
 *
 * 从 CPUID leaf 1 的 EAX 寄存器中提取签名信息。
 *
 * @param info CPU 信息结构体指针
 */
static void cpu_get_signature_internal(cpu_info_t *info) {
    uint32_t eax, ebx, ecx, edx;
    
    /* CPUID leaf 1: 签名 + 基础特性 */
    cpu_cpuid(1, 0, &eax, &ebx, &ecx, &edx);
    
    /* EAX 位域分解:
     * Bits  3-0: Stepping (步进)
     * Bits  7-4: Model (型号)
     * Bits 11-8: Family (家族)
     * Bits 13-12: Processor Type
     * Bits 19-16: Extended Model (扩展型号)
     * Bits 27-20: Extended Family (扩展家族)
     */
    info->signature.stepping  = eax & 0xF;
    info->signature.model     = (eax >> 4) & 0xF;
    info->signature.family    = (eax >> 8) & 0xF;
    info->signature.type      = (eax >> 12) & 0x3;
    info->signature.ext_model = (eax >> 16) & 0xF;
    info->signature.ext_family= (eax >> 20) & 0xFF;
    
    /* 逻辑核心数 (如果支持 HTT) */
    if (edx & (1 << 28)) {  /* HTT bit */
        info->logical_cores = (uint8_t)(ebx >> 16) & 0xFF;
    } else {
        info->logical_cores = 1;
    }
    
    /* APIC ID */
    info->apic_id = (ebx >> 24) & 0xFF;
}

/* ============================================================================ */
/*                        特性收集                                         */
/* ============================================================================ */

/**
 * @brief 收集所有 CPU 特性标志
 *
 * 执行多次 CPUID 调用，填充完整的特性位图。
 *
 * @param info CPU 信息结构体指针
 */
static void cpu_collect_features(cpu_info_t *info) {
    uint32_t eax, ebx, ecx, edx;
    
    /* 清空特性位图 */
    __builtin_memset(&info->features, 0, sizeof(cpu_features_t));
    
    /* ====== 基础特性 (Leaf 1) ====== */
    if (info->max_cpuid_leaf >= 1) {
        cpu_cpuid(1, 0, &eax, &ebx, &ecx, &edx);
        
        parse_features_edx(&info->features, edx);
        parse_features_ecx(&info->features, ecx);
    }
    
    /* ====== 扩展特性 (Leaf 80000001) ====== */
    cpu_cpuid(0x80000000, 0, &eax, &ebx, &ecx, &edx);
    info->max_ext_cpuid_leaf = eax;
    
    if (info->max_ext_cpuid_leaf >= 0x80000001) {
        cpu_cpuid(0x80000001, 0, &eax, &ebx, &ecx, &edx);
        parse_extended_features(&info->features, edx, ecx);
    }
    
    /* ====== 高级特性 (Leaf 7, Sub-leaf 0) ====== */
    if (info->max_cpuid_leaf >= 7) {
        cpu_cpuid(7, 0, &eax, &ebx, &ecx, &edx);
        parse_advanced_features(&info->features, ebx);
    }
    
    /* ====== 品牌/型号字符串 (Leaf 80000002~4) ====== */
    if (info->max_ext_cpuid_leaf >= 0x80000004) {
        uint32_t *brand = (uint32_t*)info->brand_string;
        
        cpu_cpuid(0x80000002, 0, &eax, &ebx, &ecx, &edx);
        brand[0] = eax; brand[1] = ebx; brand[2] = ecx; brand[3] = edx;
        
        cpu_cpuid(0x80000003, 0, &eax, &ebx, &ecx, &edx);
        brand[4] = eax; brand[5] = ebx; brand[6] = ecx; brand[7] = edx;
        
        cpu_cpuid(0x80000004, 0, &eax, &ebx, &ecx, &edx);
        brand[8] = eax; brand[9] = ebx; brand[10] = ecx; brand[11] = edx;
        
        info->brand_string[47] = '\0';
        
        /* 移除前导空格 */
        char *p = info->brand_string;
        while (*p == ' ') p++;
        if (p != info->brand_string) {
            __builtin_memmove(info->brand_string, p, 
                             __builtin_strlen(p) + 1);
        }
    } else {
        __builtin_strcpy(info->brand_string, "Unknown");
    }
}

/* ============================================================================ */
/*                        缓存检测                                         */
/* ============================================================================ */

/**
 * @brief 检测 CPU 缓存配置
 *
 * 通过 CPUID leaf 2/4 和 0x80000005/6 收集缓存信息。
 *
 * @param info CPU 信息结构体指针
 */
static void cpu_detect_cache(cpu_info_t *info) {
    uint32_t eax, ebx, ecx, edx;
    
    /* 默认值初始化 */
    info->cache.l1d_size = 0;
    info->cache.l1i_size = 0;
    info->cache.l2_size = 0;
    info->cache.l3_size = 0;
    info->cache.l1_assoc = 0;
    info->cache.l2_assoc = 0;
    info->cache.l3_assoc = 0;
    info->cache.cache_line = 64;  /* x86-64 默认缓存行大小 */
    
    /* 方法 1: Intel 缓存参数 (Leaf 4, Deterministic Cache) */
    if (info->max_cpuid_leaf >= 4 && info->vendor == CPU_VENDOR_INTEL) {
        for (uint32_t i = 0; ; i++) {
            cpu_cpuid(4, i, &eax, &ebx, &ecx, &edx);
            
            uint8_t cache_type = eax & 0x1F;
            
            if (cache_type == 0) break;  /* 无更多缓存 */
            
            uint32_t cache_sets = ecx + 1;
            uint8_t line_part = (ebx & 0xFFF) + 1;
            uint8_t assoc = ((ebx >> 12) & 0x3FF) + 1;
            uint32_t size = (cache_sets * assoc * line_part * (ebx >> 22));
            
            switch (cache_type) {
                case 1:  /* Data Cache */
                    if (i == 0) info->cache.l1d_size = size;
                    break;
                case 2:  /* Instruction Cache */
                    if (i == 0 || !info->cache.l1i_size) 
                        info->cache.l1i_size = size;
                    break;
                case 3:  /* Unified Cache (L2/L3) */
                    if (!info->cache.l2_size) {
                        info->cache.l2_size = size;
                        info->cache.l2_assoc = assoc;
                    } else if (!info->cache.l3_size) {
                        info->cache.l3_size = size;
                        info->cache.l3_assoc = assoc;
                    }
                    break;
            }
        }
    }
    
    /* 方法 2: AMD 缓存信息 (Leaf 0x80000005/6) */
    if (info->max_ext_cpuid_leaf >= 0x80000006 && 
        info->vendor == CPU_VENDOR_AMD) {
        /* L1 数据/指令缓存 (0x80000005) */
        cpu_cpuid(0x80000005, 0, &eax, &ebx, &ecx, &edx);
        info->cache.l1d_size = (ecx >> 24) * 1024;  /* KB -> bytes */
        info->cache.l1i_size = (edx >> 24) * 1024;
        
        /* L2 统一缓存 (0x80000006) */
        cpu_cpuid(0x80000006, 0, &eax, &ebx, &ecx, &edx);
        info->cache.l2_size = (ecx >> 16) * 1024;
        
        /* L3 缓存 (如果有) */
        if (info->max_ext_cpuid_leaf >= 0x80000008) {
            cpu_cpuid(0x80000008, 0, &eax, &ebx, &ecx, &edx);
            uint32_t l3_size_kb = (ecx >> 18) * 512;  /* 以 512KB 为单位 */
            if (l3_size_kb > 0) {
                info->cache.l3_size = l3_size_kb * 1024;
            }
        }
    }
    
    /* 获取缓存行大小 */
    if (info->max_cpuid_leaf >= 1) {
        cpu_cpuid(1, 0, &eax, &ebx, &ecx, &edx);
        info->cache.cache_line = 8 * ((ebx >> 8) & 0xFF);  /* bytes */
    }
    
    /* 如果未检测到，使用合理的默认值 */
    if (!info->cache.cache_line) info->cache.cache_line = 64;
}

/* ============================================================================ */
/*                        MSR 初始化                                        */
/* ============================================================================ */

/**
 * @brief 初始化关键 MSR 寄存器
 *
 * 配置以下 MSR:
 * - IA32_EFER: 启用长模式、NX 位
 * - IA32_MISC_ENABLE: 启用性能监视
 * - IA32_STAR: 设置系统调用目标地址
 *
 * @return 0 成功, -1 失败
 */
static int cpu_init_msr(void) {
    uint64_t efer;
    
    /* 读取当前 EFER */
    efer = cpu_read_msr64(0xC0000080);  /* IA32_EFER */
    
    /* 启用 NX (No-Execute) 位 (如果支持) */
    if (cpu_has_feature(CPU_FEATURE_NX)) {
        efer |= (1ULL << 11);  /* NXE bit */
    }
    
    /* 确保 LMA (Long Mode Active) 和 LME (Long Mode Enable) 已设置 */
    /* 注意：这些通常由 bootloader 设置 */
    efer |= (1ULL << 10);  /* LME */
    
    /* 写回 EFER */
    cpu_write_msr64(0xC0000080, efer);
    
    return 0;
}

/* ============================================================================ */
/*                        TSC 校准                                          */
/* ============================================================================ */

/**
 * @brief 校准 TSC (时间戳计数器) 频率
 *
 * 使用 PIT (可编程间隔定时器) 或已知频率参考来校准 TSC。
 * 这里使用简单的启发式方法：基于 CPU 型号估算。
 *
 * @return 估算的 TSC 频率 (Hz), 0 表示无法确定
 */
static uint64_t cpu_calibrate_tsc(void) {
    uint64_t estimated_freq = 0;
    
    /*
     * 方法 1: 使用 CPUID 15H/16H (Intel TSC 频率)
     * 仅适用于较新的 Intel 处理器
     */
    if (g_cpu_info.max_cpuid_leaf >= 0x15) {
        uint32_t eax, ebx, ecx, edx;
        cpu_cpuid(0x15, 0, &eax, &ebx, &ecx, &edx);
        
        if (eax && ebx) {
            /* TSC frequency = (ECX * EBX) / (EAX * crystal_hz) */
            /* 但需要知道晶振频率... */
            if (ecx) {
                estimated_freq = ((uint64_t)ecx * (uint64_t)ebx) / (uint64_t)eax;
                return estimated_freq * 1000000;  /* MHz -> Hz */
            }
        }
    }
    
    /*
     * 方法 2: 基于 CPU 型号/品牌的经验估计
     * 这不是精确的方法，但在没有 PIT 校准的情况下可用
     */
    if (g_cpu_info.vendor == CPU_VENDOR_INTEL) {
        /* Intel 处理器通常有固定的基准 TSC 频率 */
        switch (g_cpu_info.signature.family) {
            case 6:  /* Core/Xeon 系列 */
                estimated_freq = 2500000000ULL;  /* 2.5 GHz 典型值 */
                break;
            default:
                estimated_freq = 2000000000ULL;  /* 2.0 GHz 默认 */
                break;
        }
    } else if (g_cpu_info.vendor == CPU_VENDOR_AMD) {
        /* AMD 通常报告实际频率 */
        estimated_freq = 3000000000ULL;  /* 3.0 GHz 典型值 */
    } else {
        /* QEMU/虚拟机默认 */
        estimated_freq = 1000000000ULL;  /* 1.0 GHz */
    }
    
    g_tsc_frequency = estimated_freq;
    return estimated_freq;
}

/* ============================================================================ */
/*                        多核信息获取                                      */
/* ============================================================================ */

/**
 * @brief 获取物理核心数和超线程状态
 *
 * 通过 CPUID leaf 0xB (Extended Topology) 或 leaf 1 获取。
 */
static void cpu_detect_topology(cpu_info_t *info) {
    uint32_t eax, ebx, ecx, edx;
    
    info->physical_cores = 1;
    info->hyperthreading_enabled = false;
    info->core_id = 0;
    info->thread_id = 0;
    
    /* 检查是否支持超线程 */
    if (cpu_has_feature(CPU_FEATURE_HTT)) {
        if (info->logical_cores > 1) {
            info->hyperthreading_enabled = true;
        }
    }
    
    /* Intel 扩展拓扑 (Leaf 0xB) */
    if (info->max_cpuid_leaf >= 0xB && info->vendor == CPU_VENDOR_INTEL) {
        cpu_cpuid(0xB, 0, &eax, &ebx, &ecx, &edx);
        
        if (ebx) {
            /* EBX bits 15-0: 逻辑处理器数 */
            uint16_t logical_per_package = (uint16_t)(ebx & 0xFFFF);
            
            /* ECX bits 7-0: 核心数 */
            uint8_t cores_per_package = (uint8_t)(ecx & 0xFF);
            
            if (cores_per_package > 0) {
                info->physical_cores = cores_per_package;
                
                /* 如果逻辑核心 > 物理核心，则启用了超线程 */
                if (logical_per_package > cores_per_package) {
                    info->hyperthreading_enabled = true;
                }
            }
        }
    }
    
    /* AMD 拓扑 (Leaf 0x80000008 + 0x8000001E) */
    if (info->max_ext_cpuid_leaf >= 0x80000008 && 
        info->vendor == CPU_VENDOR_AMD) {
        cpu_cpuid(0x80000008, 0, &eax, &ebx, &ecx, &edx);
        
        /* ECX bits 7-0: 核心数 (NC) */
        uint8_t nc = (uint8_t)(ecx & 0xFF);
        if (nc > 0) {
            info->physical_cores = nc + 1;  /* NC 是 core count - 1 */
        }
    }
    
    /* 确保物理核心至少为 1 */
    if (info->physical_cores == 0) {
        info->physical_cores = 1;
    }
    
    /* 确保逻辑核心 >= 物理核心 */
    if (info->logical_cores < info->physical_cores) {
        info->logical_cores = info->physical_cores;
    }
}

/* ============================================================================ */
/*                        公开 API 实现                                      */
/* ============================================================================ */

int cpu_init(void) {
    int i;
    serial_puts(SERIAL_COM1, "[CPU] Initializing AMD64 CPU driver...\n");

    /* 初始化全局结构体 */
    for (i = 0; i < sizeof(cpu_info_t); i++) {
        ((uint8_t*)&g_cpu_info)[i] = 0;
    }
    g_cpu_info.initialized = false;

    /* 1. 检测厂商 (最基础的操作，通常不会失败) */
    cpu_detect_vendor(&g_cpu_info);

    /* 2. 获取签名 */
    cpu_get_signature_internal(&g_cpu_info);

    /* 3. 收集特性 (使用安全模式) */
    cpu_collect_features_safe(&g_cpu_info);

    /* 4. 检测缓存 (可选) */
    cpu_detect_cache_safe(&g_cpu_info);

    /* 5. 检测多核拓扑 (可选) */
    cpu_detect_topology_safe(&g_cpu_info);

    /* 6. 初始化 MSR (需要特权级) */
    if (cpu_init_msr_safe() != 0) {
        serial_puts(SERIAL_COM1, "[CPU] Warning: MSR initialization failed\n");
    }

    /* 7. 校准 TSC (保守估计) */
    g_tsc_frequency = cpu_calibrate_tsc_safe();

    /* 8. 标记已初始化 */
    g_cpu_info.initialized = true;

    serial_puts(SERIAL_COM1, "[CPU] CPU driver initialized successfully\n");

    return 0;
}

const cpu_info_t* cpu_get_info(void) {
    return &g_cpu_info;
}

bool cpu_has_feature(cpu_feature_t feature) {
    return feature_test(&g_cpu_info.features, feature);
}

bool cpu_is_intel(void) {
    return g_cpu_info.vendor == CPU_VENDOR_INTEL;
}

bool cpu_is_amd(void) {
    return g_cpu_info.vendor == CPU_VENDOR_AMD;
}

bool cpu_is_virtualized(void) {
    /* 检查虚拟化特征或 QEMU 厂商 */
    return (g_cpu_info.vendor == CPU_VENDOR_QEMU) ||
           cpu_has_feature(CPU_FEATURE_VMX) ||
           cpu_has_feature(CPU_FEATURE_SVM);
}

void cpu_cpuid(uint32_t leaf, uint32_t subleaf,
               uint32_t *eax, uint32_t *ebx,
               uint32_t *ecx, uint32_t *edx) {
    __asm__ volatile(
        "cpuid"
        : "=a"(*eax), "=b"(*ebx), "=c"(*ecx), "=d"(*edx)
        : "a"(leaf), "c"(subleaf)
    );
}

uint32_t cpu_get_max_cpuid_leaf(void) {
    return g_cpu_info.max_cpuid_leaf;
}

uint32_t cpu_get_max_ext_cpuid_leaf(void) {
    return g_cpu_info.max_ext_cpuid_leaf;
}

uint32_t cpu_get_apic_id(void) {
    return (uint32_t)g_cpu_info.apic_id;
}

uint8_t cpu_get_logical_cores(void) {
    return g_cpu_info.logical_cores;
}

uint8_t cpu_get_physical_cores(void) {
    return g_cpu_info.physical_cores;
}

cpu_signature_t cpu_get_signature(void) {
    return g_cpu_info.signature;
}

const cpu_cache_info_t* cpu_get_cache_info(void) {
    return &g_cpu_info.cache;
}

uint64_t cpu_get_tsc_frequency(void) {
    return g_tsc_frequency;
}

/* ============================================================================ */
/*                        MSR 操作实现                                       */
/* ============================================================================ */

int cpu_read_msr(uint32_t msr, uint32_t *low, uint32_t *high) {
    if (!low || !high) return -1;
    
    __asm__ volatile(
        "rdmsr"
        : "=a"(*low), "=d"(*high)
        : "c"(msr)
    );
    
    return 0;
}

int cpu_write_msr(uint32_t msr, uint32_t low, uint32_t high) {
    __asm__ volatile(
        "wrmsr"
        :
        : "c"(msr), "a"(low), "d"(high)
        : "memory"
    );
    
    return 0;
}

uint64_t cpu_read_msr64(uint32_t msr) {
    uint32_t low, high;
    
    __asm__ volatile(
        "rdmsr"
        : "=a"(low), "=d"(high)
        : "c"(msr)
    );
    
    return ((uint64_t)high << 32) | low;
}

int cpu_write_msr64(uint32_t msr, uint64_t value) {
    uint32_t low = (uint32_t)(value & 0xFFFFFFFF);
    uint32_t high = (uint32_t)(value >> 32);
    
    return cpu_write_msr(msr, low, high);
}

/* ============================================================================ */
/*                        信息打印 (简化版)                                   */
/* ============================================================================ */

void cpu_print_info(void (*output_func)(const char*)) {
    if (!output_func) output_func = serial_output_wrapper;
    
    const cpu_info_t *info = &g_cpu_info;
    
    (*output_func)("\n");
    (*output_func)("╔════════════════════════════════════════════╗\n");
    (*output_func)("║       QX AMD64 CPU Information              ║\n");
    (*output_func)("╚════════════════════════════════════════════╝\n");
    
    (*output_func)("Vendor: ");
    (*output_func)(info->vendor_string);
    (*output_func)("\nBrand: ");
    (*output_func)(info->brand_string);
    (*output_func)("\nCores: ");
    serial_put_dec(SERIAL_COM1, info->physical_cores);
    (*output_func)(" physical, ");
    serial_put_dec(SERIAL_COM1, info->logical_cores);
    (*output_func)(" logical\n");
    
    if (cpu_has_feature(CPU_FEATURE_LM)) {
        (*output_func)("Mode: 64-bit Long Mode\n");
    }
    
    if (g_tsc_frequency > 0) {
        (*output_func)("TSC: ~");
        serial_put_dec(SERIAL_COM1, (uint32_t)(g_tsc_frequency / 1000000));
        (*output_func)(" MHz\n");
    }
}

/* ============================================================================ */
/*                        安全版本函数 (增强兼容性)                             */
/* ============================================================================ */

/**
 * @brief 安全的特性收集版本 (避免访问不支持的 CPUID leaf)
 */
static void cpu_collect_features_safe(cpu_info_t *info) {
    uint32_t eax, ebx, ecx, edx;
    int i;

    /* 清空特性位图 - 使用简单循环避免 __builtin_memset 问题 */
    for (i = 0; i < sizeof(cpu_features_t); i++) {
        ((uint8_t*)&info->features)[i] = 0;
    }

    /* 基础特性 (Leaf 1) - 几乎所有 CPU 都支持 */
    if (info->max_cpuid_leaf >= 1) {
        cpu_cpuid(1, 0, &eax, &ebx, &ecx, &edx);
        parse_features_edx(&info->features, edx);
        parse_features_ecx(&info->features, ecx);
    }

    /* 扩展特性 (Leaf 80000001) - 检查是否支持 */
    cpu_cpuid(0x80000000, 0, &eax, &ebx, &ecx, &edx);
    info->max_ext_cpuid_leaf = eax;

    if (info->max_ext_cpuid_leaf >= 0x80000001) {
        cpu_cpuid(0x80000001, 0, &eax, &ebx, &ecx, &edx);
        parse_extended_features(&info->features, edx, ecx);

        /* 品牌/型号字符串 (Leaf 80000002~4) */
        if (info->max_ext_cpuid_leaf >= 0x80000004) {
            uint32_t *brand = (uint32_t*)info->brand_string;

            cpu_cpuid(0x80000002, 0, &eax, &ebx, &ecx, &edx);
            brand[0] = eax; brand[1] = ebx; brand[2] = ecx; brand[3] = edx;

            cpu_cpuid(0x80000003, 0, &eax, &ebx, &ecx, &edx);
            brand[4] = eax; brand[5] = ebx; brand[6] = ecx; brand[7] = edx;

            cpu_cpuid(0x80000004, 0, &eax, &ebx, &ecx, &edx);
            brand[8] = eax; brand[9] = ebx; brand[10] = ecx; brand[11] = edx;

            info->brand_string[47] = '\0';
        } else {
            __builtin_strcpy(info->brand_string, "Unknown");
        }
    } else {
        __builtin_strcpy(info->brand_string, "Generic CPU");
    }
    
    /* 高级特性 (Leaf 7, Sub-leaf 0) - 仅在支持时访问 */
    if (info->max_cpuid_leaf >= 7) {
        cpu_cpuid(7, 0, &eax, &ebx, &ecx, &edx);
        parse_advanced_features(&info->features, ebx);
    }
}

/**
 * @brief 安全的缓存检测版本
 */
static void cpu_detect_cache_safe(cpu_info_t *info) {
    /* 使用保守的默认值 */
    info->cache.l1i_size = 32 * 1024;   /* bytes */
    info->cache.l1d_size = 32 * 1024;    /* bytes */
    info->cache.l2_size = 256 * 1024;    /* bytes */
    info->cache.l3_size = 0;             /* 可能不存在 */
    info->cache.l1_assoc = 4;
    info->cache.l2_assoc = 8;
    info->cache.l3_assoc = 0;
    info->cache.cache_line = 64;
    
    /* 尝试使用 CPUID 4 获取详细信息（如果支持）*/
    if (info->max_cpuid_leaf >= 4) {
        uint32_t eax, ebx, ecx, edx;
        
        cpu_cpuid(4, 0, &eax, &ebx, &ecx, &edx);
        
        /* 解析缓存信息 */
        uint32_t cache_type = eax & 0x1F;
        if (cache_type > 0) {
            uint32_t cache_level = (eax >> 5) & 0x7;
            
            if (cache_level == 1) {
                info->cache.l1d_size = ((ebx >> 22) + 1) * (ebx & 0x3FF);
            } else if (cache_level == 2) {
                info->cache.l2_size = ((ebx >> 22) + 1) * (ebx & 0x3FF);
            } else if (cache_level == 3) {
                info->cache.l3_size = ((ebx >> 22) + 1) * (ebx & 0x3FF);
            }
        }
    }
}

/**
 * @brief 安全的拓扑检测版本
 */
static void cpu_detect_topology_safe(cpu_info_t *info) {
    /* 使用保守的默认值 */
    info->logical_cores = 1;
    info->physical_cores = 1;
    info->hyperthreading_enabled = false;
    
    /* 尝试从 CPUID 1 获取逻辑处理器数 */
    if (info->max_cpuid_leaf >= 1) {
        uint32_t eax, ebx, ecx, edx;
        cpu_cpuid(1, 0, &eax, &ebx, &ecx, &edx);
        
        info->logical_cores = (ebx >> 16) & 0xFF;
        if (info->logical_cores == 0) info->logical_cores = 1;
        
        /* 检查超线程 */
        info->hyperthreading_enabled = (ecx >> 28) & 1;
        
        /* 如果没有超线程，物理核 = 逻辑核 */
        if (!info->hyperthreading_enabled) {
            info->physical_cores = info->logical_cores;
        } else {
            /* 有超线程时，假设每个物理核有 2 个逻辑核 */
            info->physical_cores = info->logical_cores / 2;
            if (info->physical_cores == 0) info->physical_cores = 1;
        }
    }
    
    /* APIC ID */
    if (info->max_cpuid_leaf >= 1) {
        uint32_t eax, ebx, ecx, edx;
        cpu_cpuid(1, 0, &eax, &ebx, &ecx, &edx);
        info->apic_id = (ebx >> 24) & 0xFF;
    }
}

/**
 * @brief 安全的 MSR 初始化版本
 *
 * 在 QEMU -cpu host 模式下，MSR 访问可能需要特殊权限。
 * 此函数会优雅地处理失败情况。
 */
static int cpu_init_msr_safe(void) {
    /*
     * 在虚拟化环境中，MSR 访问可能受限。
     * 我们不做实际的 MSR 初始化，只是标记 MSR 可用性。
     * 实际的 MSR 读写会在运行时检查。
     */
    
    /* 检查 CPU 是否支持 MSR (通过 CPUID.1:EDX[5]) */
    if (cpu_has_feature(CPU_FEATURE_MSR)) {
        serial_puts(SERIAL_COM1, "[CPU] MSR support detected\n");
    } else {
        serial_puts(SERIAL_COM1, "[CPU] MSR not supported by CPU\n");
        return -1;
    }

    /*
     * 启用 SSE/SSE2 支持
     * x86-64 要求 SSE/SSE2 存在，但 OS 必须设置 CR4.OSFXSR(bit9)
     * 和 CR4.OSXMMEXCPT(bit10) 才能使用 SSE 指令。
     * 否则 pxor/movaps 等 SSE 指令会触发 #UD (Invalid Opcode)。
     */
    {
        uint64_t cr4;
        __asm__ volatile("mov %%cr4, %0" : "=r"(cr4));
        cr4 |= (1UL << 9);   /* CR4.OSFXSR - enable FXSAVE/FXRSTOR + SSE */
        cr4 |= (1UL << 10);  /* CR4.OSXMMEXCPT - enable #XF exception */
        __asm__ volatile("mov %0, %%cr4" :: "r"(cr4) : "memory");
        serial_puts(SERIAL_COM1, "[CPU] SSE/SSE2 enabled (CR4.OSFXSR+OSXMMEXCPT)\n");
    }

    /* 启用 FPU (设置 CR0.TS=0, CR0.EM=0, CR0.MP=1) */
    {
        uint64_t cr0;
        __asm__ volatile("mov %%cr0, %0" : "=r"(cr0));
        cr0 &= ~(1UL << 3);  /* CR0.TS=0 - no task switched */
        cr0 &= ~(1UL << 2);  /* CR0.EM=0 - no FPU emulation */
        cr0 |= (1UL << 1);   /* CR0.MP=1 - monitor coprocessor */
        __asm__ volatile("mov %0, %%cr0" :: "r"(cr0) : "memory");

        /* 执行 FNINIT 初始化 FPU 状态 */
        __asm__ volatile("fninit");
    }

    return 0;
}

/**
 * @brief 安全的 TSC 校准版本 (使用保守估计)
 */
static uint64_t cpu_calibrate_tsc_safe(void) {
    uint64_t estimated_freq = 0;
    
    /*
     * 方法 1: 使用 CPUID 15H/16H (Intel TSC 频率)
     * 仅适用于较新的 Intel 处理器，且必须确保 leaf 存在
     */
    if (g_cpu_info.max_cpuid_leaf >= 0x15) {
        uint32_t eax, ebx, ecx, edx;

        cpu_cpuid(0x15, 0, &eax, &ebx, &ecx, &edx);

        if (eax && ebx && ecx) {
            /* TSC frequency = (ECX * EBX) / EAX */
            estimated_freq = ((uint64_t)ecx * (uint64_t)ebx) / (uint64_t)eax;
            if (estimated_freq > 0) {
                return estimated_freq * 1000000;  /* MHz -> Hz */
            }
        }
    }
    
    /*
     * 方法 2: 基于 CPU 型号/品牌的经验估计
     * 这不是精确的方法，但在没有 PIT 校准的情况下可用
     */
    if (g_cpu_info.vendor == CPU_VENDOR_INTEL) {
        switch (g_cpu_info.signature.family) {
            case 6:
                estimated_freq = 2500000000ULL;
                break;
            default:
                estimated_freq = 2000000000ULL;
                break;
        }
    } else if (g_cpu_info.vendor == CPU_VENDOR_AMD) {
        estimated_freq = 3000000000ULL;
    } else {
        estimated_freq = 1000000000ULL;
    }
    
    return estimated_freq;
}

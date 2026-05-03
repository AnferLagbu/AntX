/**
 * ============================================================================
 * cpu.h - QX (QueenX) AMD64 CPU 驱动接口定义
 * ============================================================================
 *
 * 功能:
 *   • 提供 AMD64 处理器的抽象接口
 *   • CPU 特性检测和管理
 *   • MSR (模型特定寄存器) 访问
 *   • 多核/SMP 支持
 *   • CPU 状态监控和性能计数
 *
 * 架构:
 *   ┌─────────────────────────────────────┐
 *   │         QX CPU Driver API          │  ← 本文件定义的接口
 *   ├─────────────────────────────────────┤
 *   │  ┌─────────┐ ┌─────────┐ ┌──────┐ │
 *   │  │ CPUID   │ │ MSR     │ │ SMP  │ │  ← 实现模块
 *   │  │ 检测    │ │ 管理    │ │ 支持 │ │
 *   │  └─────────┘ └─────────┘ └──────┘ │
 *   ├─────────────────────────────────────┤
 *   │      x86_64 硬件指令层              │  ← 内联汇编
 *   └─────────────────────────────────────┘
 *
 * 使用示例:
 *   // 初始化 CPU 驱动
 *   cpu_init();
 *
 *   // 获取 CPU 信息
 *   const cpu_info_t *info = cpu_get_info();
 *   printk("CPU: %s\n", info->vendor_string);
 *
 *   // 检查特性支持
 *   if (cpu_has_feature(CPU_FEATURE_SSE2)) {
 *       // 使用 SSE2 指令
 *   }
 *
 * 作者: AntX Development Team
 * 版本: 1.0 (2026-05-03)
 * ============================================================================
 */

#ifndef __CPU_H__
#define __CPU_H__

#include "types.h"

#ifdef __cplusplus
extern "C" {
#endif

/* ============================================================================ */
/*                        常量定义                                         */
/* ============================================================================ */

/** @brief 最大支持的 CPU 数量 (SMP) */
#define CPU_MAX_CORES          64

/** @brief 厂商字符串长度 */
#define CPU_VENDOR_STRING_LEN  16

/** @brief CPU 型号名称长度 */
#define CPU_BRAND_STRING_LEN   48

/** @brief CPU 特性位图大小 (bits) */
#define CPU_FEATURE_BITMAP_SIZE 512

/* ============================================================================ */
/*                        CPU 特性标志                                      */
/* ============================================================================ */

/**
 * @brief CPU 特性枚举 (基于 CPUID 结果)
 *
 * 分类:
 *   - 基础特性 (0x00000001 EDX/ECX)
 *   - 扩展特性 (0x80000001 EDX/ECX)
 *   - 高级特性 (leaf 7, subleaf 0)
 */
typedef enum {
    /* ====== 基础特性 (EDX, leaf 1) ====== */
    CPU_FEATURE_FPU        = 0,    /**< 浮点运算单元 (FPU) */
    CPU_FEATURE_VME        = 1,    /**< 虚拟8086模式扩展 */
    CPU_FEATURE_DE         = 2,    /**< 调试扩展 (Debugging Extensions) */
    CPU_FEATURE_PSE        = 3,    /**< 页面大小扩展 (Page Size Extension) */
    CPU_FEATURE_TSC        = 4,    /**< 时间戳计数器 (Time Stamp Counter) */
    CPU_FEATURE_MSR        = 5,    /**< 模型特定寄存器 (MSR) */
    CPU_FEATURE_PAE        = 6,    /**< 物理地址扩展 (Physical Address Ext.) */
    CPU_FEATURE_MCE        = 7,    /**< 机器检查异常 (Machine Check Exception) */
    CPU_FEATURE_CX8        = 8,    /**< CMPXCHG8B 指令 */
    CPU_FEATURE_APIC       = 9,    /**< 片上 APIC (Advanced Programmable IC) */
    CPU_FEATURE_SEP        = 11,   /**< 快速系统调用 (SYSCALL/SYSRET) */
    CPU_FEATURE_MTRR       = 12,   /**< 内存类型范围寄存器 */
    CPU_FEATURE_PGE        = 13,   /**< 页面全局启用 (Page Global Enable) */
    CPU_FEATURE_MCA        = 14,   /**< 机器检查架构 (Machine Check Arch.) */
    CPU_FEATURE_CMOV       = 15,   /**< 条件移动指令 (CMOV) */
    CPU_FEATURE_PAT        = 16,   /**< 页面属性表 (Page Attribute Table) */
    CPU_FEATURE_PSE36      = 17,   /**< 36位页面尺寸扩展 */
    CPU_FEATURE_CLFLUSH    = 19,   /**< CLFLUSH 指令 */
    CPU_FEATURE_MMX        = 23,   /**< MMX 技术支持 */
    CPU_FEATURE_FXSR       = 24,   /**< FXSAVE/FXRSTOR 指令 */
    CPU_FEATURE_SSE        = 25,   /**< 流式 SIMD 扩展 (SSE) */
    CPU_FEATURE_SSE2       = 26,   /**< SSE2 */
    CPU_FEATURE_SS         = 27,   /**< 自旋锁指令 (Self Snoop) */
    CPU_FEATURE_HTT        = 28,   /**< 超线程技术 (Hyper-Threading) */
    CPU_FEATURE_SSE3       = 0,    /**< SSE3 (ECX, leaf 1) + 32 */
    CPU_FEATURE_MONITOR    = 3 + 32,/**< MONITOR/MWAIT 指令 + 32 */
    CPU_FEATURE_VMX        = 5 + 32,/**< 虚拟化技术 (Intel VMX) + 32 */
    CPU_FEATURE_SMX        = 6 + 32,/**< 安全模式扩展 (Safer Mode Ext.) + 32 */
    CPU_FEATURE_EST        = 7 + 32,/**< 增强型 Intel SpeedStep + 32 */
    CPU_FEATURE_TM2        = 8 + 32,/**< Thermal Monitor 2 + 32 */
    CPU_FEATURE_SSSE3      = 9 + 32,/**< 补充 SIMD 扩展 (SSSE3) + 32 */
    CPU_FEATURE_CID        = 10+32,/**< Context ID + 32 */
    CPU_FEATURE_CX16       = 13+32,/**< CMPXCHG16B 指令 + 32 */
    CPU_FEATURE_XTPR       = 14+32,/**< 任务优先级寄存器控制 + 32 */
    CPU_FEATURE_PDCM       = 15+32,/**< 性能/调试能力 MSR + 32 */
    CPU_FEATURE_PCID       = 17+32,/**< 进程上下文标识符 (PCID) + 32 */
    CPU_FEATURE_SSE41      = 19+32,/**< SSE4.1 + 32 */
    CPU_FEATURE_SSE42      = 20+32,/**< SSE4.2 + 32 */
    CPU_FEATURE_X2APIC     = 21+32,/**< x2APIC 特性 + 32 */
    CPU_FEATURE_MOVBE      = 22+32,/**< MOVBE 指令 + 32 */
    CPU_FEATURE_POPCNT     = 23+32,/**< POPCNT 指令 + 32 */
    CPU_FEATURE_AES        = 25+32,/**< AESNI 指令集 + 32 */
    CPU_FEATURE_XSAVE      = 26+32,/**< XSAVE/XRSTOR 指令 + 32 */
    CPU_FEATURE_OSXSAVE    = 27+32,/**< 操作系统对 XSAVE 的支持 + 32 */
    CPU_FEATURE_AVX        = 28+32,/**< 高级向量扩展 (AVX) + 32 */

    /* ====== 扩展特性 (EDX, leaf 80000001) ====== */
    CPU_FEATURE_SYSCALL    = 11+64, /**< SYSCALL/SYSRET (AMD64) + 64 */
    CPU_FEATURE_NX         = 20+64, /**< 不执行位 (No-Execute Bit) + 64 */
    CPU_FEATURE_MMXEXT     = 22+64, /**< AMD MMX 扩展 + 64 */
    CPU_FEATURE_FFXSR      = 24+64, /**< 快速 FXSAVE/FXRSTOR (FFXSR) + 64 */
    CPU_FEATURE_1GBPAGE    = 26+64, /**< 1 GB 大页支持 + 64 */
    CPU_FEATURE_RDTSCP     = 27+64, /**< RDTSCP 指令 + 64 */
    CPU_FEATURE_LM         = 29+64, /**< 长模式 (Long Mode, 64-bit) + 64 */
    CPU_FEATURE_3DNOWEXT   = 30+64, /**< 3DNow! 扩展 + 64 */
    CPU_FEATURE_3DNOW      = 31+64, /**< 3DNow! 技术 + 64 */

    /* ====== 扩展特性 (ECX, leaf 80000001) ====== */
    CPU_FEATURE_LAHF_LM    = 0+96,   /**< LAHF/SAHF 在 64 位模式 + 96 */
    CPU_FEATURE_CMP_LEGACY = 1+96,   /**< 比较与交换遗留模式 + 96 */
    CPU_FEATURE_SVM        = 2+96,   /**< 安全虚拟机 (AMD SVM) + 96 */
    CPU_FEATURE_ABM        = 5+96,   /**< 高级位操作 (ABM) + 96 */
    CPU_FEATURE_SSE4A      = 6+96,   /**< SSE4a (AMD) + 96 */
    CPU_FEATURE_MISALIGN   = 7+96,   /**< 未对齐访问模式 + 96 */
    CPU_FEATURE_PREFETCHW  = 8+96,   /**< PREFETCHW 指令 + 96 */
    CPU_FEATURE_OSVW       = 9+96,   /**< 操作系统可见工作区 + 96 */
    CPU_FEATURE_IBS        = 10+96,  /**< 基于指令的采样 (IBS) + 96 */
    CPU_FEATURE_SKINIT     = 12+96,  /**< SKINIT/STGI 指令 + 96 */
    CPU_FEATURE_WDT        = 13+96,  /**< 看门狗定时器 + 96 */

    /* ====== 高级特性 (Leaf 7, Sub-leaf 0 EBX) ====== */
    CPU_FEATURE_FSGSBASE   = 0+128,  /**< RDFGSBASE/WRFSBASE + 128 */
    CPU_FEATURE_TSC_ADJUST = 1+128,  /**< TSC 调整 MSR + 128 */
    CPU_FEATURE_BMI1       = 3+128,  /**< BMI1 指令集 + 128 */
    CPU_FEATURE_HLE        = 4+128,  /**< 硬件锁省略 (HLE) + 128 */
    CPU_FEATURE_AVX2       = 5+128,  /**< AVX2 指令集 + 128 */
    CPU_FEATURE_FMA        = 12+128, /**< FMA (融合乘加) + 128 */
    CPU_FEATURE_BMI2       = 8+128,  /**< BMI2 指令集 + 128 */
    CPU_FEATURE_ERMS       = 9+128,  /**< 增强型 REP MOVSB/STOSB + 128 */
    CPU_FEATURE_INVPCID    = 10+128, /**< INVPCID 指令 + 128 */
    CPU_FEATURE_RTM        = 11+128, /**< 有限事务内存 (RTM) + 128 */
    CPU_FEATURE_MPX        = 14+128, /**< 内存保护扩展 (MPX) + 128 */
    CPU_FEATURE_AVX512F    = 16+128, /**< AVX-512 基础 + 128 */
    CPU_FEATURE_AVX512DQ   = 17+128, /**< AVX-512 双字/四字 + 128 */
    CPU_FEATURE_RDSEED     = 18+128, /**< RDSEED 指令 + 128 */
    CPUFEATURE_ADX        = 19+128, /**< ADX (多精度加法) + 128 */
    CPU_FEATURE_AVX512IFMA = 21+128,/**< AVX-512 整数融合乘加 + 128 */
    CPU_FEATURE_CLWB       = 24+128, /**< CACHE LINE WRITE BACK + 128 */
    CPU_FEATURE_AVX512CD   = 28+128, /**< AVX-512 冲突检测 + 128 */
    CPU_FEATURE_SHA        = 29+128, /**< SHA 指令扩展 + 128 */
    CPU_FEATURE_AVX512BW   = 30+128, /**< AVX-512 字节/字 + 128 */
    CPU_FEATURE_AVX512VL   = 31+128, /**< AVX-512 128 位向量长度 + 128 */

    /* ====== 特殊标记 ====== */
    CPU_FEATURE_MAX        = 160    /**< 特性总数上限 */
} cpu_feature_t;

/* ============================================================================ */
/*                        数据结构定义                                       */
/* ============================================================================ */

/**
 * @brief CPU 厂商类型
 */
typedef enum {
    CPU_VENDOR_UNKNOWN = 0,  /**< 未知厂商 */
    CPU_VENDOR_INTEL,        /**< Intel */
    CPU_VENDOR_AMD,          /**< AMD */
    CPU_VENDOR_VIA,          /**< VIA */
    CPU_VENDOR_TRANSMETA,    /**< Transmeta */
    CPU_VENDOR_CYRIX,        /**< Cyrix */
    CPU_VENDOR_QEMU,         /**< QEMU 虚拟化 */
} cpu_vendor_t;

/**
 * @brief CPU 家族/型号信息
 */
typedef struct {
    uint8_t stepping;    /**< 步进 (Stepping) */
    uint8_t model;       /**< 型号 (Model) */
    uint8_t family;      /**< 家族 (Family) */
    uint8_t type;        /**< 类型 (Type): 0=OEM, 1=Overdrive, 2=Dual */
    uint8_t ext_model;   /**< 扩展型号 (Extended Model) */
    uint8_t ext_family;  /**< 扩展家族 (Extended Family) */
} cpu_signature_t;

/**
 * @brief CPU 缓存信息
 */
typedef struct {
    uint32_t l1d_size;   /**< L1 数据缓存大小 (bytes) */
    uint32_t l1i_size;   /**< L1 指令缓存大小 (bytes) */
    uint32_t l2_size;    /**< L2 统一缓存大小 (bytes) */
    uint32_t l3_size;    /**< L3 缓存大小 (bytes), 0=无 */
    uint8_t  l1_assoc;   /**< L1 关联度 */
    uint8_t  l2_assoc;   /**< L2 关联度 */
    uint8_t  l3_assoc;   /**< L3 关联度 */
    uint8_t  cache_line; /**< 缓存行大小 (bytes) */
} cpu_cache_info_t;

/**
 * @brief CPU 特性位图
 *
 * 使用位数组存储所有 CPU 特性标志。
 * 每个比特代表一个特性的存在与否。
 */
typedef struct {
    /**
     * @brief 特性位图数组
     *
     * 索引计算: feature / 8
     * 位掩码: 1 << (feature % 8)
     */
    uint8_t bitmap[CPU_FEATURE_BITMAP_SIZE / 8];
} cpu_features_t;

/**
 * @brief 完整的 CPU 信息结构体
 *
 * 包含处理器的所有关键信息：
 * - 标识信息（厂商、型号、家族）
 * - 能力信息（特性、缓存）
 * - 运行状态（频率、温度）
 */
typedef struct {
    /** ====== 标识信息 ====== */
    char vendor_string[CPU_VENDOR_STRING_LEN];  /**< 厂商字符串 (如 "GenuineIntel") */
    char brand_string[CPU_BRAND_STRING_LEN];    /**< 品牌/型号字符串 (如 "Intel(R) Core(TM) i7") */
    cpu_vendor_t vendor;                         /**< 厂商枚举 */
    cpu_signature_t signature;                  /**< CPU 签名 (步进/型号/家族) */

    /** ====== 能力信息 ====== */
    cpu_features_t features;                    /**< 支持的特性位图 */
    cpu_cache_info_t cache;                     /**< 缓存层次结构 */

    /** ====== 运行状态 ====== */
    uint32_t max_cpuid_leaf;                    /**< 最大支持的 CPUID leaf */
    uint32_t max_ext_cpuid_leaf;                /**< 最大支持的扩展 CPUID leaf */
    uint64_t apic_id;                           /**< Local APIC ID */
    uint8_t  core_id;                           /**< 逻辑核心 ID */
    uint8_t  thread_id;                         /**< 线程 ID (SMT) */
    uint8_t  logical_cores;                     /**< 逻辑核心数 */
    uint8_t  physical_cores;                    /**< 物理核心数 */
    bool     hyperthreading_enabled;            /**< 超线程是否启用 */

    /** ====== 初始化状态 ====== */
    bool initialized;                            /**< 是否已初始化 */
} cpu_info_t;

/* ============================================================================ */
/*                        函数原型声明                                       */
/* ============================================================================ */

/* ---------- 初始化和配置 ---------- */

/**
 * @brief 初始化 CPU 驱动
 *
 * 执行以下操作：
 * 1. 检测 CPU 厂商和型号 (CPUID)
 * 2. 收集特性信息
 * 3. 配置必要的 MSR
 * 4. 设置性能计数器
 *
 * @return 0 成功, <0 错误代码
 *
 * 必须在内核启动早期调用（在 GDT/IDT 之后）。
 */
int cpu_init(void);

/**
 * @brief 获取当前 CPU 信息
 *
 * 返回指向静态 cpu_info_t 结构体的指针。
 * 该结构体在 cpu_init() 时填充。
 *
 * @return 指向 CPU 信息结构体的指针 (只读)
 */
const cpu_info_t* cpu_get_info(void);

/**
 * @brief 打印 CPU 详细信息到指定输出
 *
 * 格式化输出 CPU 的完整信息，包括：
 * - 厂商和型号
 * - 家族/步进
 * - 支持的特性列表
 * - 缓存配置
 * - 多核状态
 *
 * @param output_func 输出函数指针 (如 serial_puts)
 *                   签名: void (*fn)(const char *)
 */
void cpu_print_info(void (*output_func)(const char*));

/* ---------- 特性检测 ---------- */

/**
 * @brief 检查 CPU 是否支持指定特性
 *
 * @param feature 要检查的特性 (cpu_feature_t 枚举)
 * @return true 支持, false 不支持
 *
 * 示例:
 * @code
 * if (cpu_has_feature(CPU_FEATURE_AVX2)) {
 *     // 使用 AVX2 指令优化
 * }
 * @endcode
 */
bool cpu_has_feature(cpu_feature_t feature);

/**
 * @brief 检查是否为 Intel CPU
 * @return true 如果是 Intel
 */
bool cpu_is_intel(void);

/**
 * @brief 检查是否为 AMD CPU
 * @return true 如果是 AMD
 */
bool cpu_is_amd(void);

/**
 * @brief 检查是否运行在虚拟化环境
 * @return true 如果是虚拟机 (QEMU/KVM/VirtualBox 等)
 */
bool cpu_is_virtualized(void);

/* ---------- MSR 管理 ---------- */

/**
 * @brief 读取 MSR (模型特定寄存器)
 *
 * @param msr MSR 地址 (如 IA32_EFER = 0xC0000080)
 * @param low  [输出] 低 32 位值
 * @param high [输出] 高 32 位值
 * @return 0 成功, -1 失败 (MSR 不存在或权限不足)
 */
int cpu_read_msr(uint32_t msr, uint32_t *low, uint32_t *high);

/**
 * @brief 写入 MSR
 *
 * @param msr  MSR 地址
 * @param low  低 32 位值
 * @param high 高 32 位值
 * @return 0 成功, -1 失败
 */
int cpu_write_msr(uint32_t msr, uint32_t low, uint32_t high);

/**
 * @brief 读取 64 位 MSR 值
 *
 * 便捷封装，自动组合低/高 32 位。
 *
 * @param msr MSR 地址
 * @return 64 位 MSR 值 (失败时返回 0)
 */
uint64_t cpu_read_msr64(uint32_t msr);

/**
 * @brief 写入 64 位 MSR 值
 *
 * @param msr  MSR 地址
 * @param value 64 位值
 * @return 0 成功, -1 失败
 */
int cpu_write_msr64(uint32_t msr, uint64_t value);

/* ---------- CPUID 接口 ---------- */

/**
 * @brief 执行 CPUID 指令
 *
 * 封装原始 CPUID 指令，返回 4 个寄存器的值。
 *
 * @param leaf     CPUID 主叶 (EAX 输入)
 * @param subleaf  CPUID 子叶 (ECX 输入, 可选)
 * @param eax      [输出] EAX 结果
 * @param ebx      [输出] EBX 结果
 * @param ecx      [输出] ECX 结果
 * @param edx      [输出] EDX 结果
 */
void cpu_cpuid(uint32_t leaf, uint32_t subleaf,
               uint32_t *eax, uint32_t *ebx,
               uint32_t *ecx, uint32_t *edx);

/**
 * @brief 获取最大支持的 CPUID 叶
 * @return 最大标准叶号 (通常 >= 0x0F)
 */
uint32_t cpu_get_max_cpuid_leaf(void);

/**
 * @brief 获取最大支持的扩展 CPUID 叶
 * @return 最大扩展叶号 (通常 >= 0x8000001F)
 */
uint32_t cpu_get_max_ext_cpuid_leaf(void);

/* ---------- 状态查询 ---------- */

/**
 * @brief 获取当前 CPU 的 APIC ID
 * @return Local APIC ID (0-based)
 */
uint32_t cpu_get_apic_id(void);

/**
 * @brief 获取逻辑核心数
 * @return 当前系统的逻辑处理器数 (含超线程)
 */
uint8_t cpu_get_logical_cores(void);

/**
 * @brief 获取物理核心数
 * @return 物理核心数 (不含超线程)
 */
uint8_t cpu_get_physical_cores(void);

/**
 * @brief 获取 CPU 步进/型号/家族签名
 * @return CPU 签名结构体
 */
cpu_signature_t cpu_get_signature(void);

/**
 * @brief 获取缓存信息
 * @return 缓存信息结构体
 */
const cpu_cache_info_t* cpu_get_cache_info(void);

/* ---------- 性能和诊断 ---------- */

/**
 * @brief 获取 TSC (时间戳计数器) 频率估算
 *
 * 通过校准测量 TSC 频率 (Hz)。
 *
 * @return TSC 频率 (Hz), 0 表示无法测量
 */
uint64_t cpu_get_tsc_frequency(void);

/**
 * @brief 读取 TSC 当前值
 * @return 64 位时间戳
 */
static inline uint64_t cpu_rdtsc(void) {
    uint32_t low, high;
    __asm__ volatile("rdtsc" : "=a"(low), "=d"(high));
    return ((uint64_t)high << 32) | low;
}

/**
 * @brief 读取 TSC 并添加序列化屏障
 *
 * 确保 RDTSC 之前的指令都已执行完毕。
 *
 * @return 64 位时间戳
 */
static inline uint64_t cpu_rdtsc_serialized(void) {
    unsigned int aux;
    uint64_t val;
    __asm__ volatile("cpuid" : "=a"(aux) : "a"(0) : "%ebx", "%ecx", "%edx");
    __asm__ volatile("rdtsc" : "=a"(val), "=d"(((uint32_t*)&val)[1]));
    return val;
}

/**
 * @brief CPU 空闲指令 (节能)
 *
 * 执行 HLT 或 MWAIT 指令让 CPU 进入低功耗状态。
 * 直到下一个中断唤醒。
 */
static inline void cpu_hlt(void) {
    __asm__ volatile("hlt");
}

/**
 * @brief CPU 暂停指令 (自旋锁优化)
 *
 * 提示 CPU 当前处于自旋等待状态，
 * 允许 CPU 进行功耗优化。
 */
static inline void cpu_pause(void) {
    __asm__ volatile("pause");
}

/**
 * @brief 序列化操作 (内存屏障)
 *
 * 确保之前的所有加载/存储操作都已完成。
 */
static inline void cpu_serialize(void) {
    __asm__ volatile("mfence");
}

/**
 * @brief 读取 CR0 寄存器
 * @return CR0 值
 */
static inline uint64_t cpu_read_cr0(void) {
    uint64_t value;
    __asm__ volatile("mov %%cr0, %0" : "=r"(value));
    return value;
}

/**
 * @brief 读取 CR2 寄存器 (页故障地址)
 * @return CR2 值
 */
static inline uint64_t cpu_read_cr2(void) {
    uint64_t value;
    __asm__ volatile("mov %%cr2, %0" : "=r"(value));
    return value;
}

/**
 * @brief 读取 CR3 寄存器 (PML4 基址)
 * @return CR3 值
 */
static inline uint64_t cpu_read_cr3(void) {
    uint64_t value;
    __asm__ volatile("mov %%cr3, %0" : "=r"(value));
    return value;
}

/**
 * @brief 写入 CR3 寄存器
 * @param value 新的 PML4 基址
 */
static inline void cpu_write_cr3(uint64_t value) {
    __asm__ volatile("mov %0, %%cr3" :: "r"(value));
}

/**
 * @brief 读取 CR4 寄存器
 * @return CR4 值
 */
static inline uint64_t cpu_read_cr4(void) {
    uint64_t value;
    __asm__ volatile("mov %%cr4, %0" : "=r"(value));
    return value;
}

/**
 * @brief 使 TLB 条目失效 (单个地址)
 * @param addr 要失效的虚拟地址
 */
static inline void cpu_invlpg(void *addr) {
    __asm__ volatile("invlpg (%0)" :: "r"(addr) : "memory");
}

/**
 * @brief 使整个 TLB 失效
 */
static inline void cpu_flush_tlb(void) {
    cpu_write_cr3(cpu_read_cr3());
}

/* ---------- 多核支持 (SMP) ---------- */

#ifdef CONFIG_SMP

/**
 * @brief 初始化多核支持 (AP 启动)
 *
 * 仅在编译时启用了 CONFIG_SMP 时可用。
 *
 * @return 0 成功, <0 错误
 */
int cpu_smp_init(void);

/**
 * @brief 获取当前执行的 CPU ID
 * @return CPU ID (0 ~ logical_cores-1)
 */
uint8_t cpu_get_current_id(void);

/**
 * @brief 发送 IPI (处理器间中断)
 *
 * @param target_cpu 目标 CPU ID
 * @param vector     中断向量
 * @return 0 成功, -1 失败
 */
int cpu_send_ipi(uint8_t target_cpu, uint8_t vector);

#endif /* CONFIG_SMP */

#ifdef __cplusplus
}
#endif

#endif /* __CPU_H__ */

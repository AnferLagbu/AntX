#include "smp.h"
#include "serial.h"
#include "idt.h"
#include "io.h"
#include "string.h"

/**
 * @brief SMP (Symmetric Multi-Processing) 实现
 *
 * 支持 x86_64 多核处理器初始化和管理。
 * 使用 Intel/AMD 的 Local APIC 进行核间通信。
 */

/* 全局 CPU 信息表 */
static cpu_info_t cpus[MAX_CPUS];
static int cpu_count = 0;
static int bsp_cpu_id = -1;

/* IPI 处理程序表 */
static ipi_handler_t ipi_handlers[IPI_MAX_TYPES];

/* Barrier 同步变量 */
static volatile int barrier_count = 0;
static volatile int barrier_target = 0;

/* AP 启动代码 (16-bit real mode) */
__attribute__((aligned(4096)))
static uint8_t ap_boot_code[] = {
    /* 简化的 AP 启动 stub */
    0xFA,                           /* CLI */
    0xB8, 0x00, 0xA0, 0x00, 0x00,   /* MOV AX, 0xA000 */
    0x8E, 0xD8,                     /* MOV DS, AX */
    0x8E, 0xC0,                     /* MOV ES, AX */
    0x8E, 0xD0,                     /* MOV SS, AX */
    0xBC, 0x00, 0x7C, 0x00, 0x00,   /* MOV SP, 0x7C00 */
    0xEA, ...                       /* JMP to long mode entry */
};

/**
 * @brief 读取 MSR 寄存器
 */
static inline uint64_t read_msr(uint32_t msr) {
    uint32_t low, high;
    __asm__ volatile (
        "rdmsr"
        : "=a"(low), "=d"(high)
        : "c"(msr)
    );
    return ((uint64_t)high << 32) | low;
}

/**
 * @brief 写入 MSR 寄存器
 */
static inline void write_msr(uint32_t msr, uint64_t value) {
    uint32_t low = value & 0xFFFFFFFF;
    uint32_t high = value >> 32;
    __asm__ volatile (
        "wrmsr"
        :
        : "c"(msr), "a"(low), "d"(high)
    );
}

/**
 * @brief 检测并初始化 Local APIC
 *
 * @param cpu CPU 信息结构指针
 * @return 0 成功，-1 失败
 */
static int init_local_apic(cpu_info_t *cpu) {
    uint32_t apic_base_msr = read_msr(0x1B);  /* IA32_APIC_BASE */

    cpu->apic_base = apic_base_msr & 0xFFFFF000;
    cpu->is_bsp = (apic_base_msr >> 8) & 1;   /* BSP bit */
    cpu->apic_id = ((apic_base_msr >> 24) & 0xFF);

    /* 映射 APIC 到虚拟地址空间 */
    extern int vmm_map_page(uint64_t virt, uint64_t phys, uint64_t flags);
    uint64_t apic_virt = 0xFFFFF80000000000ULL + (uint64_t)cpu->apic_base;

    if (vmm_map_page(apic_virt, cpu->apic_base, 0x03) != 0) {
        serial_puts(SERIAL_COM1, "[SMP] ERROR: Failed to map APIC for CPU ");
        serial_put_dec(SERIAL_COM1, cpu->apic_id);
        return -1;
    }

    cpu->local_apic = (volatile uint32_t *)apic_virt;

    /* 验证 APIC 是否存在 */
    uint32_t version = cpu->local_apic[0x30 / sizeof(uint32_t)];
    if ((version & 0xFF) == 0) {
        serial_puts(SERIAL_COM1, "[SMP] WARNING: No APIC detected for CPU ");
        serial_put_dec(SERIAL_COM1, cpu->apic_id);
        return -1;
    }

    /* 启用 Local APIC */
    uint32_t svr = cpu->local_apid[0xF0 / sizeof(uint32_t)];
    svr |= 0x100;  /* Enable bit */
    cpu->local_apic[0xF0 / sizeof(uint32_t)] = svr;

    serial_puts(SERIAL_COM1, "[SMP] Local APIC initialized for CPU ");
    serial_put_dec(SERIAL_COM1, cpu->apic_id);
    serial_puts(SERIAL_COM1, " at 0x");
    serial_put_hex(SERIAL_COM1, cpu->apic_base);
    serial_puts(SERIAL_COM1, "\n");

    return 0;
}

/**
 * @brief 为 AP 分配栈空间
 *
 * @param cpu_id CPU 编号
 * @return 栈顶地址，NULL 表示失败
 */
static void* allocate_ap_stack(int cpu_id) {
    extern void* kmalloc(uint64_t size);
    extern void* kcalloc(uint64_t num, uint64_t size);

    void *stack = kcalloc(1, AP_STACK_SIZE);
    if (stack == NULL) {
        serial_puts(SERIAL_COM1, "[SMP] ERROR: Failed to allocate stack for CPU ");
        serial_put_dec(SERIAL_COM1, cpu_id);
        return NULL;
    }

    /* 栈向下增长，返回顶部 */
    return (void*)((uint8_t*)stack + AP_STACK_SIZE);
}

/**
 * @brief 发送 INIT IPI 到指定 AP
 *
 * @param target_apic_id 目标 APIC ID
 */
static void send_init_ipi(uint8_t target_apic_id) {
    volatile uint32_t *lapic = cpus[0].local_apic;  /* 使用 BSP 的 LAPIC */

    /* 设置目标 */
    lapic[0x310 / sizeof(uint32_t)] = (uint32_t)target_apic_id << 24;

    /* 发送 INIT IPI */
    lapic[0x300 / sizeof(uint32_t)] = 0x000C5000;  /* Level, assert, Init */

    /* 等待 10ms */
    for (volatile int i = 0; i < 1000000; i++);
}

/**
 * @brief 发送 STARTUP IPI 到指定 AP
 *
 * @param target_apic_id 目标 APIC ID
 * @param page_num 启动代码页号 (0-255)
 */
static void send_startup_ipi(uint8_t target_apic_id, uint8_t page_num) {
    volatile uint32_t *lapic = cpus[0].local_apic;  /* 使用 BSP 的 LAPIC */

    /* 设置目标 */
    lapic[0x310 / sizeof(uint32_t)] = (uint32_t)target_apic_id << 24;

    /* 发送 STARTUP IPI */
    lapic[0x300 / sizeof(uint32_t)] =
        0x00060600 |  /* Level, assert, Startup */
        (page_num & 0xFF);  /* 启动向量 */
}

/**
 * @brief AP 启动入口点 (Long Mode)
 *
 * 这个函数由 AP 在进入保护模式后调用。
 * 必须是独立的、不依赖全局变量的函数。
 */
__attribute__((noreturn))
static void ap_entry_long_mode(void) {
    /*
     * AP 在这里执行：
     * 1. 加载 Per-CPU GDT 和 TSS
     * 2. 设置内核栈
     * 3. 调用 smp_ap_ready() 标记就绪
     * 4. 进入空闲循环或调度循环
     */

    /* TODO: 实现完整的 AP 初始化序列 */

    while (1) {
        __asm__ volatile ("hlt");
    }
}

/**
 * @brief AP 就绪回调
 *
 * 由 AP 调用以通知 BSP 它已准备好。
 */
void smp_ap_ready(void) {
    cpu_info_t *current = smp_get_current_cpu();
    if (current == NULL) return;

    current->state = CPU_STATE_RUNNING;

    serial_puts(SERIAL_COM1, "[SMP] CPU ");
    serial_put_dec(SERIAL_COM1, current->cpu_id);
    serial_puts(SERIAL_COM1, " (APIC ");
    serial_put_dec(SERIAL_COM1, current->apic_id);
    serial_puts(SERIAL_COM1, ") is now RUNNING\n");
}

/**
 * @brief IPI 中断处理程序
 *
 * 统一处理所有 IPI 类型。
 */
void smp_ipi_handler(struct interrupt_frame *frame) {
    cpu_info_t *current = smp_get_current_cpu();
    if (current == NULL) return;

    current->ipi_received++;

    /* 从 vector 获取 IPI 类型 (假设 vector = IPI_INTERRUPT + type) */
    ipi_type_t type = (ipi_type_t)(frame->int_no - 0xF0);  /* 假设 IPI base = 0xF0 */

    if (type >= 0 && type < IPI_MAX_TYPES && ipi_handlers[type] != NULL) {
        ipi_handlers[type](current, NULL);
    } else {
        serial_puts(SERIAL_COM1, "[SMP] Unknown IPI type: ");
        serial_put_dec(SERIAL_COM1, type);
        serial_puts(SERIAL_COM1, "\n");
    }
}

/**
 * @brief 初始化 BSP (Bootstrap Processor)
 *
 * @return 0 成功，-1 失败
 */
static int init_bsp(void) {
    cpu_info_t *bsp = &cpus[0];

    __builtin_memset(bsp, 0, sizeof(cpu_info_t));

    bsp->cpu_id = 0;
    bsp->state = CPU_STATE_RUNNING;

    /* 初始化 Local APIC */
    if (init_local_apic(bsp) != 0) {
        return -1;
    }

    /* BSP 的 APIC ID 应该已经设置 */
    bsp->is_bsp = 1;
    bsp_cpu_id = 0;

    /* 分配内核栈 */
    bsp->kernel_stack = allocate_ap_stack(0);
    if (bsp->kernel_stack == NULL) {
        return -1;
    }

    /* 设置当前 CPU 为 BSP */
    extern void write_gs_base(uint64_t base);
    write_gs_base((uint64_t)bsp);

    serial_puts(SERIAL_COM1, "[SMP] BSP initialized: APIC ID=");
    serial_put_dec(SERIAL_COM1, bsp->apic_id);
    serial_puts(SERIAL_COM1, ", CPU ID=0\n");

    cpu_count = 1;
    return 0;
}

/**
 * @brief 检测可用 CPU 数量 (通过 ACPI MADT 或 MP Tables)
 *
 * @return 检测到的 CPU 数量，如果无法检测返回 1 (仅 BSP)
 */
static int detect_cpus(void) {
    /*
     * 方法 1: 搜索 RSDP → RSDT/XSDT → MADT
     * 方法 2: 搜索 MP Floating Pointer Structure
     * 方法 3: 使用 QEMU 默认配置 (-smp N)
     *
     * 这里简化实现：假设最多 4 个 CPU
     * 完整实现需要解析 ACPI 表
     */

    /* 尝试读取 QEMU -smp 参数或检测硬件 */
    /* 暂时硬编码为 4 (可通过编译选项修改) */
    #ifdef CONFIG_SMP_MAX_CPUS
    return CONFIG_SMP_MAX_CPUS;
    #else
    return 4;  /* 默认假设 4 核 */
    #endif
}

/**
 * @brief 启动指定的 Application Processor
 *
 * @param cpu_id 要启动的 CPU 编号 (1-based)
 * @return 0 成功，-1 失败
 */
static int start_ap(int cpu_id) {
    if (cpu_id <= 0 || cpu_id >= MAX_CPUS) return -1;

    cpu_info_t *ap = &cpus[cpu_id];
    __builtin_memset(ap, 0, sizeof(cpu_info_t));

    ap->cpu_id = cpu_id;
    ap->state = CPU_STATE_BOOTING;

    /* 假设 APIC ID = CPU ID (简化) */
    ap->apic_id = (uint8_t)(cpu_id);

    /* 分配栈 */
    ap->kernel_stack = allocate_ap_stack(cpu_id);
    if (ap->kernel_stack == NULL) {
        ap->state = CPU_STATE_ERROR;
        return -1;
    }

    /* 将启动代码复制到低内存 (0x7000) */
    extern int vmm_map_page(uint64_t virt, uint64_t phys, uint64_t flags);
    vmm_map_page(AP_BOOT_ADDR, AP_BOOT_ADDR, 0x03);  /* Identity map */

    __builtin_memcpy((void*)AP_BOOT_ADDR, ap_boot_code, sizeof(ap_boot_code));

    /* 发送 INIT-SIPI-SIPI 序列 */
    send_init_ipi(ap->apic_id);

    /* 等待 10ms */
    for (volatile int i = 0; i < 10000000; i++);

    /* 第一次 SIPI */
    send_startup_ipi(ap->apic_id, AP_BOOT_ADDR >> 12);

    /* 等待 200μs */
    for (volatile int i = 0; i < 200000; i++);

    /* 第二次 SIPI (Intel 要求) */
    send_startup_ipi(ap->apic_id, AP_BOOT_ADDR >> 12);

    /* 等待 AP 启动 */
    for (int timeout = 0; timeout < 1000000; timeout++) {
        if (ap->state == CPU_STATE_RUNNING) {
            serial_puts(SERIAL_COM1, "[SMP] AP ");
            serial_put_dec(SERIAL_COM1, cpu_id);
            serial_puts(SERIAL_COM1, " started successfully\n");
            return 0;
        }
    }

    serial_puts(SERIAL_COM1, "[SMP] WARNING: AP ");
    serial_put_dec(SERIAL_COM1, cpu_id);
    serial_puts(SERIAL_COM1, " failed to start\n");

    ap->state = CPU_STATE_ERROR;
    return -1;
}

int smp_init(void) {
    serial_puts(SERIAL_COM1, "[SMP] Initializing Symmetric Multi-Processing...\n");

    /* 清空 IPI handler 表 */
    for (int i = 0; i < IPI_MAX_TYPES; i++) {
        ipi_handlers[i] = NULL;
    }

    /* 步骤 1: 初始化 BSP */
    if (init_bsp() != 0) {
        serial_puts(SERIAL_COM1, "[SMP] ERROR: Failed to initialize BSP\n");
        return -1;
    }

    /* 步骤 2: 检测 CPU 数量 */
    int total_cpus = detect_cpus();
    serial_puts(SERIAL_COM1, "[SMP] Detected ");
    serial_put_dec(SERIAL_COM1, total_cpus);
    serial_puts(SERIAL_COM1, " CPUs total\n");

    /* 步骤 3: 启动所有 AP (跳过 BSP, 即 CPU 0) */
    int started_count = 1;  /* BSP 已经运行 */

    for (int i = 1; i < total_cpus && i < MAX_CPUS; i++) {
        if (start_ap(i) == 0) {
            started_count++;
            cpu_count++;
        }
    }

    /* 注册 IPI 处理程序到 IDT */
    idt_set_handler(0xF0, (interrupt_handler_t)smp_ipi_handler, "IPI_Interrupt");
    idt_set_handler(0xF1, (interrupt_handler_t)smp_ipi_handler, "IPI_Reschedule");
    idt_set_handler(0xF2, (interrupt_handler_t)smp_ipi_handler, "IPI_Stop");
    idt_set_handler(0xF3, (interrupt_handler_t)smp_ipi_handler, "IPI_FlushTLB");
    idt_set_handler(0xF4, (interrupt_handler_t)smp_ipi_handler, "IPI_CallFunc");

    serial_puts(SERIAL_COM1, "[SMP] Initialization complete: ");
    serial_put_dec(SERIAL_COM1, started_count);
    serial_puts(SERIAL_COM1, "/");
    serial_put_dec(SERIAL_COM1, total_cpus);
    serial_puts(SERIAL_COM1, " CPUs active\n");

    return started_count;
}

cpu_info_t* smp_get_current_cpu(void) {
    /*
     * 从 GS 寄存器读取 Per-CPU 数据
     * GS base 应该在 init_bsp() 或 AP 启动时设置
     */
    uint64_t gs_base;
    __asm__ volatile ("mov %%gs:0, %0" : "=r"(gs_base));

    if (gs_base == 0) return &cpus[0];  /* 默认返回 BSP */
    return (cpu_info_t*)gs_base;
}

cpu_info_t* smp_get_cpu(uint8_t apic_id) {
    for (int i = 0; i < cpu_count; i++) {
        if (cpus[i].apic_id == apic_id) {
            return &cpus[i];
        }
    }
    return NULL;
}

cpu_info_t* smp_get_bsp(void) {
    if (bsp_cpu_id >= 0) {
        return &cpus[bsp_cpu_id];
    }
    return NULL;
}

int smp_get_active_cpu_count(void) {
    int count = 0;
    for (int i = 0; i < cpu_count; i++) {
        if (cpus[i].state == CPU_STATE_RUNNING) {
            count++;
        }
    }
    return count;
}

int smp_send_ipi(uint8_t target_apic_id, ipi_type_t type, void *data) {
    cpu_info_t *target = smp_get_cpu(target_apic_id);
    if (target == NULL || target->state != CPU_STATE_RUNNING) {
        return -1;
    }

    cpu_info_t *sender = smp_get_current_cpu();
    sender->ipi_sent++;

    volatile uint32_t *lapic = sender->local_apic;

    /* 设置目标 */
    lapic[0x310 / sizeof(uint32_t)] = (uint32_t)target_apic_id << 24;

    /* 计算 IPI vector */
    uint32_t vector = 0xF0 + (uint32_t)type;

    /* 发送 IPI */
    lapic[0x300 / sizeof(uint32_t)] =
        0x00040000 |  /* Level, assert */
        vector;       /* Vector */

    return 0;
}

int smp_broadcast_ipi(int exclude_self, ipi_type_t type, void *data) {
    cpu_info_t *sender = smp_get_current_cpu();
    int sent = 0;

    for (int i = 0; i < cpu_count; i++) {
        if (exclude_self && (&cpus[i] == sender)) continue;

        if (smp_send_ipi(cpus[i].apic_id, type, data) == 0) {
            sent++;
        }
    }

    return (sent > 0) ? 0 : -1;
}

int smp_register_ipi_handler(ipi_type_t type, ipi_handler_t handler) {
    if (type >= IPI_MAX_TYPES || handler == NULL) return -1;

    ipi_handlers[type] = handler;

    serial_puts(SERIAL_COM1, "[SMP] Registered IPI handler for type ");
    serial_put_dec(SERIAL_COM1, type);
    serial_puts(SERIAL_COM1, "\n");

    return 0;
}

int smp_barrier_wait(uint32_t timeout_us) {
    barrier_target = smp_get_active_cpu_count();
    barrier_count = 0;

    /* 通知其他 CPU 到达同步点 */
    smp_broadcast_ipi(0, IPI_INTERRUPT, NULL);

    /* 等待所有 CPU 到达 */
    uint32_t elapsed = 0;
    while (barrier_count < barrier_target && elapsed < timeout_us) {
        for (volatile int i = 0; i < 1000; i++);  /* ~1μs delay */
        elapsed++;
    }

    if (barrier_count < barrier_target) {
        serial_puts(SERIAL_COM1, "[SMP] Barrier timeout: ");
        serial_put_dec(SERIAL_COM1, barrier_count);
        serial_puts(SERIAL_COM1, "/");
        serial_put_dec(SERIAL_COM1, barrier_target);
        serial_puts(SERIAL_COM1, " CPUs arrived\n");
        return -1;
    }

    return 0;
}

void smp_dump_status(void) {
    serial_puts(SERIAL_COM1, "\n=== SMP Status ===\n");
    serial_puts(SERIAL_COM1, "Total CPUs: ");
    serial_put_dec(SERIAL_COM1, cpu_count);
    serial_puts(SERIAL_COM1, "\n");
    serial_puts(SERIAL_COM1, "Active CPUs: ");
    serial_put_dec(SERIAL_COM1, smp_get_active_cpu_count());
    serial_puts(SERIAL_COM1, "\n\n");

    for (int i = 0; i < cpu_count; i++) {
        serial_puts(SERIAL_COM1, "CPU ");
        serial_put_dec(SERIAL_COM1, i);
        serial_puts(SERIAL_COM1, ": ");

        switch (cpus[i].state) {
            case CPU_STATE_UNINITIALIZED:
                serial_puts(SERIAL_COM1, "Uninitialized"); break;
            case CPU_STATE_BOOTING:
                serial_puts(SERIAL_COM1, "Booting"); break;
            case CPU_STATE_RUNNING:
                serial_puts(SERIAL_COM1, "Running"); break;
            case CPU_STATE_HALTED:
                serial_puts(SERIAL_COM1, "Halted"); break;
            case CPU_STATE_ERROR:
                serial_puts(SERIAL_COM1, "Error"); break;
            default:
                serial_puts(SERIAL_COM1, "Unknown"); break;
        }

        serial_puts(SERIAL_COM1, " [APIC=");
        serial_put_dec(SERIAL_COM1, cpus[i].apic_id);

        if (cpus[i].is_bsp) {
            serial_puts(SERIAL_COM1, ", BSP");
        }

        serial_puts(SERIAL_COM1, "] [IRQs=");
        serial_put_dec(SERIAL_COM1, cpus[i].interrupts_total);
        serial_puts(SERIAL_COM1, ", IPI_rx=");
        serial_put_dec(SERIAL_COM1, cpus[i].ipi_received);
        serial_puts(SERIAL_COM1, ", IPI_tx=");
        serial_put_dec(SERIAL_COM1, cpus[i].ipi_sent);
        serial_puts(SERIAL_COM1, "]\n");
    }

    serial_puts(SERIAL_COM1, "==================\n");
}

int smp_stop_cpu(uint8_t apic_id) {
    return smp_send_ipi(apic_id, IPI_STOP, NULL);
}

int smp_restart_cpu(uint8_t apic_id) {
    cpu_info_t *cpu = smp_get_cpu(apic_id);
    if (cpu == NULL) return -1;

    /* 先停止，再重新启动 */
    if (smp_stop_cpu(apic_id) != 0) return -1;

    /* 等待停止 */
    for (int i = 0; i < 1000000; i++) {
        if (cpu->state == CPU_STATE_HALTED) break;
    }

    /* 重新发送 SIPI */
    send_startup_ipi(apic_id, AP_BOOT_ADDR >> 12);

    return 0;
}

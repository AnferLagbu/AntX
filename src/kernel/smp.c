#include "smp.h"
#include "klog.h"
#include "idt.h"
#include "io.h"
#include "string.h"
#include <stdint.h>
#include "kmalloc.h"

static cpu_info_t cpus[MAX_CPUS];
static int cpu_count = 0;
static int bsp_cpu_id = -1;

static ipi_handler_t ipi_handlers[IPI_MAX_TYPES];

static volatile int barrier_count = 0;
static volatile int barrier_target = 0;

__attribute__((aligned(4096)))
static uint8_t ap_boot_code[256] = {
    0xFA,                           /* CLI */
    0xB8, 0x00, 0xA0, 0x00, 0x00,  /* MOV AX, 0xA000 */
    0x8E, 0xD8,                    /* MOV DS, AX */
    0x8E, 0xC0,                    /* MOV ES, AX */
    0x8E, 0xD0,                    /* MOV SS, AX */
    0xBC, 0x00, 0x7C, 0x00, 0x00,  /* MOV SP, 0x7C00 */
    0xEA, 0x00, 0x00, 0x80, 0x00, 0x00,  /* JMP FAR 0x80:0000 (long mode entry) */
    /* 剩余填充为 0 */
};

static inline uint64_t read_msr(uint32_t msr) {
    uint32_t low, high;
    __asm__ volatile (
        "rdmsr"
        : "=a"(low), "=d"(high)
        : "c"(msr)
    );
    return ((uint64_t)high << 32) | low;
}

static inline void write_msr(uint32_t msr, uint64_t value) {
    uint32_t low = value & 0xFFFFFFFF;
    uint32_t high = value >> 32;
    __asm__ volatile (
        "wrmsr"
        :
        : "c"(msr), "a"(low), "d"(high)
    );
}

static inline void write_gs_base(uint64_t base) {
    /* MSR 0xC0000101 - GS Base */
    write_msr(0xC0000101, base);
}

static int init_local_apic(cpu_info_t *cpu) {
    uint32_t apic_base_msr = read_msr(0x1B);

    cpu->apic_base = apic_base_msr & 0xFFFFF000;
    cpu->is_bsp = (apic_base_msr >> 8) & 1;
    cpu->apic_id = ((apic_base_msr >> 24) & 0xFF);

    extern int vmm_map_page(uint64_t virt, uint64_t phys, uint64_t flags);
    uint64_t apic_virt = 0xFFFFF80000000000ULL + (uint64_t)cpu->apic_base;

    if (vmm_map_page(apic_virt, cpu->apic_base, 0x03) != 0) {
        klog_kern_err("SMP: Failed to map APIC for CPU %d", cpu->apic_id);
        return -1;
    }

    cpu->local_apic = (volatile uint32_t *)apic_virt;

    uint32_t version = cpu->local_apic[0x30 / sizeof(uint32_t)];
    if ((version & 0xFF) == 0) {
        klog_kern_warn("SMP: No APIC detected for CPU %d", cpu->apic_id);
        return -1;
    }

    uint32_t svr = cpu->local_apic[0xF0 / sizeof(uint32_t)];
    svr |= 0x100;
    cpu->local_apic[0xF0 / sizeof(uint32_t)] = svr;

    klog_kern("SMP: Local APIC initialized for CPU %d at 0x%x", cpu->apic_id, cpu->apic_base);

    return 0;
}

void lapic_send_eoi(void) {
    cpu_info_t *cpu = smp_get_current_cpu();
    if (cpu && cpu->local_apic) {
        cpu->local_apic[0xB0 / sizeof(uint32_t)] = 0;
    }
}

static void* allocate_ap_stack(int cpu_id) {
    extern void* kmalloc(uint64_t size);

    void *stack = kmalloc(AP_STACK_SIZE);
    if (stack == NULL) {
        klog_kern_err("SMP: Failed to allocate stack for CPU %d", cpu_id);
        return NULL;
    }
    
    memset(stack, 0, AP_STACK_SIZE);

    return (void*)((uint8_t*)stack + AP_STACK_SIZE);
}

static void send_init_ipi(uint8_t target_apic_id) {
    volatile uint32_t *lapic = cpus[0].local_apic;

    lapic[0x310 / sizeof(uint32_t)] = (uint32_t)target_apic_id << 24;
    lapic[0x300 / sizeof(uint32_t)] = 0x000C5000;

    for (volatile int i = 0; i < 1000000; i++);
}

static void send_startup_ipi(uint8_t target_apic_id, uint8_t page_num) {
    volatile uint32_t *lapic = cpus[0].local_apic;

    lapic[0x310 / sizeof(uint32_t)] = (uint32_t)target_apic_id << 24;
    lapic[0x300 / sizeof(uint32_t)] =
        0x00060600 |
        (page_num & 0xFF);
}

__attribute__((noreturn))
static void ap_entry_long_mode(void) {
    while (1) {
        __asm__ volatile ("hlt");
    }
}

void smp_ap_ready(void) {
    cpu_info_t *current = smp_get_current_cpu();
    if (current == NULL) return;

    current->state = CPU_STATE_RUNNING;

    klog_kern("SMP: CPU %d (APIC %d) is now RUNNING", current->cpu_id, current->apic_id);
}

void smp_ipi_handler(struct interrupt_frame *frame) {
    cpu_info_t *current = smp_get_current_cpu();
    if (current == NULL) return;

    current->ipi_received++;

    ipi_type_t type = (ipi_type_t)(frame->int_no - 0xF0);

    if (type >= 0 && type < IPI_MAX_TYPES && ipi_handlers[type] != NULL) {
        ipi_handlers[type](current, NULL);
    } else {
        klog_kern_warn("SMP: Unknown IPI type: %d", type);
    }
}

static int init_bsp(void) {
    cpu_info_t *bsp = &cpus[0];

    __builtin_memset(bsp, 0, sizeof(cpu_info_t));

    bsp->cpu_id = 0;
    bsp->state = CPU_STATE_RUNNING;

    if (init_local_apic(bsp) != 0) {
        return -1;
    }

    bsp->is_bsp = 1;
    bsp_cpu_id = 0;

    bsp->kernel_stack = allocate_ap_stack(0);
    if (bsp->kernel_stack == NULL) {
        return -1;
    }

    write_gs_base((uint64_t)bsp);

    klog_kern("SMP: BSP initialized: APIC ID=%d, CPU ID=0", bsp->apic_id);

    cpu_count = 1;
    return 0;
}

static int detect_cpus(void) {
    #ifdef CONFIG_SMP_MAX_CPUS
    return CONFIG_SMP_MAX_CPUS;
    #else
    return 4;
    #endif
}

static int start_ap(int cpu_id) {
    if (cpu_id <= 0 || cpu_id >= MAX_CPUS) return -1;

    cpu_info_t *ap = &cpus[cpu_id];
    __builtin_memset(ap, 0, sizeof(cpu_info_t));

    ap->cpu_id = cpu_id;
    ap->state = CPU_STATE_BOOTING;

    ap->apic_id = (uint8_t)(cpu_id);

    ap->kernel_stack = allocate_ap_stack(cpu_id);
    if (ap->kernel_stack == NULL) {
        ap->state = CPU_STATE_ERROR;
        return -1;
    }

    extern int vmm_map_page(uint64_t virt, uint64_t phys, uint64_t flags);
    vmm_map_page(AP_BOOT_ADDR, AP_BOOT_ADDR, 0x03);

    __builtin_memcpy((void*)AP_BOOT_ADDR, ap_boot_code, sizeof(ap_boot_code));

    send_init_ipi(ap->apic_id);

    for (volatile int i = 0; i < 10000000; i++);

    send_startup_ipi(ap->apic_id, AP_BOOT_ADDR >> 12);

    for (volatile int i = 0; i < 200000; i++);

    send_startup_ipi(ap->apic_id, AP_BOOT_ADDR >> 12);

    for (int timeout = 0; timeout < 1000000; timeout++) {
        if (ap->state == CPU_STATE_RUNNING) {
            klog_kern("SMP: AP %d started successfully", cpu_id);
            return 0;
        }
    }

    klog_kern_warn("SMP: AP %d failed to start", cpu_id);

    ap->state = CPU_STATE_ERROR;
    return -1;
}

int smp_init(void) {
    klog_kern("Initializing Symmetric Multi-Processing...");

    for (int i = 0; i < IPI_MAX_TYPES; i++) {
        ipi_handlers[i] = NULL;
    }

    if (init_bsp() != 0) {
        klog_kern_err("SMP: Failed to initialize BSP");
        return -1;
    }

    int total_cpus = detect_cpus();
    klog_kern("SMP: Detected %d CPUs total", total_cpus);

    int started_count = 1;

    for (int i = 1; i < total_cpus && i < MAX_CPUS; i++) {
        if (start_ap(i) == 0) {
            started_count++;
            cpu_count++;
        }
    }

    idt_set_handler(0xF0, (interrupt_handler_t)smp_ipi_handler, "IPI_Interrupt");
    idt_set_handler(0xF1, (interrupt_handler_t)smp_ipi_handler, "IPI_Reschedule");
    idt_set_handler(0xF2, (interrupt_handler_t)smp_ipi_handler, "IPI_Stop");
    idt_set_handler(0xF3, (interrupt_handler_t)smp_ipi_handler, "IPI_FlushTLB");
    idt_set_handler(0xF4, (interrupt_handler_t)smp_ipi_handler, "IPI_CallFunc");

    klog_kern("SMP: Initialization complete: %d/%d CPUs active", started_count, total_cpus);

    /* 初始化 Per-CPU 运行队列 */
    smp_init_runqueues();

    return started_count;
}

cpu_info_t* smp_get_current_cpu(void) {
    uint64_t gs_base;
    __asm__ volatile ("mov %%gs:0, %0" : "=r"(gs_base));

    if (gs_base == 0) return &cpus[0];
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

    lapic[0x310 / sizeof(uint32_t)] = (uint32_t)target_apic_id << 24;

    uint32_t vector = 0xF0 + (uint32_t)type;

    lapic[0x300 / sizeof(uint32_t)] =
        0x00040000 |
        vector;

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

    klog_kern("SMP: Registered IPI handler for type %d", type);

    return 0;
}

int smp_barrier_wait(uint32_t timeout_us) {
    barrier_target = smp_get_active_cpu_count();
    barrier_count = 0;

    smp_broadcast_ipi(0, IPI_INTERRUPT, NULL);

    uint32_t elapsed = 0;
    while (barrier_count < barrier_target && elapsed < timeout_us) {
        for (volatile int i = 0; i < 1000; i++);
        elapsed++;
    }

    if (barrier_count < barrier_target) {
        klog_kern_warn("SMP: Barrier timeout: %d/%d CPUs arrived", barrier_count, barrier_target);
        return -1;
    }

    return 0;
}

void smp_dump_status(void) {
    klog_kern("=== SMP Status ===");
    klog_kern("Total CPUs: %d", cpu_count);
    klog_kern("Active CPUs: %d", smp_get_active_cpu_count());
    klog_kern("Total Load: %d", smp_get_total_load());

    for (int i = 0; i < cpu_count; i++) {
        const char *state_str;
        switch (cpus[i].state) {
            case CPU_STATE_UNINITIALIZED: state_str = "Uninitialized"; break;
            case CPU_STATE_BOOTING:       state_str = "Booting"; break;
            case CPU_STATE_RUNNING:       state_str = "Running"; break;
            case CPU_STATE_HALTED:        state_str = "Halted"; break;
            case CPU_STATE_ERROR:         state_str = "Error"; break;
            default:                      state_str = "Unknown"; break;
        }

        klog_kern("CPU %d: %s [APIC=%d%s] [IRQs=%d, IPI_rx=%d, IPI_tx=%d]",
                  i, state_str, cpus[i].apic_id,
                  cpus[i].is_bsp ? ", BSP" : "",
                  cpus[i].interrupts_total,
                  cpus[i].ipi_received,
                  cpus[i].ipi_sent);
        
        /* 显示运行队列信息 */
        if (cpus[i].state == CPU_STATE_RUNNING) {
            klog_kern("  └─ RQ: runnable=%d load=%d last_bal=%d",
                      cpus[i].runqueue.runnable_count,
                      cpus[i].runqueue.total_load,
                      (uint32_t)cpus[i].runqueue.last_balance_tick);
        }
    }
}

int smp_stop_cpu(uint8_t apic_id) {
    return smp_send_ipi(apic_id, IPI_STOP, NULL);
}

int smp_restart_cpu(uint8_t apic_id) {
    cpu_info_t *cpu = smp_get_cpu(apic_id);
    if (cpu == NULL) return -1;

    if (smp_stop_cpu(apic_id) != 0) return -1;

    for (int i = 0; i < 1000000; i++) {
        if (cpu->state == CPU_STATE_HALTED) break;
    }

    send_startup_ipi(apic_id, AP_BOOT_ADDR >> 12);

    return 0;
}

/* ==================== Per-CPU 调度实现 ==================== */

void smp_init_runqueues(void) {
    for (int i = 0; i < MAX_CPUS; i++) {
        cpus[i].runqueue.runnable_count = 0;
        cpus[i].runqueue.total_load = 0;
        cpus[i].runqueue.last_balance_tick = 0;
        cpus[i].runqueue.need_reschedule = 0;
    }
    
    klog_kern("SMP: Per-CPU runqueues initialized for %d CPUs", cpu_count);
}

per_cpu_rq_t* smp_get_runqueue(int cpu_id) {
    if (cpu_id < 0 || cpu_id >= MAX_CPUS) {
        return NULL;
    }
    return &cpus[cpu_id].runqueue;
}

void smp_add_load(int cpu_id, uint32_t load) {
    if (cpu_id < 0 || cpu_id >= MAX_CPUS) return;
    
    cpus[cpu_id].runqueue.runnable_count++;
    cpus[cpu_id].runqueue.total_load += load;
}

void smp_remove_load(int cpu_id, uint32_t load) {
    if (cpu_id < 0 || cpu_id >= MAX_CPUS) return;
    
    if (cpus[cpu_id].runqueue.runnable_count > 0) {
        cpus[cpu_id].runqueue.runnable_count--;
    }
    if (cpus[cpu_id].runqueue.total_load >= load) {
        cpus[cpu_id].runqueue.total_load -= load;
    } else {
        cpus[cpu_id].runqueue.total_load = 0;
    }
}

static int smp_calc_imbalance(int src_cpu, int dst_cpu) {
    if (src_cpu < 0 || dst_cpu >= MAX_CPUS) return 0;
    if (dst_cpu < 0 || src_cpu >= MAX_CPUS) return 0;
    
    int src_load = (int)cpus[src_cpu].runqueue.runnable_count;
    int dst_load = (int)cpus[dst_cpu].runqueue.runnable_count;
    
    return src_load - dst_load;
}

int smp_try_balance_load(uint64_t current_tick) {
    if (cpu_count <= 1) return 0;
    
    cpu_info_t *current = smp_get_current_cpu();
    if (current == NULL) return 0;
    
    /* 检查是否到了负载均衡时间 */
    if (current_tick - current->runqueue.last_balance_tick < LOAD_BALANCE_INTERVAL) {
        return 0;
    }
    
    current->runqueue.last_balance_tick = current_tick;
    
    int migrations = 0;
    int my_cpu = current->cpu_id;
    int my_load = (int)current->runqueue.runnable_count;
    
    /* 查找最忙和最空闲的 CPU */
    int busiest_cpu = my_cpu;
    int idlest_cpu = my_cpu;
    int max_load = my_load;
    int min_load = my_load;
    
    for (int i = 0; i < cpu_count; i++) {
        if (cpus[i].state != CPU_STATE_RUNNING) continue;
        
        int load = (int)cpus[i].runqueue.runnable_count;
        if (load > max_load) {
            max_load = load;
            busiest_cpu = i;
        }
        if (load < min_load) {
            min_load = load;
            idlest_cpu = i;
        }
    }
    
    /* 如果负载差超过阈值，触发迁移 */
    int imbalance = smp_calc_imbalance(busiest_cpu, idlest_cpu);
    
    if (imbalance > LOAD_BALANCE_THRESHOLD && migrations < MAX_MIGRATION_PER_CYCLE) {
        /* 从最忙的CPU迁移到最空闲的CPU */
        smp_remove_load(busiest_cpu, 1);
        smp_add_load(idlest_cpu, 1);
        migrations++;
        
        klog_kern("SMP: Load balance: CPU%d(%d)->CPU%d(%d)", 
                  busiest_cpu, max_load-1, idlest_cpu, min_load+1);
    }
    
    return migrations;
}

int smp_set_affinity(uint32_t pid, uint64_t cpu_mask) {
    (void)pid;
    (void)cpu_mask;
    
    /* TODO: 完整实现需要与调度器集成 */
    klog_kern("SMP: Set affinity pid=%d mask=0x%x (stub)", pid, (uint32_t)cpu_mask);
    
    return 0;
}

int smp_find_idlest_cpu(void) {
    int idlest = -1;
    uint32_t min_load = 0xFFFFFFFF;  /* UINT32_MAX */
    
    for (int i = 0; i < cpu_count; i++) {
        if (cpus[i].state != CPU_STATE_RUNNING) continue;
        
        if (cpus[i].runqueue.runnable_count < min_load) {
            min_load = cpus[i].runqueue.runnable_count;
            idlest = i;
        }
    }
    
    return idlest;
}

uint32_t smp_get_total_load(void) {
    uint32_t total = 0;
    
    for (int i = 0; i < cpu_count; i++) {
        total += cpus[i].runqueue.total_load;
    }
    
    return total;
}

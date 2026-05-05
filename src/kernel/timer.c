#include "timer.h"
#include "idt.h"
#include "proc.h"
#include "klog.h"
#include "pwid.h"

#define PIT_CHANNEL0    0x40
#define PIT_COMMAND     0x43

#define PIT_FREQ        1193182
#define TIMER_HZ        100

#define PWID_CLEANUP_INTERVAL 100

static uint64_t timer_ticks = 0;

/* lwIP 时间 hooks (定义于 sys_arch.c) */
extern void sys_tick_inc(void);
extern void sys_check_timeouts(void);

static void timer_handler(struct interrupt_frame *frame) {
    (void)frame;
    timer_ticks++;
    scheduler_tick();

    /* lwIP 时钟: 每次 tick (10ms) 推进 */
    sys_tick_inc();
    sys_check_timeouts();
    
    if (timer_ticks % PWID_CLEANUP_INTERVAL == 0) {
        pwid_cleanup_internal();
    }
}

void timer_init(void) {
    uint32_t divisor = PIT_FREQ / TIMER_HZ;
    
    outb(PIT_COMMAND, 0x36);
    
    outb(PIT_CHANNEL0, divisor & 0xFF);
    outb(PIT_CHANNEL0, (divisor >> 8) & 0xFF);
    
    idt_register_irq(0, timer_handler, "timer", 0);
    
    klog_kern("Timer initialized (100 Hz)");
}

uint64_t timer_get_ticks(void) {
    return timer_ticks;
}

void timer_sleep(uint64_t ms) {
    uint64_t target = timer_ticks + (ms * TIMER_HZ) / 1000;
    while (timer_ticks < target) {
        __asm__ volatile ("hlt");
    }
}

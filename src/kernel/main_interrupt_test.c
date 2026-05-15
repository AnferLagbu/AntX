/* Interrupt test main stub */
void kernel_init(void);
void klog_init(void);

void _start(void) {
    klog_init();
    kernel_init();
    while (1) { __asm__ volatile("hlt"); }
}

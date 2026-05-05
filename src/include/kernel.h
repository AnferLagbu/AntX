#ifndef _KERNEL_H
#define _KERNEL_H

#include "types.h"
#include "io.h"
#include "gdt.h"
#include "idt.h"
#include "mm.h"
#include "proc.h"
#include "string.h"
#include "klog.h"
#include "assert.h"

#define KERNEL_NAME    "QueenX"
#include "version_auto.h"

#define MEMORY_SIZE    (512 * 1024 * 1024)

void kernel_main(void);
void panic(const char *msg);

void enable_interrupts(void);
void disable_interrupts(void);
void interrupt_idle(void);

#endif

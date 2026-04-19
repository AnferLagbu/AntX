#ifndef _KERNEL_H
#define _KERNEL_H

#include "types.h"
#include "io.h"
#include "serial.h"
#include "gdt.h"
#include "idt.h"
#include "mm.h"
#include "proc.h"
#include "pwid.h"
#include "hvfs.h"
#include "syscall.h"
#include "keyboard.h"
#include "string.h"
#include "printk.h"
#include "ata.h"
#include "assert.h"

#define KERNEL_NAME    "QueenX"
#define KERNEL_VERSION "0.1.0"

#define MEMORY_SIZE    (128 * 1024 * 1024)

void kernel_main(void);
void panic(const char *msg);

void enable_interrupts(void);
void disable_interrupts(void);
void interrupt_idle(void);

#endif

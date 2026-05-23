// Minimal AArch64 user-space init program
// Just loops, exercising EL0 execution

.section .text._start
.globl _start
.type _start, @function

_start:
    // Simple counter loop to consume some cycles
    mov x0, #0
    ldr x1, =1000000
1:  add x0, x0, #1
    cmp x0, x1
    b.lt 1b

    // Yield via SVC
    mov x0, #0
2:  svc #0
    b 2b

.section .note.GNU-stack,"",%progbits
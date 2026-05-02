# QueenX Operating System - Code Style Guide

## 📋 Overview

This document defines the unified coding standards for the QueenX operating system project. All contributors MUST follow these guidelines to ensure code consistency and maintainability.

**Version**: 1.0  
**Last Updated**: 2026-05-02  
**Language Support**: C (kernel), Rust (security-critical), Assembly (boot)

---

## 🎯 Naming Conventions

### **C Language (Kernel & Drivers)**

#### **Functions: `snake_case`**
```c
// ✅ Correct
void idt_init(void);
int pwid_elevate(uint64_t target, const char *password);
uint64_t pmm_alloc_pages(uint64_t count);

// ❌ Wrong
void IdtInit(void);           // camelCase
int PwidElevate(...);        // PascalCase
```

#### **Variables: `snake_case`**
```c
// ✅ Correct
uint64_t nested_interrupt_count;
struct idt_entry idt[IDT_ENTRIES];
int current_process_id;

// ❌ Wrong
uint64_t NestedInterruptCount;  // camelCase
int CurrentProcessID;            // PascalCase
```

#### **Constants/Macros: `UPPER_SNAKE_CASE`**
```c
// ✅ Correct
#define IDT_ENTRIES 256
#define IRQ_BASE 32
#define MAX_PATH_LENGTH 4096

// ❌ Wrong
#define IdtEntries 256          // camelCase
#define max_path_length 4096    // lowercase
```

#### **Structs/Types: `PascalCase` with `_t` suffix**
```c
// ✅ Correct
typedef struct interrupt_frame InterruptFrame;  // or struct interrupt_frame
typedef uint64_t pwid_t;
struct process { ... };

// ❌ Wrong
typedef struct interrupt_frame interrupt_frame_t;  // redundant _t
struct Process { ... };                            // inconsistent with kernel style
```

#### **Enums: `UPPER_SNAKE_CASE` values, `PascalCase` type**
```c
// ✅ Correct
typedef enum {
    PWID_LEVEL_ROOT = 0,
    PWID_LEVEL_TRUSTWORTHY = 1,
    PWID_LEVEL_UNTRUSTWORTHY = 2
} PwidLevel;

// ❌ Wrong
enum pwid_level { root, trustworthy, untrustworthy };  // lowercase values
```

---

### **Rust Language (Security-Critical Components)**

#### **Functions/Methods: `snake_case`**
```rust
// ✅ Correct
pub fn find_mount(&self, path: &str) -> Option<usize> {
pub fn write_superblock_to_disk(&self) -> i32 {

// ❌ Wrong
pub fn FindMount(...) {    // PascalCase
pub fn WriteSuperBlock() { // camelCase
```

#### **Variables: `snake_case`**
```rust
// ✅ Correct
let mount_idx: Option<usize> = None;
let best_len = 0usize;
let mut bytes_written = 0usize;

// ❌ Wrong
let MountIdx: Option<usize> = None;   // PascalCase
let BestLen = 0usize;                  // camelCase
```

#### **Structs/Enums/Types: `PascalCase`**
```rust
// ✅ Correct
pub struct HvFsData { ... }
pub enum VfsFileType { ... }
pub type PwidToken = u64;

// ❌ Wrong
pub struct hvfs_data { ... }     // snake_case
pub enum vfs_file_type { ... }   // snake_case
```

#### **Constants: `UPPER_SNAKE_CASE` or `SCREAMING_SNAKE_CASE`**
```rust
// ✅ Correct
const HVFS_MAGIC: u32 = 0x48565F53;
const RAMFS_MAX_BLOCKS: u32 = 1024;
static mut GLOBAL_COUNTER: u64 = 0;

// ❌ Wrong
const HvfsMagic: u32 = ...      // camelCase
const ramfs_max_blocks: u32 = ...  // lowercase
```

#### **Modules/Files: `snake_case`**
```rust
// ✅ Correct (file: src/fs/vfs/vfs.rs)
mod vfs { ... }

// File names: mod.rs, hvfs.rs, diskfs.rs (all lowercase)
```

---

### **Assembly Language**

#### **Labels: `snake_case`**
```asm
; ✅ Correct
isr_common_stub:
irq_common_stub:
idt_flush:

; ❌ Wrong
IsrCommonStub:
IRQ_CommonStub:
```

#### **Comments: Clear and descriptive**
```asm
; Save registers on stack
push rbp
mov rbp, rsp

; Call C handler
call exception_handler
```

---

## 📐 Code Formatting

### **Indentation**
- **Width**: 4 spaces (NO tabs)
- **Continuation lines**: 8 spaces (or align with opening parenthesis)

```c
// ✅ Correct - 4 space indent
if (condition) {
    do_something();
    if (nested) {
        do_nested();
    }
}

// ✅ Correct - continuation alignment
long_function_name(parameter_one,
                    parameter_two,
                    parameter_three);
```

### **Braces Style: K&R (Opening brace on same line)**
```c
// ✅ Correct - K&R style
if (condition) {
    statement;
} else {
    other_statement;
}

// ❌ Wrong - Allman style (opening brace on new line)
if (condition)
{
    statement;
}
```

**Exception**: Function definitions (use K&R for consistency)
```c
// ✅ Correct
int function_name(int param) {
    return param + 1;
}
```

### **Line Length: Maximum 100 characters**
```c
// ✅ Correct - under 100 chars
if (very_long_condition && another_long_condition && yet_another) {
    break;
}

// ✅ Correct - split long lines
result = some_very_long_function_name(first_parameter,
                                    second_parameter,
                                    third_parameter);
```

### **Blank Lines**
- **Between functions**: 2 blank lines
- **Between logical sections**: 1 blank line
- **Inside functions**: Use sparingly to group related statements

```c
// ✅ Correct spacing
void function_one(void) {
    /* implementation */
}


void function_two(void) {
    /* implementation */
}
```

---

## 💬 Comments Style

### **C Language: Doxygen-style for public APIs**
```c
/**
 * @brief Brief description of function
 *
 * Detailed description if needed.
 * Can span multiple lines.
 *
 * @param param1 Description of first parameter
 * @param param2 Description of second parameter
 * @return Description of return value
 * @retval 0 Success
 * @retval -1 Error occurred
 *
 * @note Important usage notes
 * @warning Potential pitfalls
 * @see related_function()
 */
int example_function(int param1, char *param2);
```

### **Inline Comments: `//` for single-line, `/* */` for multi-line**
```c
// Single-line comment (preferred)
/* Legacy multi-line comment */
```

### **TODO/FIXME/HACK Tags**
```c
// TODO(username): Implement feature X by YYYY-MM-DD
// FIXME: This has a race condition - need mutex
// HACK: Workaround for hardware bug XYZ
// NOTE: Performance critical path - optimize later
// SAFETY: Caller must hold lock before calling this
```

### **Rust Language: `///` for doc comments, `//` for regular**
```rust
/// Brief description of function.
///
/// # Examples
///
/// ```
/// let result = function_call();
/// assert!(result.is_ok());
/// ```
///
/// # Arguments
///
/// * `param1` - Description
///
/// # Returns
///
/// * `Ok(value)` - On success
/// * `Err(e)` - On error
pub fn documented_function(param1: Type) -> Result<Type, Error> {
    // Regular comment
    unimplemented!();
}
```

---

## 📁 File Organization

### **Header Files (.h)**

**Structure** (in order):
1. License header (if applicable)
2. `#ifndef` / `#define` include guard (or `#pragma once`)
3. Includes (system headers first, then project headers)
4. Macro definitions (`#define`)
5. Type definitions (`typedef`, `struct`, `enum`)
5. Global variable declarations (`extern`)
6. Function prototypes

**Example**:
```c
/**
 * @file filename.h
 * @brief One-line description of module
 */

#ifndef FILENAME_H
#define FILENAME_H

#include "types.h"
#include <stdint.h>

/* ============================================================
 * Constants and Macros
 * ============================================================ */
#define MAX_BUFFER_SIZE 4096

/* ============================================================
 * Type Definitions
 * ============================================================ */
struct example_struct {
    int field1;
    char field2[256];
};

/* ============================================================
 * Function Prototypes
 * ============================================================ */
int init_module(void);
void cleanup_module(void);

#endif /* FILENAME_H */
```

### **Source Files (.c)**

**Structure** (in order):
1. License header
2. Includes (corresponding .h file first)
3. Private macros/constants
4. Private type definitions
5. Static/global variables
6. Static helper functions
8. Public API implementations

**Example**:
```c
/**
 * @file filename.c
 * @brief Implementation of module functionality
 */

#include "filename.h"
#include "other_header.h"

/* ============================================================
 * Private Constants
 * ============================================================ */
static const int DEFAULT_TIMEOUT = 30;

/* ============================================================
 * Private Functions
 * ============================================================ */
static int helper_function(int x) {
    return x * 2;
}

/* ============================================================
 * Public API Implementation
 * ============================================================ */
int init_module(void) {
    return 0;
}
```

### **Rust Modules (mod.rs)**

**Structure**:
1. Module documentation (`//!`)
2. Re-exports (`pub use`)
3. Public types
4. Public traits/implementations
5. Private helpers (if any)

**Example**:
```rust
//! Module description.

pub use self::internal::InternalType;

pub mod internal;

pub struct PublicType { ... }

impl PublicType {
    pub fn new() -> Self { ... }
}
```

---

## 🔀 Specific Guidelines

### **Error Handling in C**
```c
// ✅ Use consistent error codes
#define ERROR_NONE       0
#define ERROR_INVALID   -1
#define ERROR_NO_MEMORY -2
#define ERROR_NOT_FOUND -3

// Return negative on error, 0 or positive on success
int operation(int input) {
    if (input < 0) {
        return ERROR_INVALID;
    }
    return 0;  // SUCCESS
}

// Always check return values
int result = dangerous_operation();
if (result != ERROR_NONE) {
    log_error("Operation failed: %d", result);
    return result;
}
```

### **Memory Management in C**
```c
// ✅ Initialize all variables before use
char buffer[256] = {0};
int count = 0;

// ✅ Check for NULL after allocation
void *ptr = malloc(size);
if (ptr == NULL) {
    return ERROR_NO_MEMORY;
}

// ✅ Free memory when done
free(ptr);
ptr = NULL;  // Prevent use-after-free
```

### **Logging in Kernel**
```c
// ✅ Use serial output consistently
serial_puts(SERIAL_COM1, "[MODULE] Message\n");
serial_put_hex(SERIAL_COM1, value);
serial_put_dec(SERIAL_COM1, count);

// Log levels (optional enhancement)
#define LOG_DEBUG   "[DEBUG] "
#define LOG_INFO    "[INFO]  "
#define LOG_WARNING "[WARN]  "
#define LOG_ERROR   "[ERROR] "

serial_puts(SERIAL_COM1, LOG_ERROR "Critical failure\n");
```

---

## ⚠️ Common Pitfalls to Avoid

### **1. Inconsistent Naming**
```c
// ❌ BAD - Mixed conventions in same module
void InitSystem(void);         // PascalCase
int get_process_count(void);   // snake_case
#define maxProcesses 100;       // camelCase

// ✅ GOOD - Consistent throughout
void system_init(void);        // snake_case
int process_get_count(void);   // snake_case
#define MAX_PROCESSES 100;      // UPPER_SNAKE_CASE
```

### **2. Magic Numbers**
```c
// ❌ BAD - Unclear meaning
if (size > 512) { ... }
sleep(3000);  // What unit? seconds? milliseconds?

// ✅ GOOD - Named constants
#define SECTOR_SIZE 512
#define TIMEOUT_MS 3000

if (size > SECTOR_SIZE) { ... }
sleep_ms(TIMEOUT_MS);
```

### **3. Deep Nesting**
```c
// ❌ BAD - Hard to read
if (a) {
    if (b) {
        if (c) {
            do_something();
        }
    }
}

// ✅ GOOD - Early returns reduce nesting
if (!a || !b || !c) {
    return ERROR_INVALID;
}
do_something();
```

---

## 🛠️ Tools Configuration

### **EditorConfig (.editorconfig)**
See `.editorconfig` in project root for automated formatting rules.

### **Recommended VS Code Extensions**
- C/C++ IntelliSense
- rust-analyzer
- EditorConfig for VS Code
- Better TOML (for Cargo.toml)

### **Clang Format (Optional)**
Create `.clang-format` for automatic C code formatting:
```yaml
BasedOnStyle: LLVM
IndentWidth: 4
TabWidth: 4
UseTab: Never
ColumnLimit: 100
BreakBeforeBraces: Attach
AllowShortFunctionsOnASingleLine: None
AllowShortIfStatementsOnASingleLine: false
SpaceAfterCStyleCast: true
```

---

## ✅ Checklist Before Committing

- [ ] All functions/variables follow naming convention (snake_case for C/Rust)
- [ ] All constants/macros are UPPER_SNAKE_CASE
- [ ] All structs/types use PascalCase
- [ ] Indentation is 4 spaces (no tabs)
- [ ] Line length ≤ 100 characters
- [ ] Braces use K&R style
- [ ] No trailing whitespace
- [ ] File ends with newline
- [ ] Public APIs have documentation comments
- [ ] No TODO/FIXME left without issue reference
- [ ] Code compiles without warnings (or warnings are justified)

---

## 📚 References

- [Linux Kernel Coding Style](https://www.kernel.org/doc/html/latest/process/coding-style.html)
- [Rust API Guidelines](https://rust-lang.github.io/api-guidelines/)
- [Google C++ Style Guide](https://google.github.io/styleguide/cppguide/) (adapted for C)

---

**Maintainers**: QueenX Development Team  
**Review Cycle**: Quarterly  
**Last Review**: 2026-05-02

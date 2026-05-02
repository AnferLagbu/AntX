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

## 📝 Git Commit 规范

### **Commit 前缀格式**

项目使用统一的 Commit 前缀，所有提交信息**必须使用中文**：

| 前缀 | 含义 | 使用场景 |
|------|------|---------|
| `fix:` | 修复 Bug | 修复已知问题、解决崩溃、消除异常 |
| `feat:` | 新增/增强功能 | 添加新特性、扩展现有功能、实现新模块 |
| `docs:` | 文档相关 | 更新文档、添加注释、改进说明 |
| `chore:` | 构建/工具相关 | 构建配置修改、依赖管理、脚本更新 |
| `refactor:` | 重构代码 | 优化代码结构、改善设计模式、提升可读性 |
| `test:` | 测试相关 | 添加测试用例、修改测试逻辑、增强覆盖率 |
| `perf:` | 性能优化 | 提升运行性能、减少延迟、优化资源使用 |

### **Commit 信息格式**

```
<前缀>: <简短描述（中文）>

<详细说明（可选）>

主要改动：
- 改动点 1
- 改动点 2
- 改动点 3

影响范围：
- 模块 A
- 模块 B

测试结果：
- 通过: X/Y
- 失败: Z
```

### **示例**

```bash
# ✅ 好的 Commit
fix: 修复VFS FFI层Invalid Opcode异常

主要修复：
- 添加 vfs_unlink_internal() 函数实现
- 修复 test_interrupt.c 中断处理函数指针
- 解决 Scheduler Enhanced 测试失败问题

影响范围：
- VFS FFI 层 (ffi.rs)
- 测试框架 (test_*.c)

测试结果：通过 80/84 (95.2%)

# ❌ 不好的 Commit
Fix bug
update code
test fix
```

### **Commit 最佳实践**

1. **使用中文**：所有 commit 信息必须使用中文
2. **简明扼要**：标题不超过 50 个字符
3. **详细说明**：复杂改动需要在正文中详细说明
4. **影响范围**：明确列出受影响的模块和文件
5. **测试结果**：包含相关的测试验证结果
6. **原子性**：每次提交只做一件事
7. **可追溯**：commit 信息应该能让人理解做了什么以及为什么

---

## 🧪 测试代码规范

### **测试文件命名**

```
test_<module_name>.c          # 标准测试模块
test_<module_name>_enhanced.c # 增强版测试模块
test_<category>.c             # 特定类型测试（如 memory_safety）
```

**示例**：
```c
test_pmm.c              // 物理内存管理基础测试
test_process_enhanced.c // 进程管理增强测试
test_memory_safety.c    // 内存安全专项测试
test_performance.c      // 性能基准测试
```

### **测试函数命名**

```c
static int test_<module>_<feature>(void);
static int test_<module>_<scenario>(void);
```

**示例**：
```c
// ✅ 正确的命名
static int test_pmm_allocation_basic(void);
static int test_vfs_nested_directories(void);
static int test_perf_kmalloc_throughput(void);

// ❌ 错误的命名
static int TestPMMAllocation();          // PascalCase
static int pmm_test_basic();             // 前缀错误
static int test_pmm_alloc();             // 太模糊
```

### **测试用例结构**

每个测试文件必须遵循以下结构：

```c
#include "kernel_test.h"
#include "<相关头文件>.h"

// 外部函数声明（如果需要）
extern return_type function_name(params);

// ==================== 测试用例实现 ====================

/**
 * @brief 简短描述测试目的
 *
 * 详细说明（可选）。
 * 包括测试的场景、预期行为等。
 *
 * @return TEST_PASS 通过
 * @return TEST_FAIL 失败
 * @return TEST_SKIP 跳过（前置条件不满足）
 */
static int test_<name>(void) {
    // 1. 准备阶段：创建测试数据和环境
    void *ptr = kmalloc(100);
    if (ptr == NULL) {
        return TEST_SKIP;  // 资源不足时跳过
    }

    // 2. 执行阶段：调用被测函数
    memset(ptr, 0xAA, 100);

    // 3. 验证阶段：检查结果
    TEST_ASSERT_EQ(ptr[0], 0xAA);  // 使用宏进行断言
    TEST_ASSERT_NE(ptr, NULL);

    // 4. 清理阶段：释放资源
    kfree(ptr);

    return TEST_PASS;  // 或 TEST_FAIL
}

// ==================== 模块注册 ====================

void test_<module>_register(void) {
    int mod = test_register_module("<模块显示名称>");
    if (mod < 0) return;  // 注册失败则返回

    // 注册所有测试用例
    test_register_case(mod, "<用例1名称>", test_<case1>);
    test_register_case(mod, "<用例2名称>", test_<case2>);
    test_register_case(mod, "<用例3名称>", test_<case3>);
}
```

### **断言宏使用**

```c
// 相等性检查
TEST_ASSERT_EQ(actual, expected);       // actual == expected
TEST_ASSERT_NE(actual, not_expected);   // actual != expected

// 范围检查
TEST_ASSERT_GT(value, threshold);       // value > threshold
TEST_ASSERT_GE(value, min_value);       // value >= min_value
TEST_ASSERT_LT(value, max_value);       // value < max_value
TEST_ASSERT_LE(value, max_value);       // value <= max_value

// 指针检查
TEST_ASSERT_NOT_NULL(pointer);          // pointer != NULL
TEST_ASSERT_NULL(pointer);              // pointer == NULL

// 布尔检查
TEST_ASSERT_TRUE(condition);            // condition 为真
TEST_ASSERT_FALSE(condition);           // condition 为假
```

### **测试分类规范**

#### **核心系统测试**
```c
void test_pmm_register(void) {
    int mod = test_register_module("PMM");
    
    // 基础功能测试
    test_register_case(mod, "Basic allocation", test_pmm_alloc_basic);
    test_register_case(mod, "Free operation", test_pmm_free_basic);
    
    // 边界条件测试
    test_register_case(mod, "Zero allocation", test_pmm_alloc_zero);
    test_register_case(mod, "Large allocation", test_pmm_alloc_large);
    
    // 压力测试
    test_register_case(mod, "Stress test", test_pmm_stress_50);
}
```

#### **增强测试模块**
```c
void test_process_enhanced_register(void) {
    int mod = test_register_module("Process Management Enhanced");
    
    // 高级场景测试
    test_register_case(mod, "Process tree structure", test_process_tree_structure);
    test_register_case(mod, "Priority inheritance", test_process_priority_inheritance);
    test_register_case(mod, "Rapid create/destroy", test_process_rapid_create_destroy);
    
    // 并发和安全测试
    test_register_case(mod, "Concurrent creation", test_process_concurrent_creation);
    test_register_case(mod, "Resource limits", test_process_resource_limits);
}
```

#### **质量保证测试**
```c
void test_memory_safety_register(void) {
    int mod = test_register_module("Memory Safety");
    
    // 安全特性测试
    test_register_case(mod, "NULL pointer handling", test_kmalloc_null_pointer);
    test_register_case(mod, "Double-free protection", test_kmalloc_double_free_protection);
    test_register_case(mod, "Buffer overflow detection", test_kmalloc_buffer_overflow_detection);
}
```

### **测试输出规范**

在测试中使用统一的日志格式：

```c
serial_puts(SERIAL_COM1, "[MODULE] Description: ");
serial_put_dec(SERIAL_COM1, value);
serial_puts(SERIAL_COM1, "\n");

// 性能数据输出
serial_puts(SERIAL_COM1, "[PERF] Operation: ");
serial_put_dec(SERIAL_COM1, count);
serial_puts(SERIAL_COM1, " iterations in ");
serial_put_dec(SERIAL_COM1, elapsed_time);
serial_puts(SERIAL_COM1, " ticks\n");
```

**日志级别标识**：
- `[DEBUG]` - 调试信息（开发阶段）
- `[INFO]` - 一般信息
- `[WARN]` - 警告（非致命问题）
- `[ERROR]` - 错误（测试失败原因）

### **测试最佳实践**

1. **幂等性**：测试可以重复执行且结果一致
2. **独立性**：每个测试用例相互独立，不依赖执行顺序
3. **快速执行**：单个测试用例应在合理时间内完成（<100ms）
4. **清晰断言**：使用明确的断言消息说明预期 vs 实际
5. **资源清理**：确保测试结束后释放所有分配的资源
6. **跳过而非失败**：当前置条件不满足时返回 TEST_SKIP
7. **覆盖边界**：包括正常值、边界值和异常值

### **性能基准测试**

对于性能测试，需要记录并输出关键指标：

```c
static int test_perf_operation(void) {
    const int iterations = 100;
    uint64_t start = timer_get_ticks();
    
    for (int i = 0; i < iterations; i++) {
        perform_operation();
    }
    
    uint64_t end = timer_get_ticks();
    uint64_t elapsed = end - start;
    
    // 输出性能数据
    serial_puts(SERIAL_COM1, "[PERF] ");
    serial_puts(SERIAL_COM1, "Operation: ");
    serial_put_dec(SERIAL_COM1, iterations);
    serial_puts(SERIAL_COM1, " iters in ");
    serial_put_dec(SERIAL_COM1, (uint32_t)elapsed);
    serial_puts(SERIAL_COM1, " ticks (");
    serial_put_dec(SERIAL_COM1, (uint32_t)(elapsed / (iterations > 0 ? iterations : 1)));
    serial_puts(SERIAL_COM1, " us/iter)\n");
    
    TEST_ASSERT_GT(elapsed, 0);
    return TEST_PASS;
}
```

---

**Maintainers**: QueenX Development Team  
**Review Cycle**: Quarterly  
**Last Review**: 2026-05-02

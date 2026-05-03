# QueenX 操作系统 - 代码规范指南

## 📋 概述

本文档定义了 QueenX 操作系统项目的统一编码标准。所有贡献者**必须**遵循这些准则，以确保代码的一致性和可维护性。

**版本**: 1.0
**最后更新**: 2026-05-03
**支持语言**: C（内核）、Rust（安全关键组件）、Assembly（引导程序）

---

## 🎯 命名规范

### **C 语言（内核和驱动）**

#### **函数：`snake_case`（蛇形命名）**
```c
// ✅ 正确
void idt_init(void);
int pwid_elevate(uint64_t target, const char *password);
uint64_t pmm_alloc_pages(uint64_t count);

// ❌ 错误
void IdtInit(void);           // 驼峰式命名
int PwidElevate(...);        // 帕斯卡命名
```

#### **变量：`snake_case`**
```c
// ✅ 正确
uint64_t nested_interrupt_count;
struct idt_entry idt[IDT_ENTRIES];
int current_process_id;

// ❌ 错误
uint64_t NestedInterruptCount;  // 驼峰式命名
int CurrentProcessID;            // 帕斯卡命名
```

#### **常量/宏：`UPPER_SNAKE_CASE`（大写蛇形）**
```c
// ✅ 正确
#define IDT_ENTRIES 256
#define IRQ_BASE 32
#define MAX_PATH_LENGTH 4096

// ❌ 错误
#define IdtEntries 256          // 驼峰式命名
#define max_path_length 4096    // 小写
```

#### **结构体/类型：`PascalCase`（帕斯卡命名）+ `_t` 后缀**
```c
// ✅ 正确
typedef struct interrupt_frame InterruptFrame;  // 或 struct interrupt_frame
typedef uint64_t pwid_t;
struct process { ... };

// ❌ 错误
typedef struct interrupt_frame interrupt_frame_t;  // 冗余的 _t
struct Process { ... };                            // 与内核风格不一致
```

#### **枚举：值使用 `UPPER_SNAKE_CASE`，类型使用 `PascalCase`**
```c
// ✅ 正确
typedef enum {
    PWID_LEVEL_ROOT = 0,
    PWID_LEVEL_TRUSTWORTHY = 1,
    PWID_LEVEL_UNTRUSTWORTHY = 2
} PwidLevel;

// ❌ 错误
enum pwid_level { root, trustworthy, untrustworthy };  // 小写值
```

---

### **Rust 语言（安全关键组件）**

#### **函数/方法：`snake_case`**
```rust
// ✅ 正确
pub fn find_mount(&self, path: &str) -> Option<usize> {
pub fn write_superblock_to_disk(&self) -> i32 {

// ❌ 错误
pub fn FindMount(...) {    // 帕斯卡命名
pub fn WriteSuperBlock() { // 驼峰式命名
```

#### **变量：`snake_case`**
```rust
// ✅ 正确
let mount_idx: Option<usize> = None;
let best_len = 0usize;
let mut bytes_written = 0usize;

// ❌ 错误
let MountIdx: Option<usize> = None;   // 帕斯卡命名
let BestLen = 0usize;                  // 驼峰式命名
```

#### **结构体/枚举/类型：`PascalCase`**
```rust
// ✅ 正确
pub struct HvFsData { ... }
pub enum VfsFileType { ... }
pub type PwidToken = u64;

// ❌ 错误
pub struct hvfs_data { ... }     // 蛇形命名
pub enum vfs_file_type { ... }   // 蛇形命名
```

#### **常量：`UPPER_SNAKE_CASE` 或 `SCREAMING_SNAKE_CASE`**
```rust
// ✅ 正确
const HVFS_MAGIC: u32 = 0x48565F53;
const RAMFS_MAX_BLOCKS: u32 = 1024;
static mut GLOBAL_COUNTER: u64 = 0;

// ❌ 错误
const HvfsMagic: u32 = ...      // 驼峰式命名
const ramfs_max_blocks: u32 = ...  // 小写
```

#### **模块/文件：`snake_case`**
```rust
// ✅ 正确 (文件: src/fs/vfs/vfs.rs)
mod vfs { ... }

// 文件名: mod.rs, hvfs.rs, diskfs.rs (全部小写)
```

---

### **汇编语言**

#### **标签：`snake_case`**
```asm
; ✅ 正确
isr_common_stub:
irq_common_stub:
idt_flush:

; ❌ 错误
IsrCommonStub:
IRQ_CommonStub:
```

#### **注释：清晰且具有描述性**
```asm
; 将寄存器压入栈中保存
push rbp
mov rbp, rsp

; 调用 C 语言处理函数
call exception_handler
```

---

## 📐 代码格式化

### **缩进**
- **宽度**：4 个空格（禁止使用 Tab）
- **续行**：8 个空格（或与左括号对齐）

```c
// ✅ 正确 - 4 空格缩进
if (condition) {
    do_something();
    if (nested) {
        do_nested();
    }
}

// ✅ 正确 - 续行对齐
long_function_name(parameter_one,
                    parameter_two,
                    parameter_three);
```

### **花括号风格：K&R（左花括号在同一行）**
```c
// ✅ 正确 - K&R 风格
if (condition) {
    statement;
} else {
    other_statement;
}

// ❌ 错误 - Allman 风格（左花括号在新行）
if (condition)
{
    statement;
}
```

**例外情况**：函数定义（为保持一致性使用 K&R）
```c
// ✅ 正确
int function_name(int param) {
    return param + 1;
}
```

### **行长度限制：最多 100 个字符**
```c
// ✅ 正确 - 不超过 100 字符
if (very_long_condition && another_long_condition && yet_another) {
    break;
}

// ✅ 正确 - 分割长行
result = some_very_long_function_name(first_parameter,
                                    second_parameter,
                                    third_parameter);
```

### **空行使用**
- **函数之间**：2 个空行
- **逻辑段落之间**：1 个空行
- **函数内部**：谨慎使用，用于分组相关语句

```c
// ✅ 正确的间距
void function_one(void) {
    /* 实现代码 */
}


void function_two(void) {
    /* 实现代码 */
}
```

---

## 💬 注释风格

### **C 语言：公共 API 使用 Doxygen 风格**
```c
/**
 * @brief 函数简短描述
 *
 * 详细描述（如果需要）。
 * 可以跨越多行。
 *
 * @param param1 第一个参数描述
 * @param param2 第二个参数描述
 * @return 返回值描述
 * @retval 0 成功
 * @retval -1 发生错误
 *
 * @note 重要使用说明
 * @warning 潜在陷阱
 * @see 相关函数()
 */
int example_function(int param1, char *param2);
```

### **行内注释：单行使用 `//`，多行使用 `/* */`**
```c
// 单行注释（推荐）
/* 传统多行注释 */
```

### **TODO/FIXME/HACK 标签**
```c
// TODO(用户名): 在 YYYY-MM-DD 前实现功能 X
// FIXME: 这里存在竞态条件 - 需要互斥锁
// HACK: 硬件 bug XYZ 的临时解决方案
// NOTE: 性能关键路径 - 后续优化
// SAFETY: 调用此函数前必须持有锁
```

### **Rust 语言：文档注释使用 `///`，普通注释使用 `//`**
```rust
/// 函数简短描述。
///
/// # 示例
///
/// ```
/// let result = function_call();
/// assert!(result.is_ok());
/// ```
///
/// # 参数
///
/// * `param1` - 描述
///
/// # 返回值
///
/// * `Ok(value)` - 成功时
/// * `Err(e)` - 出错时
pub fn documented_function(param1: Type) -> Result<Type, Error> {
    // 普通注释
    unimplemented!();
}
```

---

## 📁 文件组织结构

### **头文件 (.h)**

**结构顺序**：
1. 许可证头文件（如适用）
2. `#ifndef` / `#define` 头文件保护（或 `#pragma once`）
3. 包含文件（系统头文件在前，项目头文件在后）
4. 宏定义 (`#define`)
5. 类型定义 (`typedef`, `struct`, `enum`)
6. 全局变量声明 (`extern`)
7. 函数原型

**示例**：
```c
/**
 * @file filename.h
 * @brief 模块一行描述
 */

#ifndef FILENAME_H
#define FILENAME_H

#include "types.h"
#include <stdint.h>

/* ============================================================
 * 常量和宏定义
 * ============================================================ */
#define MAX_BUFFER_SIZE 4096

/* ============================================================
 * 类型定义
 * ============================================================ */
struct example_struct {
    int field1;
    char field2[256];
};

/* ============================================================
 * 函数原型
 * ============================================================ */
int init_module(void);
void cleanup_module(void);

#endif /* FILENAME_H */
```

### **源文件 (.c)**

**结构顺序**：
1. 许可证头文件
2. 包含文件（对应的 .h 文件在前）
3. 私有宏/常量
4. 私有类型定义
5. 静态/全局变量
6. 静态辅助函数
7. 公共 API 实现

**示例**：
```c
/**
 * @file filename.c
 * @brief 模块功能实现
 */

#include "filename.h"
#include "other_header.h"

/* ============================================================
 * 私有常量
 * ============================================================ */
static const int DEFAULT_TIMEOUT = 30;

/* ============================================================
 * 私有函数
 * ============================================================ */
static int helper_function(int x) {
    return x * 2;
}

/* ============================================================
 * 公共 API 实现
 * ============================================================ */
int init_module(void) {
    return 0;
}
```

### **Rust 模块 (mod.rs)**

**结构顺序**：
1. 模块文档 (`//!`)
2. 重导出 (`pub use`)
3. 公共类型
4. 公共 Trait/实现
5. 私有辅助函数（如有）

**示例**：
```rust
//! 模块描述。

pub use self::internal::InternalType;

pub mod internal;

pub struct PublicType { ... }

impl PublicType {
    pub fn new() -> Self { ... }
}
```

---

## 🔀 特定规范指南

### **C 语言错误处理**
```c
// ✅ 使用统一的错误码
#define ERROR_NONE       0
#define ERROR_INVALID   -1
#define ERROR_NO_MEMORY -2
#define ERROR_NOT_FOUND -3

// 错误时返回负数，成功时返回 0 或正数
int operation(int input) {
    if (input < 0) {
        return ERROR_INVALID;
    }
    return 0;  // 成功
}

// 始终检查返回值
int result = dangerous_operation();
if (result != ERROR_NONE) {
    log_error("操作失败: %d", result);
    return result;
}
```

### **C 语言内存管理**
```c
// ✅ 使用前初始化所有变量
char buffer[256] = {0};
int count = 0;

// ✅ 分配后检查 NULL
void *ptr = malloc(size);
if (ptr == NULL) {
    return ERROR_NO_MEMORY;
}

// ✅ 完成后释放内存
free(ptr);
ptr = NULL;  // 防止释放后使用
```

### **内核日志记录**
```c
// ✅ 统一使用串口输出
serial_puts(SERIAL_COM1, "[模块名] 消息\n");
serial_put_hex(SERIAL_COM1, value);
serial_put_dec(SERIAL_COM1, count);

// 日志级别（可选增强）
#define LOG_DEBUG   "[调试] "
#define LOG_INFO    "[信息]  "
#define LOG_WARNING "[警告]  "
#define LOG_ERROR   "[错误] "

serial_puts(SERIAL_COM1, LOG_ERROR "严重故障\n");
```

---

## ⚠️ 常见陷阱及避免方法

### **1. 命名不一致**
```c
// ❌ 差 - 同一模块内混合使用不同风格
void InitSystem(void);         // 帕斯卡命名
int get_process_count(void);   // 蛇形命名
#define maxProcesses 100;       // 驼峰命名

// ✅ 好 - 全程保持一致
void system_init(void);        // 蛇形命名
int process_get_count(void);   // 蛇形命名
#define MAX_PROCESSES 100;      // 大写蛇形命名
```

### **2. 魔法数字**
```c
// ❌ 差 - 含义不明确
if (size > 512) { ... }
sleep(3000);  // 什么单位？秒？毫秒？

// ✅ 好 - 使用命名常量
#define SECTOR_SIZE 512
#define TIMEOUT_MS 3000

if (size > SECTOR_SIZE) { ... }
sleep_ms(TIMEOUT_MS);
```

### **3. 深层嵌套**
```c
// ❌ 差 - 难以阅读
if (a) {
    if (b) {
        if (c) {
            do_something();
        }
    }
}

// ✅ 好 - 提前返回减少嵌套
if (!a || !b || !c) {
    return ERROR_INVALID;
}
do_something();
```

---

## 🛠️ 工具配置

### **EditorConfig (.editorconfig)**
项目根目录下的 `.editorconfig` 文件包含自动化格式化规则。

### **推荐的 VS Code 扩展**
- C/C++ IntelliSense
- rust-analyzer
- EditorConfig for VS Code
- Better TOML (用于 Cargo.toml)

### **Clang Format（可选）**
创建 `.clang-format` 文件实现 C 代码自动格式化：
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

## ✅ 提交前检查清单

- [ ] 所有函数/变量遵循命名规范（C/Rust 使用 snake_case）
- [ ] 所有常量/宏使用 UPPER_SNAKE_CASE
- [ ] 所有结构体/类型使用 PascalCase
- [ ] 缩进使用 4 个空格（禁止 Tab）
- [ ] 行长度 ≤ 100 个字符
- [ ] 花括号使用 K&R 风格
- [ ] 无尾部空白字符
- [ ] 文件以换行符结尾
- [ ] 公共 API 有文档注释
- [ ] 无遗留的 TODO/FIXME（无关联 issue 的）
- [ ] 代码编译无警告（或警告有合理解释）

---

## 📚 参考资料

- [Linux 内核编码风格](https://www.kernel.org/doc/html/latest/process/coding-style.html)
- [Rust API 指南](https://rust-lang.github.io/api-guidelines/)
- [Google C++ 风格指南](https://google.github.io/styleguide/cppguide/)（适配 C 语言）

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
test_<模块名>.c              # 标准测试模块
test_<模块名>_enhanced.c      # 增强版测试模块
test_<类别>.c                 # 特定类型测试（如 memory_safety）
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
static int test_<模块>_<功能>(void);
static int test_<模块>_<场景>(void);
```

**示例**：
```c
// ✅ 正确的命名
static int test_pmm_allocation_basic(void);
static int test_vfs_nested_directories(void);
static int test_perf_kmalloc_throughput(void);

// ❌ 错误的命名
static int TestPMMAllocation();          // 帕斯卡命名
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
static int test_<名称>(void) {
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

void test_<模块>_register(void) {
    int mod = test_register_module("<模块显示名称>");
    if (mod < 0) return;  // 注册失败则返回

    // 注册所有测试用例
    test_register_case(mod, "<用例1名称>", test_<用例1>);
    test_register_case(mod, "<用例2名称>", test_<用例2>);
    test_register_case(mod, "<用例3名称>", test_<用例3>);
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
    test_register_case(mod, "基本分配", test_pmm_alloc_basic);
    test_register_case(mod, "释放操作", test_pmm_free_basic);
    
    // 边界条件测试
    test_register_case(mod, "零大小分配", test_pmm_alloc_zero);
    test_register_case(mod, "大块分配", test_pmm_alloc_large);
    
    // 压力测试
    test_register_case(mod, "压力测试", test_pmm_stress_50);
}
```

#### **增强测试模块**
```c
void test_process_enhanced_register(void) {
    int mod = test_register_module("进程管理增强版");
    
    // 高级场景测试
    test_register_case(mod, "进程树结构", test_process_tree_structure);
    test_register_case(mod, "优先级继承", test_process_priority_inheritance);
    test_register_case(mod, "快速创建/销毁", test_process_rapid_create_destroy);
    
    // 并发和安全测试
    test_register_case(mod, "并发创建", test_process_concurrent_creation);
    test_register_case(mod, "资源限制", test_process_resource_limits);
}
```

#### **质量保证测试**
```c
void test_memory_safety_register(void) {
    int mod = test_register_module("内存安全");
    
    // 安全特性测试
    test_register_case(mod, "空指针处理", test_kmalloc_null_pointer);
    test_register_case(mod, "双重释放保护", test_kmalloc_double_free_protection);
    test_register_case(mod, "缓冲区溢出检测", test_kmalloc_buffer_overflow_detection);
}
```

### **测试输出规范**

在测试中使用统一的日志格式：

```c
serial_puts(SERIAL_COM1, "[模块] 描述: ");
serial_put_dec(SERIAL_COM1, value);
serial_puts(SERIAL_COM1, "\n");

// 性能数据输出
serial_puts(SERIAL_COM1, "[性能] 操作: ");
serial_put_dec(SERIAL_COM1, count);
serial_puts(SERIAL_COM1, " 次迭代，耗时 ");
serial_put_dec(SERIAL_COM1, elapsed_time);
serial_puts(SERIAL_COM1, " 时钟周期\n");
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
    serial_puts(SERIAL_COM1, "[性能] ");
    serial_puts(SERIAL_COM1, "操作: ");
    serial_put_dec(SERIAL_COM1, iterations);
    serial_puts(SERIAL_COM1, " 次迭代，耗时 ");
    serial_put_dec(SERIAL_COM1, (uint32_t)elapsed);
    serial_puts(SERIAL_COM1, " 时钟周期 (");
    serial_put_dec(SERIAL_COM1, (uint32_t)(elapsed / (iterations > 0 ? iterations : 1)));
    serial_puts(SERIAL_COM1, " us/次)\n");
    
    TEST_ASSERT_GT(elapsed, 0);
    return TEST_PASS;
}
```

---

**维护者**: QueenX 开发团队
**审查周期**: 季度
**最后审查**: 2026-05-03

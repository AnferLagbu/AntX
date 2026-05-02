# QueenX 操作系统 - 代码风格规范 v1.0

> **生效日期**: 2026-05-02
> **适用范围**: 所有 C (.c/.h) 和 Rust (.rs) 源文件
> **强制执行**: 是 (CI/CD 检查)

---

## 一、命名规范 (Naming Conventions)

### 1.1 C 语言命名规则

| 类型 | 规范 | 示例 |
|------|------|------|
| **函数** | `snake_case` | `pwid_elevate()`, `idt_set_gate()` |
| **变量 (局部)** | `snake_case` | `block_num`, `inode_count` |
| **变量 (静态/全局)** | `g_` 前缀 + `snake_case` | `g_current_context`, `g_idt_table` |
| **常量** | `UPPER_SNAKE_CASE` | `MAX_PWID_ENTRIES`, `VFS_MAX_NAME` |
| **宏定义** | `UPPER_SNAKE_CASE` + 模块前缀 | `PWID_ERR_DENIED`, `VFS_PERM_R` |
| **枚举值** | `UPPER_SNAKE_CASE` | `PWID_LEVEL_ROOT`, `VFS_FILE_REGULAR` |
| **结构体** | `PascalCase` + `_t` 后缀 | `struct pwid_entry_t`, `struct idt_entry_t` |
| **联合体** | `PascalCase` + `_u` 后缀 | `union value_u` |
| **类型定义 (typedef)** | `PascalCase` + `_t` 后缀 | `typedef uint64_t pwid_t;` |

### 1.2 Rust 语言命名规则

| 类型 | 规范 | 示例 |
|------|------|------|
| **函数/方法** | `snake_case` | `fn read_file_data()`, `fn alloc_block()` |
| **变量** | `snake_case` | `let block_num = 0;` |
| **常量 (const)** | `UPPER_SNAKE_CASE` | `const HVFS_MAGIC: u32` |
| **静态变量 (static)** | `UPPER_SNAKE_CASE` | `static mut INSTANCE: Option<HvFsData>` |
| **结构体/枚举** | `PascalCase` | `struct HvfsInode`, `enum VfsFileType` |
| **Trait** | `PascalCase` | `trait FileSystem` |
| **模块 (mod)** | `snake_case` | `mod pwid;`, `mod hvfs;` |
| **宏 (macro_rules!)** | `UPPER_SNAKE_CASE` | `macro_rules! SERIAL_PUT_HEX` |

### 1.3 文件和目录命名

| 类型 | 规范 | 示例 |
|------|------|------|
| **C 源文件** | `snake_case.c` | `pwid.c`, `idt.c`, `serial.c` |
| **C 头文件** | `snake_case.h` | `pwid.h`, `types.h` |
| **Rust 模块** | `snake_case.rs` | `mod.rs`, `hvfs.rs`, `permission.rs` |
| **目录名** | `snake_case` | `src/kernel/`, `src/fs/vfs/` |

---

## 二、格式化规范 (Formatting)

### 2.1 缩进与空格

```c
// ✅ 正确：使用 4 空格缩进
void example_function(int param1, int param2) {
    if (condition) {
        do_something();
    } else {
        do_other();
    }
}

// ❌ 错误：使用 Tab 或混合缩进
void bad_example(int a, int b) {
	if (wrong_indent) {  // Tab 缩进
		do_bad();         // Tab + Space 混合
	}
}
```

### 2.2 行宽限制

- **最大行宽**: **100 字符**
- **例外**: 长字符串、URL、日志消息可适当超出

```c
// ✅ 正确：合理分行
int result = some_function_with_very_long_name(
    parameter1,
    parameter2,
    parameter3
);

// ❌ 错误：超过100字符
int result = some_function_with_very_long_name(parameter1, parameter2, parameter3);
```

### 2.3 大括号风格

```c
// ✅ K&R 风格（推荐用于控制语句）
if (condition) {
    do_something();
} else {
    do_other();
}

// ✅ Allman 风格（用于函数定义）
void function_name(void)
{
    // 函数体
}
```

---

## 三、注释规范 (Comments)

### 3.1 文件头注释

每个源文件必须包含以下头部：

```c
/**
 * @file filename.c
 * @brief 模块简短描述（一句话）
 * 
 * 详细描述（可选）：
 * - 功能说明
 * - 使用示例
 * - 注意事项
 * 
 * @author 作者名 (可选)
 * @date 创建日期 (可选)
 * @version 版本号 (可选)
 * @copyright 版权信息 (可选)
 *
 * QueenX Operating System
 */
```

### 3.2 函数注释

公共 API 必须有完整注释：

```c
/**
 * @brief 函数简短描述
 * 
 * 详细说明（如果需要）
 * 
 * @param param1 参数1描述
 * @param param2 参数2描述
 * @return 返回值描述
 *         - 成功: 返回0或正数
 *         - 失败: 返回负数错误码
 * 
 * @note 注意事项（可选）
 * @warning 警告信息（可选）
 * @example
 * @code
 * int result = function_name(arg1, arg2);
 * if (result < 0) { handle_error(); }
 * @endcode
 */
```

### 3.3 行内注释

```c
// ✅ 正确：空格 + 有意义的内容
int count = 0;  // 初始化计数器

// TODO(作者): 待实现的功能
// FIXME(作者): 已知问题，需要修复
// HACK(作者): 临时解决方案
// SAFETY: 此处需要手动保证安全

// ❌ 错误：无意义的注释
int x = 5;  // 设置x为5
```

### 3.4 注释语言

- **优先使用英文注释**（便于国际化）
- **中文注释仅限**：
  - 内部讨论标记
  - 特定业务逻辑说明
  - 调试输出信息

```c
// ✅ 推荐：英文注释
/** Initialize the IDT with default handlers */

// ⚠️ 可接受：中文注释（特定场景）
// TODO(张三): 实现中文支持功能
serial_puts(SERIAL_COM1, "初始化完成\n");  // 用户可见的字符串可用中文
```

---

## 四、错误处理规范 (Error Handling)

### 4.1 错误码定义

```c
// 在模块头文件中统一定义
#define MODULE_ERR_BASE      (-1000)
#define MODULE_ERR_INVALID   (MODULE_ERR_BASE - 1)   // -1001
#define MODULE_ERR_NOT_FOUND (MODULE_ERR_BASE - 2)   // -1002
#define MODULE_ERR_NO_MEM    (MODULE_ERR_BASE - 3)   // -1003
#define MODULE_ERR_DENIED    (MODULE_ERR_BASE - 4)   // -1004
#define MODULE_ERR_TIMEOUT   (MODULE_ERR_BASE - 5)   // -1005
```

### 4.2 错误检查模式

```c
// ✅ 正确：检查所有可能失败的调用
int result = dangerous_operation();
if (result != 0) {
    log("Operation failed: %d\n", result);
    return result;  // 向上传播错误
}

// ✅ 正确：使用 goto 进行清理（仅限C语言）
int complex_function(void) {
    int fd = -1;
    void *buffer = NULL;
    
    fd = open_file(path);
    if (fd < 0) goto cleanup;
    
    buffer = allocate_memory(size);
    if (!buffer) goto cleanup;
    
    // ... 使用资源 ...
    
cleanup:
    if (buffer) free(buffer);
    if (fd >= 0) close(fd);
    return error_code;
}
```

### 4.3 日志输出格式

```c
// 统一日志格式：[模块] 级别: 消息
serial_puts(SERIAL_COM1, "[VFS] INFO: File created successfully\n");
serial_puts(SERIAL_COM1, "[PMM] ERROR: Out of memory\n");
serial_puts(SERIAL_COM1, "[IDT] WARN: Spurious IRQ detected\n");

// 调试输出使用 DEBUG 级别
#ifdef DEBUG
serial_puts(SERIAL_COM1, "[DEBUG] Variable x = ");
serial_put_hex(SERIAL_COM1, x);
serial_puts(SERIAL_COM1, "\n");
#endif
```

---

## 五、模块组织规范 (Module Organization)

### 5.1 C 文件结构

```c
/* ================================================================== */
/*                          模块名称                                */
/* ================================================================== */

/* 头文件包含（按字母序） */
#include "header1.h"
#include "header2.h"

/* 宏定义 */
#define CONSTANT_VALUE  100

/* 类型定义 */
typedef struct {
    int field;
} my_type_t;

/* 静态变量声明 */
static int g_static_var = 0;

/* 内部函数声明 */
static int internal_helper(void);

/* 公共函数实现 */

/* ... */

/* 内部函数实现 */
static int internal_helper(void) {
    return 0;
}

/* ================================================================== */
/*                            End of module                           */
/* ================================================================== */
```

### 5.2 Rust 模块结构

```rust
//! Module documentation
//!
//! Detailed description of this module's purpose and usage.

// 外部依赖
use crate::other_module::Type;

// 常量定义
pub const MAX_SIZE: usize = 1024;

// 类型定义
#[derive(Debug, Clone)]
pub struct MyStruct {
    pub field: i32,
}

// 实现
impl MyStruct {
    /// Create a new instance
    pub fn new() -> Self {
        Self { field: 0 }
    }
}

// 私有辅助函数
fn helper() -> bool {
    true
}
```

---

## 六、特殊场景规范

### 6.1 FFI (Foreign Function Interface)

```rust
// Rust 侧声明外部 C 函数
extern "C" {
    /// C function description
    fn c_function_name(param: u32) -> i32;
}

// 导出给 C 的函数
#[no_mangle]
pub extern "C" fn rust_function_for_c(param: i32) -> i32 {
    param + 1
}
```

### 6.2 并发安全标注

```c
// 标注线程安全性
/** @thread_safety: This function is NOT thread-safe, use mutex */
void non_thread_safe_func(void);

/** @thread_safety: This function IS thread-safe (uses internal lock) */
void thread_safe_func(void);
```

### 6.3 性能关键代码

```c
// 标注性能特征
/** @performance: O(n) time complexity, O(1) space */
void algorithm_with_linear_time(void);

/** @hotpath: This function is in the hot path, optimize carefully */
inline void frequently_called_function(void);
```

---

## 七、工具配置

### 7.1 .editorconfig (推荐)

```ini
# EditorConfig for QueenX project
root = true

[*]
charset = utf-8
end_of_line = lf
insert_final_newline = true
trim_trailing_whitespace = true
indent_style = space
indent_size = 4
max_line_length = 100

[*.rs]
indent_size = 4

[*.{c,h}]
indent_size = 4

[Makefile]
indent_style = tab
```

### 7.2 Git Hooks (pre-commit)

建议添加 pre-commit hook 自动检查：
- 行宽不超过 100 字符
- 无 Tab 字符（除 Makefile）
- 文件末尾有换行符
- 无 trailing whitespace

---

## 八、违规示例与修正

### ❌ 常见错误

```c
// 错误1: 混用命名风格
int MyVariable;           // 应为: my_variable 或 g_my_variable
#define maxCount 100       // 应为: MAX_COUNT
void DoSomething();       // 应为: do_something()

// 错误2: 不一致的注释
/* This function does X */  // 应使用 /** */ 格式
// it does Y               // 应该更有描述性

// 错误3: 过长的行
if (some_very_long_variable_name == some_other_very_long_value && another_condition) {

// 错误4: 魔法数字
int size = 256;          // 应为: #define BUFFER_SIZE 256
if (status == -1) {     // 应为: if (status == MODULE_ERR_NOT_FOUND)
```

### ✅ 修正后

```c
// 修正1: 统一命名
int g_my_variable;
#define MAX_COUNT 100
void do_something(void);

// 修正2: 规范注释
/**
 * @brief Does something useful
 * @return 0 on success, negative error code on failure
 */
int do_something(void);

// 修正3: 合理分行
if ((some_very_long_variable_name == some_other_very_long_value) &&
    (another_condition)) {

// 修正4: 使用命名常量
#define BUFFER_SIZE 256
int size = BUFFER_SIZE;
if (status == MODULE_ERR_NOT_FOUND) {
```

---

## 九、审查清单 (Review Checklist)

提交前请确认：

- [ ] 所有新代码符合本规范
- [ ] 公共 API 有完整文档注释
- [ ] 错误处理完善且一致
- [ ] 无编译警告（除明确忽略的）
- [ ] 日志输出使用统一格式 `[MODULE] LEVEL: message`
- [ ] 命名无歧义且有意义
- [ ] 避免全局变量（必要时使用 g_ 前缀）
- [ ] 魔法数字已替换为命名常量
- [ ] 文件头注释完整

---

**版本历史**:
- v1.0 (2026-05-02): 初始版本

# 编码规范

> AntX代码风格指南

---

## 🎯 总体原则

1. **清晰性**: 代码应该易于理解
2. **一致性**: 遵循统一的风格
3. **安全性**: 优先考虑安全
4. **性能**: 在保证安全的前提下优化

---

## 📝 C代码规范

### 命名约定

```c
// 函数：snake_case
void process_init(void);

// 变量：snake_case
int page_count = 0;

// 常量：UPPER_SNAKE_CASE
#define MAX_PROCESSES 256

// 类型：PascalCase
typedef struct ProcessBlock ProcessBlock;
```

### 格式化

```c
// 缩进：4空格
if (condition) {
    do_something();
}

// 大括号：K&R风格
if (x > 0) {
    return x;
} else {
    return -x;
}

// 函数定义
int add(int a, int b) {
    return a + b;
}
```

---

## 🦀 Rust代码规范

### 命名约定

```rust
// 函数：snake_case
fn process_init() {}

// 变量：snake_case
let page_count = 0;

// 常量：SCREAMING_SNAKE_CASE
const MAX_PROCESSES: usize = 256;

// 类型：PascalCase
struct ProcessBlock {}

// trait：PascalCase
trait Driver {}
```

### 格式化

使用 `rustfmt` 自动格式化：

```bash
cargo fmt
```

---

## 📚 注释规范

### C注释

```c
/**
 * @brief 初始化进程管理器
 * @return 0成功，-1失败
 */
int process_init(void) {
    // 分配进程表
    process_table = kmalloc(MAX_PROCESSES * sizeof(Process));
    if (!process_table) {
        return -1;
    }
    
    return 0;
}
```

### Rust注释

```rust
/// 初始化进程管理器
/// 
/// # Returns
/// 成功返回Ok(())，失败返回Err
pub fn process_init() -> Result<(), Error> {
    // 分配进程表
    let process_table = kmalloc(MAX_PROCESSES * size_of::<Process>())?;
    
    Ok(())
}
```

---

## 🔒 安全规范

1. **边界检查**: 所有数组访问必须检查边界
2. **空指针检查**: 使用前检查指针
3. **整数溢出**: 使用安全的算术运算
4. **资源释放**: 确保所有资源正确释放

---

**最后更新**: 2026-05-18

# AntX 键盘驱动

## 概述

AntX 的键盘驱动是一个增强型的 PS/2 键盘驱动程序，支持完整的键盘功能，包括功能键、方向键、数字小键盘等。

## 文件位置

- **头文件**: `src/include/keyboard.h`
- **实现文件**: `src/kernel/keyboard.c`

## 功能特性

### 已实现功能

#### 1. 基本按键支持
- ✅ 字母键（A-Z）
- ✅ 数字键（0-9）
- ✅ 特殊字符（!@#$%^&*()等）
- ✅ 回车键（Enter）
- ✅ 退格键（Backspace）
- ✅ 空格键（Space）
- ✅ Tab 键

#### 2. 修饰键支持
- ✅ 左 Shift
- ✅ 右 Shift
- ✅ 左 Ctrl
- ✅ 右 Ctrl
- ✅ 左 Alt
- ✅ 右 Alt
- ✅ Caps Lock（大小写锁定）
- ✅ Num Lock（数字锁定）
- ✅ Scroll Lock（滚动锁定）

#### 3. 功能键支持
- ✅ F1-F12 功能键

#### 4. 扩展键支持
- ✅ 方向键（↑↓←→）
- ✅ Home 键
- ✅ End 键
- ✅ Page Up 键
- ✅ Page Down 键
- ✅ Insert 键
- ✅ Delete 键

#### 5. 数字小键盘支持
- ✅ 数字键（0-9）
- ✅ 运算符（+、-、*、/）
- ✅ 小数点（.）
- ✅ Num Lock 状态支持

#### 6. 键盘状态管理
- ✅ 修饰键状态跟踪
- ✅ 键盘状态查询接口
- ✅ 扩展键前缀处理（0xE0）

#### 7. 缓冲区管理
- ✅ 环形缓冲区（256 字节）
- ✅ 缓冲区满保护
- ✅ 线程安全的读写操作

## API 接口

### 初始化函数

```c
void keyboard_init(void);
```

初始化键盘驱动，设置中断处理程序。

**功能**:
- 初始化键盘缓冲区
- 初始化键盘状态
- 清空键盘控制器缓冲区
- 启用键盘设备
- 设置键盘中断处理程序（IRQ1）

### 数据读取函数

```c
bool keyboard_has_data(void);
```

检查键盘缓冲区是否有数据。

**返回值**:
- `true` - 缓冲区有数据
- `false` - 缓冲区为空

---

```c
char keyboard_get_char(void);
```

从键盘缓冲区获取一个字符（非阻塞）。

**返回值**:
- 成功：返回读取的字符
- 失败：返回 0（缓冲区为空）

---

```c
int keyboard_read_char(void);
```

阻塞式读取一个字符。

**返回值**:
- 返回读取的字符（转换为 int）

**说明**:
- 如果缓冲区为空，则等待数据到达
- 使用 HLT 指令降低功耗

---

```c
int keyboard_read_line(char *buf, int max);
```

读取一行输入（阻塞式）。

**参数**:
- `buf` - 输出缓冲区
- `max` - 最大读取字符数

**返回值**:
- 返回实际读取的字符数（不包括结尾的 '\0'）

**功能**:
- 支持回车键结束输入
- 支持退格键删除字符
- 自动回显到串口

### 状态查询函数

```c
uint16_t keyboard_get_modifiers(void);
```

获取当前所有修饰键的状态。

**返回值**:
- 返回修饰键状态位掩码

**修饰键位定义**:
```c
#define MODIFIER_LSHIFT  (1 << 0)
#define MODIFIER_RSHIFT  (1 << 1)
#define MODIFIER_LCTRL   (1 << 2)
#define MODIFIER_RCTRL   (1 << 3)
#define MODIFIER_LALT    (1 << 4)
#define MODIFIER_RALT    (1 << 5)
#define MODIFIER_CAPS    (1 << 6)
#define MODIFIER_NUM     (1 << 7)
#define MODIFIER_SCROLL  (1 << 8)

#define MODIFIER_SHIFT   (MODIFIER_LSHIFT | MODIFIER_RSHIFT)
#define MODIFIER_CTRL    (MODIFIER_LCTRL | MODIFIER_RCTRL)
#define MODIFIER_ALT     (MODIFIER_LALT | MODIFIER_RALT)
```

---

```c
bool keyboard_is_shift_pressed(void);
```

检查 Shift 键是否按下。

**返回值**:
- `true` - Shift 键按下
- `false` - Shift 键未按下

---

```c
bool keyboard_is_ctrl_pressed(void);
```

检查 Ctrl 键是否按下。

**返回值**:
- `true` - Ctrl 键按下
- `false` - Ctrl 键未按下

---

```c
bool keyboard_is_alt_pressed(void);
```

检查 Alt 键是否按下。

**返回值**:
- `true` - Alt 键按下
- `false` - Alt 键未按下

---

```c
bool keyboard_is_caps_lock(void);
```

检查 Caps Lock 是否激活。

**返回值**:
- `true` - Caps Lock 激活
- `false` - Caps Lock 未激活

---

```c
bool keyboard_is_num_lock(void);
```

检查 Num Lock 是否激活。

**返回值**:
- `true` - Num Lock 激活
- `false` - Num Lock 未激活

## 扫描码定义

### 基本键扫描码

```c
#define KEY_ESC        0x01
#define KEY_1          0x02
#define KEY_2          0x03
// ... 其他数字键
#define KEY_0          0x0B
#define KEY_MINUS      0x0C
#define KEY_EQUAL      0x0D
#define KEY_BACKSPACE  0x0E
#define KEY_TAB        0x0F
// ... 其他字母键
#define KEY_SPACE      0x39
```

### 功能键扫描码

```c
#define KEY_F1         0x3B
#define KEY_F2         0x3C
// ...
#define KEY_F10        0x44
#define KEY_F11        0x57
#define KEY_F12        0x58
```

### 扩展键扫描码

```c
#define KEY_UP         0x48
#define KEY_DOWN       0x50
#define KEY_LEFT       0x4B
#define KEY_RIGHT      0x4D
#define KEY_HOME       0x47
#define KEY_END        0x4F
#define KEY_PGUP       0x49
#define KEY_PGDN       0x51
#define KEY_INSERT     0x52
#define KEY_DELETE     0x53
```

### 数字小键盘扫描码

```c
#define KEY_KP_7       0x47
#define KEY_KP_8       0x48
#define KEY_KP_9       0x49
#define KEY_KP_MINUS   0x4A
#define KEY_KP_4       0x4B
#define KEY_KP_5       0x4C
#define KEY_KP_6       0x4D
#define KEY_KP_PLUS    0x4E
#define KEY_KP_1       0x4F
#define KEY_KP_2       0x50
#define KEY_KP_3       0x51
#define KEY_KP_0       0x52
#define KEY_KP_DOT     0x53
#define KEY_KP_ASTERISK 0x37
```

## 数据结构

### 键盘缓冲区

```c
struct keyboard_buffer {
    char buffer[KEYBOARD_BUFFER_SIZE];  // 256 字节缓冲区
    int head;                            // 写入位置
    int tail;                            // 读取位置
    int count;                           // 当前字符数
};
```

### 键盘状态

```c
struct keyboard_state {
    uint16_t modifiers;      // 修饰键状态
    uint8_t last_scancode;   // 最后一个扫描码
    bool ext_prefix;         // 扩展键前缀标志
};
```

## 实现细节

### 中断处理

键盘驱动使用 IRQ1（中断号 33）处理键盘中断：

```c
static void keyboard_isr(struct interrupt_frame *frame) {
    uint8_t scancode = inb(KBD_DATA_PORT);
    
    // 处理扩展键前缀
    if (scancode == KEY_EXT_PREFIX) {
        kbd_state.ext_prefix = true;
        return;
    }
    
    // 处理按键释放
    bool is_released = (scancode & KEY_RELEASED) != 0;
    if (is_released) {
        scancode &= ~KEY_RELEASED;
        handle_modifier_release(scancode);
        kbd_state.ext_prefix = false;
        return;
    }
    
    // 处理修饰键按下
    // ...
    
    // 转换为 ASCII 字符
    // ...
    
    // 存入缓冲区
    if (c != 0) {
        kbd_buffer_put(c);
    }
}
```

### 扫描码转换

驱动维护两个扫描码转换表：

1. `scancode_to_ascii` - 正常状态的转换表
2. `scancode_to_ascii_shift` - Shift 按下时的转换表

### Caps Lock 处理

Caps Lock 的处理逻辑：

```c
if (c >= 'a' && c <= 'z') {
    if (caps) {
        c = c - 'a' + 'A';  // 小写转大写
    }
} else if (c >= 'A' && c <= 'Z') {
    if (caps && !shift) {
        c = c - 'A' + 'a';  // 大写转小写
    }
}
```

### Num Lock 处理

数字小键盘的处理：

```c
static char handle_numpad(uint8_t scancode) {
    if (kbd_state.modifiers & MODIFIER_NUM) {
        switch (scancode) {
            case KEY_KP_0: return '0';
            case KEY_KP_1: return '1';
            // ...
        }
    }
    return 0;
}
```

## 硬件端口

- **数据端口**: 0x60 - 读取扫描码
- **状态端口**: 0x64 - 读取状态
- **命令端口**: 0x64 - 发送命令

## 初始化流程

1. 初始化键盘缓冲区
2. 初始化键盘状态
3. 清空键盘控制器缓冲区
4. 启用键盘设备（命令 0xAE）
5. 配置键盘控制器
6. 设置中断处理程序（IRQ1）

## 使用示例

### 读取单个字符

```c
#include "keyboard.h"

void example_read_char(void) {
    if (keyboard_has_data()) {
        char c = keyboard_get_char();
        // 处理字符
    }
}
```

### 读取一行输入

```c
#include "keyboard.h"

void example_read_line(void) {
    char buffer[256];
    int len = keyboard_read_line(buffer, sizeof(buffer));
    // 处理输入
}
```

### 检查修饰键状态

```c
#include "keyboard.h"

void example_check_modifiers(void) {
    if (keyboard_is_ctrl_pressed() && keyboard_is_alt_pressed()) {
        // Ctrl+Alt 组合键
    }
    
    if (keyboard_is_shift_pressed()) {
        // Shift 键按下
    }
}
```

## 性能优化

1. **环形缓冲区**: 使用环形缓冲区避免内存拷贝
2. **位掩码状态**: 使用位掩码高效存储修饰键状态
3. **快速查表**: 使用查找表快速转换扫描码
4. **中断驱动**: 使用中断而非轮询，降低 CPU 占用

## 未来改进

### 计划功能
- [ ] 键盘 LED 控制（Caps Lock、Num Lock、Scroll Lock）
- [ ] 键盘重复速率设置
- [ ] 组合键回调机制
- [ ] 键盘布局切换（Dvorak 等）
- [ ] 多键盘支持

### 可能的优化
- [ ] 更大的缓冲区
- [ ] 键盘事件队列
- [ ] 非 ASCII 字符输入支持

## 调试信息

键盘驱动在初始化时会输出调试信息：

```
[OK] Keyboard (Enhanced)
```

这表示键盘驱动已成功初始化并启用了增强功能。

## 注意事项

1. **线程安全**: 当前的实现不是线程安全的，如果多线程访问需要添加锁
2. **缓冲区溢出**: 缓冲区满时会丢弃新字符
3. **扩展键**: 扩展键（方向键等）目前不产生 ASCII 字符，只更新状态

## 相关文档

- [内核架构](kernel-architecture.md)
- [中断处理](development.md)
- [驱动程序](development.md)

---

*最后更新: 2026-04-07 (键盘驱动优化完成)*

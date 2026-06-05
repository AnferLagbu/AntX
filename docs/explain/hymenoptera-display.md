# Hymenoptera 显示服务器设计文档

> AntX内核的显示服务器 - 多用户多会话图形系统

---

## 📋 文档信息

- **项目名称**: Hymenoptera Display Server
- **版本**: v0.1 (设计阶段)
- **作者**: AntX Team
- **日期**: 2026-05-18
- **状态**: 设计中

---

## 🎯 概述

### 命名由来

**Hymenoptera（膜翅目）**：
- 昆虫纲中的一个目
- 包括蚂蚁、蜜蜂、胡蜂等
- 特点：社会性、分工明确、高效协作

**寓意**：
- 像蜂群一样高效协作
- 每个组件各司其职
- 整体系统井然有序

---

### 设计目标

#### 核心目标

1. **多用户支持**
   - 支持多个用户同时登录
   - 用户间完全隔离
   - 安全的会话管理

2. **多会话支持**
   - 每个用户可有多个会话
   - 会话可持久化
   - 会话间可切换

3. **图形化界面**
   - 支持LVGL应用
   - 窗口管理
   - 界面合成

4. **性能优化**
   - 最小化开销
   - 高效合成
   - 低延迟

5. **安全隔离**
   - 基于PWID的权限控制
   - 显示资源隔离
   - 输入事件隔离

---

## 🏗️ 系统架构

### 整体架构

```
┌─────────────────────────────────────────────────────────┐
│                    用户态应用层                          │
│  ┌──────────┐  ┌──────────┐  ┌──────────┐              │
│  │ App A    │  │ App B    │  │ App C    │              │
│  │ (用户1)  │  │ (用户1)  │  │ (用户2)  │              │
│  └──────────┘  └──────────┘  └──────────┘              │
└─────────────────────────────────────────────────────────┘
                        ↕ IPC (Hymenoptera Protocol)
┌─────────────────────────────────────────────────────────┐
│                 Hymenoptera 显示服务器                   │
│  ┌──────────────────────────────────────────────────┐  │
│  │              会话管理器 (Session Manager)         │  │
│  │  ┌─────────┐  ┌─────────┐  ┌─────────┐          │  │
│  │  │ 会话1   │  │ 会话2   │  │ 会话3   │          │  │
│  │  │ 用户A   │  │ 用户A   │  │ 用户B   │          │  │
│  │  └─────────┘  └─────────┘  └─────────┘          │  │
│  └──────────────────────────────────────────────────┘  │
│  ┌──────────────────────────────────────────────────┐  │
│  │              窗口管理器 (Window Manager)          │  │
│  │  ┌─────┬─────┬─────┐                            │  │
│  │  │Win1 │Win2 │Win3 │                            │  │
│  │  └─────┴─────┴─────┘                            │  │
│  └──────────────────────────────────────────────────┘  │
│  ┌──────────────────────────────────────────────────┐  │
│  │              合成器 (Compositor)                  │  │
│  │  - 窗口合成                                       │  │
│  │  - 特效处理                                       │  │
│  │  - 输出渲染                                       │  │
│  └──────────────────────────────────────────────────┘  │
│  ┌──────────────────────────────────────────────────┐  │
│  │              输入管理器 (Input Manager)           │  │
│  │  - 键盘事件路由                                   │  │
│  │  - 鼠标事件路由                                   │  │
│  │  - 焦点管理                                       │  │
│  └──────────────────────────────────────────────────┘  │
└─────────────────────────────────────────────────────────┘
                        ↕
┌─────────────────────────────────────────────────────────┐
│                    内核驱动层                            │
│  ┌──────────┐  ┌──────────┐  ┌──────────┐              │
│  │ Framebuf │  │ Keyboard │  │ Mouse    │              │
│  │ Driver   │  │ Driver   │  │ Driver   │              │
│  └──────────┘  └──────────┘  └──────────┘              │
└─────────────────────────────────────────────────────────┘
```

---

## 📦 核心组件

### 1. 会话管理器 (Session Manager)

#### 数据结构

```rust
/// 会话结构
pub struct Session {
    pub id: SessionId,              // 会话ID
    pub user: UserId,               // 所属用户
    pub pwid: u64,                  // 用户PWID
    pub vt_id: VtId,                // 关联的虚拟终端
    pub windows: Vec<WindowId>,     // 会话的窗口
    pub processes: Vec<ProcessId>,  // 会话的进程
    pub state: SessionState,        // 会话状态
    pub login_time: u64,            // 登录时间
    pub surface: Surface,           // 会话的显示表面
}

/// 会话状态
pub enum SessionState {
    Active,      // 活跃
    Suspended,   // 挂起
    Locked,      // 锁定
    Closing,     // 关闭中
}

/// 会话管理器
pub struct SessionManager {
    sessions: HashMap<SessionId, Arc<Session>>,
    active_session: Option<SessionId>,
    user_sessions: HashMap<UserId, Vec<SessionId>>,  // 用户的会话列表
}
```

#### 核心API

```rust
impl SessionManager {
    /// 创建新会话
    pub fn create_session(
        &mut self,
        user: UserId,
        pwid: u64,
    ) -> Result<SessionId, Error>;
    
    /// 切换会话
    pub fn switch_session(
        &mut self,
        session_id: SessionId,
    ) -> Result<(), Error>;
    
    /// 关闭会话
    pub fn close_session(
        &mut self,
        session_id: SessionId,
    ) -> Result<(), Error>;
    
    /// 挂起会话
    pub fn suspend_session(
        &mut self,
        session_id: SessionId,
    ) -> Result<(), Error>;
    
    /// 恢复会话
    pub fn resume_session(
        &mut self,
        session_id: SessionId,
    ) -> Result<(), Error>;
}
```

---

### 2. 窗口管理器 (Window Manager)

#### 数据结构

```rust
/// 窗口结构
pub struct Window {
    pub id: WindowId,               // 窗口ID
    pub session: SessionId,         // 所属会话
    pub surface: Surface,           // 窗口表面
    pub position: Point,            // 位置
    pub size: Size,                 // 大小
    pub z_order: i32,               // 层叠顺序
    pub visible: bool,              // 是否可见
    pub state: WindowState,         // 窗口状态
    pub decorations: Decorations,   // 装饰（边框、标题栏）
}

/// 窗口状态
pub enum WindowState {
    Normal,      // 正常
    Minimized,   // 最小化
    Maximized,   // 最大化
    Fullscreen,  // 全屏
}

/// 窗口管理器
pub struct WindowManager {
    windows: HashMap<WindowId, Arc<Window>>,
    focus_window: Option<WindowId>,  // 焦点窗口
    z_order: Vec<WindowId>,          // Z序
}
```

#### 核心API

```rust
impl WindowManager {
    /// 创建窗口
    pub fn create_window(
        &mut self,
        session: SessionId,
        width: u32,
        height: u32,
    ) -> Result<WindowId, Error>;
    
    /// 销毁窗口
    pub fn destroy_window(
        &mut self,
        window_id: WindowId,
    ) -> Result<(), Error>;
    
    /// 移动窗口
    pub fn move_window(
        &mut self,
        window_id: WindowId,
        x: i32,
        y: i32,
    ) -> Result<(), Error>;
    
    /// 调整窗口大小
    pub fn resize_window(
        &mut self,
        window_id: WindowId,
        width: u32,
        height: u32,
    ) -> Result<(), Error>;
    
    /// 设置焦点
    pub fn set_focus(
        &mut self,
        window_id: WindowId,
    ) -> Result<(), Error>;
    
    /// 提升窗口（置顶）
    pub fn raise_window(
        &mut self,
        window_id: WindowId,
    ) -> Result<(), Error>;
}
```

---

### 3. 合成器 (Compositor)

#### 数据结构

```rust
/// 合成器
pub struct Compositor {
    output: Output,                 // 输出设备
    buffer: Framebuffer,            // 合成缓冲
    damage: DamageTracker,          // 损坏区域跟踪
    effects: Vec<Effect>,           // 特效
}

/// 输出设备
pub struct Output {
    pub name: String,
    pub width: u32,
    pub height: u32,
    pub refresh_rate: u32,
    pub framebuffer: *mut u32,
}

/// 损坏区域跟踪
pub struct DamageTracker {
    regions: Vec<Rectangle>,
}
```

#### 核心API

```rust
impl Compositor {
    /// 合成所有窗口
    pub fn composite(
        &mut self,
        windows: &[Arc<Window>],
    ) -> Result<(), Error>;
    
    /// 添加损坏区域
    pub fn add_damage(
        &mut self,
        region: Rectangle,
    );
    
    /// 应用特效
    pub fn apply_effect(
        &mut self,
        effect: Effect,
    );
    
    /// 输出到显示设备
    pub fn present(
        &mut self,
    ) -> Result<(), Error>;
}
```

#### 合成算法

```rust
/// 窗口合成
fn composite_windows(
    compositor: &mut Compositor,
    windows: &[Arc<Window>],
) {
    // 清空合成缓冲
    compositor.buffer.clear();
    
    // 按Z序从低到高合成
    for window in windows.iter().sorted_by_key(|w| w.z_order) {
        if !window.visible {
            continue;
        }
        
        // 合成窗口表面到合成缓冲
        blend_surface(
            &mut compositor.buffer,
            &window.surface,
            window.position,
            window.size,
        );
        
        // 绘制窗口装饰
        draw_decorations(
            &mut compositor.buffer,
            &window.decorations,
        );
    }
    
    // 应用特效
    for effect in &compositor.effects {
        apply_effect(&mut compositor.buffer, effect);
    }
}
```

---

### 4. 输入管理器 (Input Manager)

#### 数据结构

```rust
/// 输入事件
pub enum InputEvent {
    Keyboard(KeyEvent),
    Mouse(MouseEvent),
    Touch(TouchEvent),
}

/// 键盘事件
pub struct KeyEvent {
    pub key: u32,
    pub state: KeyState,
    pub modifiers: Modifiers,
}

/// 鼠标事件
pub struct MouseEvent {
    pub x: i32,
    pub y: i32,
    pub button: MouseButton,
    pub state: ButtonState,
}

/// 输入管理器
pub struct InputManager {
    focus_window: Option<WindowId>,
    pointer_position: Point,
    grab: Option<Grab>,
}
```

#### 核心API

```rust
impl InputManager {
    /// 处理输入事件
    pub fn handle_event(
        &mut self,
        event: InputEvent,
        wm: &WindowManager,
    );
    
    /// 设置焦点窗口
    pub fn set_focus(
        &mut self,
        window_id: WindowId,
    );
    
    /// 抓取输入（用于拖动窗口等）
    pub fn grab_input(
        &mut self,
        window_id: WindowId,
        grab_type: GrabType,
    );
}
```

#### 事件路由

```rust
/// 路由输入事件
fn route_input_event(
    input: &mut InputManager,
    event: InputEvent,
    wm: &WindowManager,
    clients: &mut HashMap<ClientId, Client>,
) {
    match event {
        InputEvent::Keyboard(key) => {
            // 发送到焦点窗口
            if let Some(focus) = input.focus_window {
                if let Some(window) = wm.get_window(focus) {
                    if let Some(client) = clients.get_mut(&window.client) {
                        client.send_event(InputEvent::Keyboard(key));
                    }
                }
            }
        }
        
        InputEvent::Mouse(mouse) => {
            // 更新指针位置
            input.pointer_position = Point::new(mouse.x, mouse.y);
            
            // 查找鼠标下的窗口
            let window = wm.find_window_at(mouse.x, mouse.y);
            
            // 发送事件
            if let Some(window) = window {
                if let Some(client) = clients.get_mut(&window.client) {
                    client.send_event(InputEvent::Mouse(mouse));
                }
            }
        }
        
        _ => {}
    }
}
```

---

## 🔌 Hymenoptera协议

### 协议设计

#### 消息类型

```rust
/// 客户端到服务器的消息
pub enum ClientMessage {
    // 连接管理
    Connect { 
        pwid: u64,
        session_id: Option<SessionId>,
    },
    Disconnect,
    
    // 会话管理
    CreateSession,
    SwitchSession { session_id: SessionId },
    CloseSession { session_id: SessionId },
    
    // 窗口管理
    CreateWindow {
        width: u32,
        height: u32,
        title: String,
    },
    DestroyWindow { window_id: WindowId },
    MoveWindow { window_id: WindowId, x: i32, y: i32 },
    ResizeWindow { window_id: WindowId, width: u32, height: u32 },
    
    // 绘制
    AttachBuffer {
        window_id: WindowId,
        buffer: SharedBuffer,
    },
    Commit {
        window_id: WindowId,
        damage: Vec<Rectangle>,
    },
    
    // 输入
    SetCursor { cursor: Cursor },
}

/// 服务器到客户端的消息
pub enum ServerMessage {
    // 连接响应
    Connected { client_id: ClientId },
    Disconnected,
    
    // 会话事件
    SessionCreated { session_id: SessionId },
    SessionSwitched { session_id: SessionId },
    SessionClosed { session_id: SessionId },
    
    // 窗口事件
    WindowCreated { window_id: WindowId },
    WindowDestroyed { window_id: WindowId },
    WindowConfigured { window_id: WindowId, config: WindowConfig },
    
    // 输入事件
    KeyEvent { key: KeyEvent },
    MouseEvent { mouse: MouseEvent },
    TouchEvent { touch: TouchEvent },
    
    // 错误
    Error { code: ErrorCode, message: String },
}
```

---

### IPC机制

#### 共享内存

```rust
/// 共享缓冲区
pub struct SharedBuffer {
    pub id: BufferId,
    pub size: usize,
    pub fd: i32,  // 文件描述符（用于共享）
    pub offset: usize,
}

/// 创建共享缓冲区
pub fn create_shared_buffer(size: usize) -> Result<SharedBuffer, Error> {
    // 使用内核的共享内存机制
    let shm = shm_create(size)?;
    
    Ok(SharedBuffer {
        id: shm.id,
        size,
        fd: shm.fd,
        offset: 0,
    })
}
```

#### 消息传递

```rust
/// 客户端连接
pub struct ClientConnection {
    pub client_id: ClientId,
    pub socket: UnixSocket,  // 或使用AntX的IPC机制
    pub shared_buffers: HashMap<BufferId, SharedBuffer>,
}

/// 发送消息
pub fn send_message(
    conn: &mut ClientConnection,
    msg: ServerMessage,
) -> Result<(), Error> {
    // 序列化消息
    let data = serialize(&msg)?;
    
    // 发送
    conn.socket.send(&data)?;
    
    Ok(())
}

/// 接收消息
pub fn recv_message(
    conn: &mut ClientConnection,
) -> Result<ClientMessage, Error> {
    // 接收数据
    let data = conn.socket.recv()?;
    
    // 反序列化
    let msg = deserialize(&data)?;
    
    Ok(msg)
}
```

---

## 🔒 安全设计

### PWID权限检查

```rust
/// 检查窗口访问权限
pub fn check_window_access(
    pwid: u64,
    window: &Window,
) -> bool {
    // 检查是否是窗口所属会话的用户
    let session = get_session(window.session);
    
    if session.user.pwid == pwid {
        return true;
    }
    
    // 检查是否有特权
    if unsafe { pwid_get_privilege_level(pwid) } == 0 {
        return true;
    }
    
    false
}

/// 检查会话访问权限
pub fn check_session_access(
    pwid: u64,
    session: &Session,
) -> bool {
    // 只有会话所属用户或特权用户可以访问
    session.pwid == pwid || unsafe { pwid_get_privilege_level(pwid) } == 0
}
```

---

### 资源隔离

```rust
/// 会话资源限制
pub struct SessionResources {
    pub max_windows: usize,      // 最大窗口数
    pub max_memory: usize,       // 最大内存
    pub max_buffers: usize,      // 最大缓冲区数
}

impl Default for SessionResources {
    fn default() -> Self {
        Self {
            max_windows: 32,
            max_memory: 64 * 1024 * 1024,  // 64MB
            max_buffers: 16,
        }
    }
}

/// 检查资源限制
pub fn check_resource_limit(
    session: &Session,
    resource_type: ResourceType,
) -> Result<(), Error> {
    let limits = &session.resource_limits;
    
    match resource_type {
        ResourceType::Window => {
            if session.windows.len() >= limits.max_windows {
                return Err(Error::ResourceLimitExceeded);
            }
        }
        ResourceType::Memory(size) => {
            if session.memory_usage + size > limits.max_memory {
                return Err(Error::ResourceLimitExceeded);
            }
        }
        _ => {}
    }
    
    Ok(())
}
```

---

## 🎨 渲染流程

### 完整流程

```
1. 客户端绘制
   ├─ 客户端在共享缓冲区绘制
   └─ 发送Commit消息（带损坏区域）

2. 服务器合成
   ├─ 接收Commit消息
   ├─ 更新窗口表面
   ├─ 合成所有可见窗口
   ├─ 应用特效
   └─ 输出到framebuffer

3. 显示输出
   ├─ 双缓冲交换
   └─ 硬件刷新
```

### 性能优化

```rust
/// 损坏区域优化
pub fn composite_optimized(
    compositor: &mut Compositor,
    windows: &[Arc<Window>],
    damage: &[Rectangle],
) {
    // 只重绘损坏区域
    for region in damage {
        // 找出覆盖该区域的所有窗口
        let affected_windows: Vec<_> = windows
            .iter()
            .filter(|w| w.visible && w.overlaps(region))
            .collect();
        
        // 只合成该区域
        for window in affected_windows {
            blend_region(
                &mut compositor.buffer,
                &window.surface,
                region,
            );
        }
    }
}
```

---

## 📊 性能指标

### 目标性能

| 指标 | 目标值 | 说明 |
|------|--------|------|
| 合成延迟 | < 16ms | 60fps |
| 输入延迟 | < 5ms | 低延迟 |
| 内存占用 | < 10MB | 服务器本身 |
| CPU占用 | < 5% | 空闲时 |
| 启动时间 | < 100ms | 快速启动 |

---

## 🚀 实施计划

### 阶段1：基础框架（2-3周）

**任务**：
- [ ] 设计核心数据结构
- [ ] 实现基础IPC机制
- [ ] 实现简单的窗口管理
- [ ] 基础合成器

**交付物**：能显示单个窗口

---

### 阶段2：会话管理（2周）

**任务**：
- [ ] 实现会话管理器
- [ ] 多用户支持
- [ ] 会话切换
- [ ] 安全隔离

**交付物**：支持多用户登录

---

### 阶段3：输入系统（1周）

**任务**：
- [ ] 输入事件路由
- [ ] 焦点管理
- [ ] 键盘/鼠标支持

**交付物**：完整的输入处理

---

### 阶段4：优化完善（2周）

**任务**：
- [ ] 性能优化
- [ ] 损坏区域跟踪
- [ ] 特效支持
- [ ] 测试和调试

**交付物**：性能达标的系统

---

## 🎨 LVGL集成方案

### 概述

LVGL（Light and Versatile Graphics Library）是一个开源的嵌入式图形库，具有以下特点：
- 轻量级、高性能
- 丰富的控件库
- 支持多种输入设备
- 低内存占用
- 硬件加速支持

**Hymenoptera与LVGL的结合**将为AntX提供完整的图形化解决方案。

---

### 集成架构

```
┌─────────────────────────────────────────────────────────┐
│                    应用层                                │
│  ┌──────────────────────────────────────────────────┐  │
│  │              LVGL 控件层                          │  │
│  │  ┌─────┐ ┌─────┐ ┌─────┐ ┌─────┐               │  │
│  │  │Button│ │Label│ │List │ │Chart│ ...          │  │
│  │  └─────┘ └─────┘ └─────┘ └─────┘               │  │
│  └──────────────────────────────────────────────────┘  │
└─────────────────────────────────────────────────────────┘
                        ↕
┌─────────────────────────────────────────────────────────┐
│                    LVGL核心层                            │
│  ┌──────────────────────────────────────────────────┐  │
│  │  渲染引擎 │ 事件系统 │ 动画系统 │ 布局系统       │  │
│  └──────────────────────────────────────────────────┘  │
└─────────────────────────────────────────────────────────┘
                        ↕
┌─────────────────────────────────────────────────────────┐
│                 Hymenoptera适配层                        │
│  ┌──────────┐  ┌──────────┐  ┌──────────┐              │
│  │ 显示驱动 │  │ 输入驱动 │  │ 内存管理 │              │
│  │ 适配器   │  │  适配器  │  │  适配器  │              │
│  └──────────┘  └──────────┘  └──────────┘              │
└─────────────────────────────────────────────────────────┘
                        ↕
┌─────────────────────────────────────────────────────────┐
│              Hymenoptera显示服务器                       │
│  ┌──────────┐  ┌──────────┐  ┌──────────┐              │
│  │ 窗口管理 │  │ 输入管理 │  │ 合成器   │              │
│  └──────────┘  └──────────┘  └──────────┘              │
└─────────────────────────────────────────────────────────┘
```

---

### 1. 显示驱动适配器

#### 数据结构

```rust
/// LVGL显示驱动适配器
pub struct LvglDisplayAdapter {
    window_id: WindowId,
    connection: Arc<HymenopteraConnection>,
    buffer: SharedBuffer,
    width: u32,
    height: u32,
    flush_pending: bool,
}

/// LVGL显示描述符
pub struct LvglDisplayDescriptor {
    pub width: u32,
    pub height: u32,
    pub draw_buf: *mut lv_color_t,
    pub draw_buf_size: usize,
    pub flush_cb: Option<unsafe extern "C" fn(*mut lv_disp_drv_t, *const lv_area_t, *mut lv_color_t)>,
    pub wait_cb: Option<unsafe extern "C" fn(*mut lv_disp_drv_t)>,
}
```

#### 实现代码

```rust
impl LvglDisplayAdapter {
    pub fn new(
        connection: Arc<HymenopteraConnection>,
        width: u32,
        height: u32,
    ) -> Result<Self, Error> {
        let window = connection.create_window(width, height, "LVGL Window")?;
        let buffer = connection.get_buffer(window)?;
        
        Ok(Self {
            window_id: window,
            connection,
            buffer,
            width,
            height,
            flush_pending: false,
        })
    }
    
    pub fn init_lvgl_display(&mut self) -> lv_disp_drv_t {
        let mut disp_drv: lv_disp_drv_t = unsafe { std::mem::zeroed() };
        
        unsafe {
            lv_disp_drv_init(&mut disp_drv);
            
            disp_drv.hor_res = self.width as i16;
            disp_drv.ver_res = self.height as i16;
            disp_drv.flush_cb = Some(lvgl_flush_cb);
            disp_drv.user_data = self as *mut _ as *mut c_void;
            
            lv_disp_drv_register(&mut disp_drv);
        }
        
        disp_drv
    }
}

unsafe extern "C" fn lvgl_flush_cb(
    disp_drv: *mut lv_disp_drv_t,
    area: *const lv_area_t,
    color_p: *mut lv_color_t,
) {
    let adapter = (*disp_drv).user_data as *mut LvglDisplayAdapter;
    let adapter = &mut *adapter;
    
    let x1 = (*area).x1 as u32;
    let y1 = (*area).y1 as u32;
    let x2 = (*area).x2 as u32;
    let y2 = (*area).y2 as u32;
    
    let width = x2 - x1 + 1;
    let height = y2 - y1 + 1;
    
    let src = color_p as *const u8;
    let dst = adapter.buffer.ptr.add((y1 * adapter.width + x1) as usize * 4) as *mut u8;
    
    for y in 0..height {
        let src_row = src.add((y * width * 4) as usize);
        let dst_row = dst.add((y * adapter.width * 4) as usize);
        std::ptr::copy_nonoverlapping(src_row, dst_row, (width * 4) as usize);
    }
    
    let damage = Rectangle {
        x: x1 as i32,
        y: y1 as i32,
        width: width as i32,
        height: height as i32,
    };
    
    if let Err(_) = adapter.connection.commit(adapter.window_id, &[damage]) {
    }
    
    lv_disp_flush_ready(disp_drv);
}
```

---

### 2. 输入驱动适配器

#### 数据结构

```rust
/// LVGL输入驱动适配器
pub struct LvglInputAdapter {
    connection: Arc<HymenopteraConnection>,
    indev_drv: lv_indev_drv_t,
    last_state: InputState,
}

#[derive(Default)]
struct InputState {
    key: u32,
    key_pressed: bool,
    mouse_x: i32,
    mouse_y: i32,
    mouse_pressed: bool,
}

/// LVGL输入设备类型
pub enum LvglInputType {
    Keyboard,
    Mouse,
    Touchpad,
    Encoder,
    Button,
}
```

#### 实现代码

```rust
impl LvglInputAdapter {
    pub fn new(
        connection: Arc<HymenopteraConnection>,
        input_type: LvglInputType,
    ) -> Self {
        let mut indev_drv: lv_indev_drv_t = unsafe { std::mem::zeroed() };
        
        unsafe {
            lv_indev_drv_init(&mut indev_drv);
            
            match input_type {
                LvglInputType::Keyboard => {
                    indev_drv.type_ = LV_INDEV_TYPE_KEYPAD;
                    indev_drv.read_cb = Some(lvgl_keyboard_read_cb);
                }
                LvglInputType::Mouse => {
                    indev_drv.type_ = LV_INDEV_TYPE_POINTER;
                    indev_drv.read_cb = Some(lvgl_mouse_read_cb);
                }
                LvglInputType::Touchpad => {
                    indev_drv.type_ = LV_INDEV_TYPE_POINTER;
                    indev_drv.read_cb = Some(lvgl_touchpad_read_cb);
                }
                _ => {}
            }
            
            indev_drv.user_data = std::ptr::null_mut();
        }
        
        Self {
            connection,
            indev_drv,
            last_state: InputState::default(),
        }
    }
    
    pub fn register(&mut self) {
        unsafe {
            self.indev_drv.user_data = self as *mut _ as *mut c_void;
            lv_indev_drv_register(&mut self.indev_drv);
        }
    }
    
    pub fn handle_event(&mut self, event: &ServerMessage) {
        match event {
            ServerMessage::KeyEvent { key } => {
                self.last_state.key = key.key;
                self.last_state.key_pressed = key.state == KeyState::Pressed;
            }
            ServerMessage::MouseEvent { mouse } => {
                self.last_state.mouse_x = mouse.x;
                self.last_state.mouse_y = mouse.y;
                self.last_state.mouse_pressed = mouse.state == ButtonState::Pressed;
            }
            _ => {}
        }
    }
}

unsafe extern "C" fn lvgl_keyboard_read_cb(
    indev_drv: *mut lv_indev_drv_t,
    data: *mut lv_indev_data_t,
) {
    let adapter = (*indev_drv).user_data as *mut LvglInputAdapter;
    let adapter = &*adapter;
    
    (*data).key = adapter.last_state.key as u32;
    (*data).state = if adapter.last_state.key_pressed {
        LV_INDEV_STATE_PR
    } else {
        LV_INDEV_STATE_REL
    };
}

unsafe extern "C" fn lvgl_mouse_read_cb(
    indev_drv: *mut lv_indev_drv_t,
    data: *mut lv_indev_data_t,
) {
    let adapter = (*indev_drv).user_data as *mut LvglInputAdapter;
    let adapter = &*adapter;
    
    (*data).point.x = adapter.last_state.mouse_x as i16;
    (*data).point.y = adapter.last_state.mouse_y as i16;
    (*data).state = if adapter.last_state.mouse_pressed {
        LV_INDEV_STATE_PR
    } else {
        LV_INDEV_STATE_REL
    };
}
```

---

### 3. 内存管理适配器

#### 数据结构

```rust
/// LVGL内存管理适配器
pub struct LvglMemoryAdapter {
    heap: Vec<u8>,
    used: usize,
    total: usize,
}

/// LVGL内存描述符
pub struct LvglMemoryDescriptor {
    pub buf: *mut u8,
    pub buf_size: usize,
}
```

#### 实现代码

```rust
impl LvglMemoryAdapter {
    pub fn new(size: usize) -> Self {
        let mut heap = vec![0u8; size];
        
        Self {
            heap,
            used: 0,
            total: size,
        }
    }
    
    pub fn init_lvgl_memory(&mut self) {
        unsafe {
            lv_mem_set_heap(self.heap.as_mut_ptr(), self.total);
        }
    }
    
    pub fn allocate(&mut self, size: usize, align: usize) -> Option<*mut u8> {
        let aligned_offset = (self.used + align - 1) & !(align - 1);
        
        if aligned_offset + size > self.total {
            return None;
        }
        
        let ptr = unsafe { self.heap.as_mut_ptr().add(aligned_offset) };
        self.used = aligned_offset + size;
        
        Some(ptr)
    }
    
    pub fn usage(&self) -> (usize, usize) {
        (self.used, self.total)
    }
}

pub unsafe extern "C" fn lvgl_mem_alloc(size: usize) -> *mut c_void {
    ADAPTER.with(|adapter| {
        let mut adapter = adapter.borrow_mut();
        adapter.allocate(size, 8)
            .map(|p| p as *mut c_void)
            .unwrap_or(std::ptr::null_mut())
    })
}

pub unsafe extern "C" fn lvgl_mem_free(ptr: *mut c_void) {
}

pub unsafe extern "C" fn lvgl_mem_realloc(ptr: *mut c_void, size: usize) -> *mut c_void {
    if ptr.is_null() {
        lvgl_mem_alloc(size)
    } else {
        std::ptr::null_mut()
    }
}
```

---

### 4. 多窗口支持

#### 窗口容器

```rust
/// LVGL窗口容器
pub struct LvglWindowContainer {
    windows: HashMap<WindowId, LvglWindow>,
    active_window: Option<WindowId>,
}

/// LVGL窗口
pub struct LvglWindow {
    pub window_id: WindowId,
    pub display: LvglDisplayAdapter,
    pub input: Vec<LvglInputAdapter>,
    pub root_obj: *mut lv_obj_t,
    pub active: bool,
}

impl LvglWindowContainer {
    pub fn new() -> Self {
        Self {
            windows: HashMap::new(),
            active_window: None,
        }
    }
    
    pub fn create_window(
        &mut self,
        connection: Arc<HymenopteraConnection>,
        width: u32,
        height: u32,
    ) -> Result<WindowId, Error> {
        let window_id = connection.create_window(width, height, "LVGL App")?;
        
        let mut display = LvglDisplayAdapter::new(connection.clone(), width, height)?;
        display.init_lvgl_display();
        
        let keyboard = LvglInputAdapter::new(connection.clone(), LvglInputType::Keyboard);
        let mouse = LvglInputAdapter::new(connection.clone(), LvglInputType::Mouse);
        
        let root_obj = unsafe { lv_obj_create(std::ptr::null_mut()) };
        
        let window = LvglWindow {
            window_id,
            display,
            input: vec![keyboard, mouse],
            root_obj,
            active: true,
        };
        
        self.windows.insert(window_id, window);
        self.active_window = Some(window_id);
        
        Ok(window_id)
    }
    
    pub fn switch_window(&mut self, window_id: WindowId) -> Result<(), Error> {
        if let Some(window) = self.windows.get_mut(&window_id) {
            window.active = true;
            self.active_window = Some(window_id);
            Ok(())
        } else {
            Err(Error::WindowNotFound)
        }
    }
    
    pub fn handle_event(&mut self, event: &ServerMessage) {
        if let Some(active_id) = self.active_window {
            if let Some(window) = self.windows.get_mut(&active_id) {
                for input in &mut window.input {
                    input.handle_event(event);
                }
            }
        }
    }
}
```

---

### 5. 多会话支持

#### 会话管理

```rust
/// LVGL会话管理器
pub struct LvglSessionManager {
    sessions: HashMap<SessionId, LvglSession>,
    active_session: Option<SessionId>,
}

/// LVGL会话
pub struct LvglSession {
    pub session_id: SessionId,
    pub user: UserId,
    pub windows: LvglWindowContainer,
    pub memory: LvglMemoryAdapter,
    pub theme: LvglTheme,
}

/// LVGL主题
pub enum LvglTheme {
    Default,
    MaterialLight,
    MaterialDark,
    Mono,
    Custom(*mut lv_theme_t),
}

impl LvglSessionManager {
    pub fn create_session(
        &mut self,
        connection: Arc<HymenopteraConnection>,
        user: UserId,
        pwid: u64,
    ) -> Result<SessionId, Error> {
        let session_id = connection.create_session(user, pwid)?;
        
        let memory = LvglMemoryAdapter::new(2 * 1024 * 1024);
        memory.init_lvgl_memory();
        
        let session = LvglSession {
            session_id,
            user,
            windows: LvglWindowContainer::new(),
            memory,
            theme: LvglTheme::MaterialLight,
        };
        
        self.sessions.insert(session_id, session);
        self.active_session = Some(session_id);
        
        Ok(session_id)
    }
    
    pub fn switch_session(&mut self, session_id: SessionId) -> Result<(), Error> {
        if self.sessions.contains_key(&session_id) {
            self.active_session = Some(session_id);
            Ok(())
        } else {
            Err(Error::SessionNotFound)
        }
    }
    
    pub fn set_theme(&mut self, session_id: SessionId, theme: LvglTheme) {
        if let Some(session) = self.sessions.get_mut(&session_id) {
            session.theme = theme;
            
            unsafe {
                match theme {
                    LvglTheme::Default => lv_theme_default_init(),
                    LvglTheme::MaterialLight => lv_theme_material_init(),
                    LvglTheme::MaterialDark => lv_theme_material_init(),
                    LvglTheme::Mono => lv_theme_mono_init(),
                    LvglTheme::Custom(t) => lv_theme_set_act(t),
                }
            }
        }
    }
}
```

---

### 6. 性能优化

#### 双缓冲优化

```rust
/// 双缓冲LVGL适配器
pub struct DoubleBufferAdapter {
    front_buffer: SharedBuffer,
    back_buffer: SharedBuffer,
    buffer_size: usize,
    swap_pending: bool,
}

impl DoubleBufferAdapter {
    pub fn new(
        connection: &HymenopteraConnection,
        window_id: WindowId,
        width: u32,
        height: u32,
    ) -> Result<Self, Error> {
        let buffer_size = (width * height * 4) as usize;
        
        let front = connection.create_buffer(buffer_size)?;
        let back = connection.create_buffer(buffer_size)?;
        
        Ok(Self {
            front_buffer: front,
            back_buffer: back,
            buffer_size,
            swap_pending: false,
        })
    }
    
    pub fn get_draw_buffer(&self) -> *mut u8 {
        self.back_buffer.ptr
    }
    
    pub fn swap_buffers(&mut self, connection: &HymenopteraConnection, window_id: WindowId) {
        std::mem::swap(&mut self.front_buffer, &mut self.back_buffer);
        self.swap_pending = true;
        
        let damage = Rectangle {
            x: 0,
            y: 0,
            width: self.front_buffer.width as i32,
            height: self.front_buffer.height as i32,
        };
        
        let _ = connection.commit(window_id, &[damage]);
    }
}
```

#### 部分刷新优化

```rust
/// 部分刷新管理器
pub struct PartialRefreshManager {
    damage_regions: Vec<Rectangle>,
    refresh_rate: u32,
    last_refresh: u64,
}

impl PartialRefreshManager {
    pub fn new(refresh_rate: u32) -> Self {
        Self {
            damage_regions: Vec::new(),
            refresh_rate,
            last_refresh: 0,
        }
    }
    
    pub fn add_damage(&mut self, region: Rectangle) {
        self.damage_regions.push(region);
    }
    
    pub fn should_refresh(&self, current_time: u64) -> bool {
        let interval = 1000 / self.refresh_rate as u64;
        current_time - self.last_refresh >= interval && !self.damage_regions.is_empty()
    }
    
    pub fn get_merged_damage(&mut self) -> Vec<Rectangle> {
        if self.damage_regions.is_empty() {
            return Vec::new();
        }
        
        let merged = self.merge_regions();
        self.damage_regions.clear();
        merged
    }
    
    fn merge_regions(&self) -> Vec<Rectangle> {
        let mut merged: Vec<Rectangle> = Vec::new();
        
        for region in &self.damage_regions {
            let mut absorbed = false;
            
            for m in &mut merged {
                if m.contains(region) {
                    absorbed = true;
                    break;
                } else if region.contains(m) {
                    *m = *region;
                    absorbed = true;
                    break;
                } else if m.overlaps(region) {
                    *m = m.merge(region);
                    absorbed = true;
                    break;
                }
            }
            
            if !absorbed {
                merged.push(*region);
            }
        }
        
        merged
    }
}
```

---

### 7. 完整示例

#### 应用初始化

```rust
use hymenoptera::*;
use lvgl::*;

struct LvglApp {
    connection: Arc<HymenopteraConnection>,
    session: LvglSessionManager,
    refresh_manager: PartialRefreshManager,
}

impl LvglApp {
    pub fn new() -> Result<Self, Error> {
        let pwid = get_current_pwid();
        let connection = Arc::new(HymenopteraConnection::connect(pwid)?);
        
        let mut session = LvglSessionManager::new();
        session.create_session(connection.clone(), get_current_user(), pwid)?;
        
        unsafe {
            lv_init();
        }
        
        let refresh_manager = PartialRefreshManager::new(60);
        
        Ok(Self {
            connection,
            session,
            refresh_manager,
        })
    }
    
    pub fn create_ui(&mut self) -> Result<(), Error> {
        let active_session = self.session.active_session.unwrap();
        let session = self.session.sessions.get_mut(&active_session).unwrap();
        
        let window_id = session.windows.create_window(
            self.connection.clone(),
            800,
            600,
        )?;
        
        let window = session.windows.windows.get(&window_id).unwrap();
        let root = window.root_obj;
        
        unsafe {
            let btn = lv_btn_create(root);
            lv_obj_set_size(btn, 120, 50);
            lv_obj_align(btn, LV_ALIGN_CENTER, 0, 0);
            
            let label = lv_label_create(btn);
            lv_label_set_text(label, c"Click Me!".as_ptr());
            
            lv_obj_add_event_cb(
                btn,
                Some(btn_event_cb),
                LV_EVENT_CLICKED,
                std::ptr::null_mut(),
            );
        }
        
        Ok(())
    }
    
    pub fn run(&mut self) -> ! {
        let mut last_tick = get_tick_ms();
        
        loop {
            let current_tick = get_tick_ms();
            
            while let Some(event) = self.connection.recv_event_nonblock()? {
                let active_session = self.session.active_session.unwrap();
                let session = self.session.sessions.get_mut(&active_session).unwrap();
                session.windows.handle_event(&event);
            }
            
            let elapsed = current_tick - last_tick;
            if elapsed > 0 {
                unsafe {
                    lv_tick_inc(elapsed as u32);
                }
                last_tick = current_tick;
            }
            
            unsafe {
                lv_timer_handler();
            }
            
            if self.refresh_manager.should_refresh(current_tick) {
                let damage = self.refresh_manager.get_merged_damage();
                if !damage.is_empty() {
                    let active_session = self.session.active_session.unwrap();
                    let session = self.session.sessions.get(&active_session).unwrap();
                    let active_window = session.windows.active_window.unwrap();
                    
                    self.connection.commit(active_window, &damage)?;
                }
            }
            
            std::thread::sleep(std::time::Duration::from_millis(1));
        }
    }
}

unsafe extern "C" fn btn_event_cb(
    e: *mut lv_event_t,
) {
    let obj = lv_event_get_target(e);
    
    let label = lv_obj_get_child(obj, 0);
    lv_label_set_text(label, c"Clicked!".as_ptr());
}

fn main() -> Result<(), Error> {
    let mut app = LvglApp::new()?;
    app.create_ui()?;
    app.run();
}
```

---

### 8. 配置选项

#### LVGL配置

```rust
pub struct LvglConfig {
    pub memory_size: usize,
    pub draw_buffer_size: usize,
    pub refresh_rate: u32,
    pub use_double_buffer: bool,
    pub use_partial_refresh: bool,
    pub use_gpu_accel: bool,
    pub default_theme: LvglTheme,
    pub font_default: *mut lv_font_t,
    pub font_small: *mut lv_font_t,
    pub font_large: *mut lv_font_t,
}

impl Default for LvglConfig {
    fn default() -> Self {
        Self {
            memory_size: 2 * 1024 * 1024,
            draw_buffer_size: 800 * 600 * 4,
            refresh_rate: 60,
            use_double_buffer: true,
            use_partial_refresh: true,
            use_gpu_accel: false,
            default_theme: LvglTheme::MaterialLight,
            font_default: unsafe { &lv_font_montserrat_14 as *const _ as *mut _ },
            font_small: unsafe { &lv_font_montserrat_12 as *const _ as *mut _ },
            font_large: unsafe { &lv_font_montserrat_20 as *const _ as *mut _ },
        }
    }
}
```

---

### 9. 性能指标

#### LVGL集成性能目标

| 指标 | 目标值 | 说明 |
|------|--------|------|
| 初始化时间 | < 50ms | LVGL初始化 |
| 内存占用 | < 3MB | LVGL核心 + 缓冲 |
| 渲染延迟 | < 10ms | 单帧渲染 |
| 输入响应 | < 5ms | 事件处理 |
| 控件数量 | > 100 | 单窗口支持 |
| 动画帧率 | 60fps | 流畅动画 |

---

## 📝 使用示例

### 客户端示例

```rust
// 连接到Hymenoptera
let conn = hymenoptera_connect(pwid)?;

// 创建窗口
let window = conn.create_window(800, 600, "My App")?;

// 获取共享缓冲区
let buffer = conn.get_buffer(window)?;

// 使用LVGL绘制
let mut lvgl = Lvgl::new(buffer);
lvgl.init();

// 主循环
loop {
    // 处理事件
    while let Some(event) = conn.recv_event()? {
        match event {
            ServerMessage::KeyEvent { key } => {
                // 处理键盘事件
            }
            ServerMessage::MouseEvent { mouse } => {
                // 处理鼠标事件
            }
            _ => {}
        }
    }
    
    // 绘制
    lvgl.update();
    
    // 提交
    conn.commit(window, &damage)?;
}
```

---

## 🔮 未来扩展

### 可能的扩展

1. **远程显示**
   - 网络透明
   - 远程会话
   - 类似VNC/RDP

2. **多显示器**
   - 多输出支持
   - 显示器热插拔

3. **高级特效**
   - 窗口动画
   - 半透明
   - 阴影

4. **硬件加速**
   - GPU合成
   - 硬件光标

---

## 📚 参考资料

### 相关项目

- **Wayland**: 现代Linux显示协议
- **X11**: 传统X Window System
- **Mir**: Ubuntu的显示服务器
- **SurfaceFlinger**: Android的合成器

### 学习资源

- Wayland协议规范
- Compositor设计原理
- 图形系统架构

---

**最后更新**: 2026-05-18  
**状态**: 设计阶段  
**下一步**: 开始实现基础框架

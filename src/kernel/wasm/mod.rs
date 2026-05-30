//! WASM 虚拟机子系统
//!
//! 实现 WebAssembly 1.0 核心规范的解释器引擎。
//!
//! ## 模块结构
//!
//! ```text
//! wasm/
//! ├── mod.rs          # 模块导出
//! ├── types.rs        # 类型定义 (Value, Opcode, WasmModule, 各 Section 结构)
//! ├── leb128.rs       # LEB128 变长整数编解码
//! ├── module.rs       # WASM 二进制格式解析器 (11 Section 解析)
//! ├── runtime.rs      # 运行时数据结构 (ValueStack, CallFrame, LinearMemory)
//! └── interpreter.rs  # 栈式虚拟机核心解释器
//! ```

pub mod types;
pub mod leb128;
pub mod module;
pub mod runtime;
pub mod interpreter;
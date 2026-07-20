#![deny(unsafe_code)]
//! @SAFE: 本文件不含 unsafe 代码。纯数据结构定义。
//! 运行时数据结构 — services 层策略主体
//!
//! ## T6-9 迁移记录
//!
//! 原属 framework/wasm/runtime.rs, 2026-06-16 提取到 services.
//! 纯数据结构 (值栈/调用帧/线性内存), 0 unsafe, 0 外部依赖.
//! framework 仅保留 re-export.
//!
//! 包含:
//! - ValueStack: 值栈, 栈式虚拟机的核心数据结构
//! - CallFrame: 调用帧, 记录函数调用的上下文
//! - LinearMemory: 线性内存, WASM 规范定义的主内存抽象

use alloc::vec;
use alloc::vec::Vec;

use super::types::*;

// ============================================================================
// 值栈
// ============================================================================

pub struct ValueStack {
    pub data: Vec<Value>,
}

impl ValueStack {
    pub fn new() -> Self {
        Self {
            data: Vec::with_capacity(256),
        }
    }

    pub fn push(&mut self, v: Value) -> Result<(), WasmError> {
        if self.data.len() >= 4096 {
            return Err(WasmError::StackOverflow);
        }
        self.data.push(v);
        Ok(())
    }

    pub fn pop(&mut self) -> Result<Value, WasmError> {
        self.data.pop().ok_or(WasmError::StackUnderflow)
    }

    pub fn pop_i32(&mut self) -> Result<i32, WasmError> {
        match self.pop()? {
            Value::I32(v) => Ok(v),
            _ => Err(WasmError::TypeMismatch),
        }
    }

    pub fn pop_i64(&mut self) -> Result<i64, WasmError> {
        match self.pop()? {
            Value::I64(v) => Ok(v),
            _ => Err(WasmError::TypeMismatch),
        }
    }

    pub fn peek(&self) -> Option<&Value> {
        self.data.last()
    }

    pub fn len(&self) -> usize {
        self.data.len()
    }

    pub fn is_empty(&self) -> bool {
        self.data.is_empty()
    }

    pub fn drain_to(&mut self, new_len: usize) {
        self.data.truncate(new_len);
    }

    pub fn clear(&mut self) {
        self.data.clear();
    }
}

// ============================================================================
// 调用帧
// ============================================================================

pub struct CallFrame {
    pub func_idx: u32,
    pub locals: Vec<Value>,
    pub pc: usize,
    pub code: Vec<u8>,
    pub arity: usize,
    pub return_pc: usize,
    pub stack_base: usize,
}

impl CallFrame {
    pub fn read_u8(&self) -> Option<u8> {
        if self.pc < self.code.len() {
            Some(self.code[self.pc])
        } else {
            None
        }
    }
}

// ============================================================================
// 线性内存
// ============================================================================

pub struct LinearMemory {
    pub data: Vec<u8>,
    pub max_pages: Option<u32>,
}

impl LinearMemory {
    pub fn new(initial_pages: u32, max_pages: Option<u32>) -> Result<Self, WasmError> {
        let size = initial_pages as usize * WASM_PAGE_SIZE as usize;
        Ok(Self {
            data: vec![0u8; size],
            max_pages,
        })
    }

    pub fn grow(&mut self, additional_pages: u32) -> Result<u32, WasmError> {
        let current_pages = (self.data.len() / WASM_PAGE_SIZE as usize) as u32;
        let new_pages = current_pages + additional_pages;
        if let Some(max) = self.max_pages {
            if new_pages > max {
                return Ok(u32::MAX);
            }
        }
        self.data
            .resize(new_pages as usize * WASM_PAGE_SIZE as usize, 0);
        Ok(current_pages)
    }

    pub fn pages(&self) -> u32 {
        (self.data.len() / WASM_PAGE_SIZE as usize) as u32
    }

    fn check_access(&self, offset: u32, size: u32) -> Result<usize, WasmError> {
        let end = offset as usize + size as usize;
        if end > self.data.len() {
            return Err(WasmError::MemoryOutOfBounds);
        }
        Ok(offset as usize)
    }

    pub fn read_u8(&self, offset: u32) -> Result<u8, WasmError> {
        let addr = self.check_access(offset, 1)?;
        Ok(self.data[addr])
    }

    pub fn read_u16(&self, offset: u32) -> Result<u16, WasmError> {
        let addr = self.check_access(offset, 2)?;
        Ok(u16::from_le_bytes([self.data[addr], self.data[addr + 1]]))
    }

    pub fn read_u32(&self, offset: u32) -> Result<u32, WasmError> {
        let addr = self.check_access(offset, 4)?;
        Ok(u32::from_le_bytes([
            self.data[addr],
            self.data[addr + 1],
            self.data[addr + 2],
            self.data[addr + 3],
        ]))
    }

    pub fn read_u64(&self, offset: u32) -> Result<u64, WasmError> {
        let addr = self.check_access(offset, 8)?;
        Ok(u64::from_le_bytes([
            self.data[addr],
            self.data[addr + 1],
            self.data[addr + 2],
            self.data[addr + 3],
            self.data[addr + 4],
            self.data[addr + 5],
            self.data[addr + 6],
            self.data[addr + 7],
        ]))
    }

    pub fn write_u8(&mut self, offset: u32, value: u8) -> Result<(), WasmError> {
        let addr = self.check_access(offset, 1)?;
        self.data[addr] = value;
        Ok(())
    }

    pub fn write_u16(&mut self, offset: u32, value: u16) -> Result<(), WasmError> {
        let addr = self.check_access(offset, 2)?;
        let bytes = value.to_le_bytes();
        self.data[addr] = bytes[0];
        self.data[addr + 1] = bytes[1];
        Ok(())
    }

    pub fn write_u32(&mut self, offset: u32, value: u32) -> Result<(), WasmError> {
        let addr = self.check_access(offset, 4)?;
        let bytes = value.to_le_bytes();
        self.data[addr..addr + 4].copy_from_slice(&bytes);
        Ok(())
    }

    pub fn write_u64(&mut self, offset: u32, value: u64) -> Result<(), WasmError> {
        let addr = self.check_access(offset, 8)?;
        let bytes = value.to_le_bytes();
        self.data[addr..addr + 8].copy_from_slice(&bytes);
        Ok(())
    }

    /// 获取可写切片 (安全包装, 供 WASI 模块使用)
    ///
    /// 返回指向 WASM 线性内存 `[offset, offset+len)` 的可写切片。
    /// 调用方无需 unsafe。
    pub fn get_slice_mut(&mut self, offset: u32, len: u32) -> Result<&mut [u8], WasmError> {
        let addr = self.check_access(offset, len)?;
        Ok(&mut self.data[addr..addr + len as usize])
    }

    /// 获取只读切片 (安全包装, 供 WASI 模块使用)
    ///
    /// 返回指向 WASM 线性内存 `[offset, offset+len)` 的只读切片。
    /// 调用方无需 unsafe。
    pub fn get_slice(&self, offset: u32, len: u32) -> Result<&[u8], WasmError> {
        let addr = self.check_access(offset, len)?;
        Ok(&self.data[addr..addr + len as usize])
    }
}

// ============================================================================
// 解释器配置
// ============================================================================

pub struct InterpreterConfig {
    pub max_call_depth: u32,
    pub max_gas: u64,
    pub max_memory_pages: u32,
}

impl Default for InterpreterConfig {
    fn default() -> Self {
        Self {
            max_call_depth: 256,
            max_gas: 10_000_000,
            max_memory_pages: 256,
        }
    }
}

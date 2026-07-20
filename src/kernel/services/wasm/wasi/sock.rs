#![deny(unsafe_code)]
//! WASI Socket: sock_accept, sock_connect, sock_recv, sock_send

use crate::kernel::services::wasm::types::{Value, WasmError};
use crate::kernel::services::wasm::interpreter::Interpreter;
use super::{WasiContext, wasi_success, wasi_errno, WasiErrno};
use super::fd_table::{WasiIoVec, read_iovec_from_memory, write_u32_to_memory, write_i32_to_memory};

/// WASI sock_accept: 接受连接
pub fn wasi_sock_accept(_ctx: &mut WasiContext, interp: &mut Interpreter) -> Result<(), WasmError> {
    let fd = interp.stack.pop_i32()? as u32;
    let _flags = interp.stack.pop_i32()? as u32;
    let _addr_ptr = interp.stack.pop_i32()? as u32;
    let _addr_len_ptr = interp.stack.pop_i32()? as u32;

    // TODO: 调用 services::net accept
    // 当前简化: 返回错误
    let _ = fd;
    interp.stack.push(Value::I32(wasi_errno(WasiErrno::Notsup)))?;
    Ok(())
}

/// WASI sock_connect: 连接到地址
pub fn wasi_sock_connect(_ctx: &mut WasiContext, interp: &mut Interpreter) -> Result<(), WasmError> {
    let fd = interp.stack.pop_i32()? as u32;
    let _addr_ptr = interp.stack.pop_i32()? as u32;
    let _addr_len = interp.stack.pop_i32()? as u32;

    // TODO: 调用 services::net connect
    let _ = fd;
    interp.stack.push(Value::I32(wasi_errno(WasiErrno::Notsup)))?;
    Ok(())
}

/// WASI sock_recv: 从 socket 接收数据
pub fn wasi_sock_recv(_ctx: &mut WasiContext, interp: &mut Interpreter) -> Result<(), WasmError> {
    let fd = interp.stack.pop_i32()? as u32;
    let iovs_ptr = interp.stack.pop_i32()? as u32;
    let iovs_len = interp.stack.pop_i32()? as u32;
    let _flags = interp.stack.pop_i32()? as u32;
    let nread_ptr = interp.stack.pop_i32()? as u32;
    let _roflags_ptr = interp.stack.pop_i32()? as u32;

    let _iovecs = read_iovec_from_memory(interp, iovs_ptr, iovs_len)?;

    // TODO: 调用 services::net recv
    let _ = fd;
    write_u32_to_memory(interp, nread_ptr, 0);
    write_i32_to_memory(interp, _roflags_ptr, 0);
    interp.stack.push(Value::I32(wasi_errno(WasiErrno::Notsup)))?;
    Ok(())
}

/// WASI sock_send: 向 socket 发送数据
pub fn wasi_sock_send(_ctx: &mut WasiContext, interp: &mut Interpreter) -> Result<(), WasmError> {
    let fd = interp.stack.pop_i32()? as u32;
    let iovs_ptr = interp.stack.pop_i32()? as u32;
    let iovs_len = interp.stack.pop_i32()? as u32;
    let _flags = interp.stack.pop_i32()? as u32;
    let nwritten_ptr = interp.stack.pop_i32()? as u32;

    let _iovecs = read_iovec_from_memory(interp, iovs_ptr, iovs_len)?;

    // TODO: 调用 services::net send
    let _ = fd;
    write_u32_to_memory(interp, nwritten_ptr, 0);
    interp.stack.push(Value::I32(wasi_errno(WasiErrno::Notsup)))?;
    Ok(())
}

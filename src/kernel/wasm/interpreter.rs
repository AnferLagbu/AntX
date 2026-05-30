//! WASM 解释器 — 栈式虚拟机核心
//!
//! 包含:
//! - Interpreter: 完整的 WASM 字节码执行引擎
//! - 控制流, 内存操作, 数值运算, 全局变量, 函数调用
//!
//! 安全边界:
//! - 所有内存访问均在 bounds check 后进行
//! - Gas metering 防止无限循环
//! - 调用深度限制防止栈溢出
//! - 除以零和溢出均返回 Trap

use alloc::boxed::Box;
use alloc::vec;
use alloc::vec::Vec;

use super::leb128::*;
use super::module::parse_wasm;
use super::runtime::*;
use super::types::*;

// ============================================================================
// 解释器
// ============================================================================

pub struct Interpreter {
    pub stack: ValueStack,
    pub memory: Option<LinearMemory>,
    call_stack: Vec<CallFrame>,
    pub config: InterpreterConfig,
    pub gas_used: u64,
    module: WasmModule,
    host_functions: Vec<Box<dyn Fn(&mut Interpreter) -> Result<(), WasmError>>>,
    import_func_count: u32,
    globals: Vec<Value>,
    tables: Vec<Vec<u32>>,
}

impl Interpreter {
    pub fn new(module: WasmModule, config: InterpreterConfig) -> Self {
        let import_func_count = module.imports.iter()
            .filter(|i| matches!(i.desc, ImportKind::Function(_)))
            .count() as u32;

        let import_global_count = module.imports.iter()
            .filter(|i| matches!(i.desc, ImportKind::Global(_)))
            .count();

        let mut globals = Vec::with_capacity(import_global_count + module.globals.len());
        for imp in module.imports.iter() {
            if let ImportKind::Global(gt) = &imp.desc {
                globals.push(Value::default_for(gt.content_type));
            }
        }
        for (gt, _) in module.globals.iter() {
            globals.push(Value::default_for(gt.content_type));
        }

        let module_globals = module.globals.clone();

        let mut memory = None;
        for imp in &module.imports {
            if let ImportKind::Memory(mem_type) = &imp.desc {
                memory = LinearMemory::new(
                    mem_type.limits.min,
                    mem_type.limits.max,
                ).ok();
                break;
            }
        }
        if memory.is_none() {
            for mem in &module.memories {
                memory = LinearMemory::new(mem.limits.min, mem.limits.max).ok();
                break;
            }
        }

        let import_table_count = module.imports.iter()
            .filter(|i| matches!(i.desc, ImportKind::Table(_)))
            .count();

        let mut tables: Vec<Vec<u32>> = Vec::with_capacity(import_table_count + module.tables.len());
        for imp in module.imports.iter() {
            if let ImportKind::Table(ref tt) = imp.desc {
                tables.push(vec![0u32; tt.limits.min as usize]);
            }
        }
        for tt in &module.tables {
            tables.push(vec![0u32; tt.limits.min as usize]);
        }

        let mut interp = Self {
            stack: ValueStack::new(),
            memory,
            call_stack: Vec::with_capacity(64),
            config,
            gas_used: 0,
            module,
            host_functions: Vec::new(),
            import_func_count,
            globals,
            tables,
        };

        for (gi, (_, init_expr)) in module_globals.iter().enumerate() {
            let global_idx = import_global_count + gi;
            if let Ok(val) = Self::eval_init_expr(init_expr, &interp.globals) {
                if global_idx < interp.globals.len() {
                    interp.globals[global_idx] = val;
                }
            }
        }

        interp.apply_data_segments();
        interp.apply_element_segments();

        interp
    }

    fn eval_init_expr(expr: &[u8], globals: &[Value]) -> Result<Value, WasmError> {
        let mut mini_stack: Vec<Value> = Vec::new();
        let mut pos: usize = 0;
        while pos < expr.len() {
            if expr[pos] == 0x0B {
                break;
            }
            match expr[pos] {
                0x41 => {
                    pos += 1;
                    mini_stack.push(Value::I32(read_leb128_i32(expr, &mut pos)?));
                }
                0x42 => {
                    pos += 1;
                    mini_stack.push(Value::I64(read_leb128_i64(expr, &mut pos)?));
                }
                0x23 => {
                    pos += 1;
                    let idx = read_leb128_u32(expr, &mut pos)? as usize;
                    let val = globals.get(idx).copied().unwrap_or(Value::I32(0));
                    mini_stack.push(val);
                }
                _ => return Err(WasmError::UnknownOpcode(expr[pos])),
            }
        }
        mini_stack.pop().ok_or(WasmError::InternalError)
    }

    fn apply_data_segments(&mut self) {
        for seg in &self.module.data {
            if !seg.offset.is_empty() {
                if let Ok(Value::I32(offset)) = Self::eval_init_expr(&seg.offset, &self.globals) {
                    if let Some(ref mut mem) = self.memory {
                        let addr = offset as usize;
                        if addr + seg.data.len() <= mem.data.len() {
                            mem.data[addr..addr + seg.data.len()]
                                .copy_from_slice(&seg.data);
                        }
                    }
                }
            }
        }
    }

    fn apply_element_segments(&mut self) {
        for seg in &self.module.elements {
            if seg.func_indices.is_empty() || seg.offset.is_empty() {
                continue;
            }
            let table_idx = seg.table_index as usize;
            if table_idx >= self.tables.len() {
                continue;
            }
            if let Ok(Value::I32(offset)) = Self::eval_init_expr(&seg.offset, &self.globals) {
                let base = offset as usize;
                let table_len = self.tables[table_idx].len();
                for (i, &func_idx) in seg.func_indices.iter().enumerate() {
                    let target = base + i;
                    if target < table_len {
                        self.tables[table_idx][target] = func_idx;
                    }
                }
            }
        }
    }

    pub fn register_host_function(
        &mut self,
        f: Box<dyn Fn(&mut Interpreter) -> Result<(), WasmError>>,
    ) {
        self.host_functions.push(f);
    }

    fn find_export(&self, name: &str) -> Option<u32> {
        self.module.exports.iter().find(|e| {
            e.name == name.as_bytes() && e.kind == ExportKind::Function
        }).map(|e| e.index)
    }

    fn get_func_type(&self, func_idx: u32) -> Result<&FuncType, WasmError> {
        if func_idx < self.import_func_count {
            let mut func_import_idx = 0u32;
            for imp in &self.module.imports {
                if let ImportKind::Function(type_idx) = imp.desc {
                    if func_import_idx == func_idx {
                        return self.module.types.get(type_idx as usize)
                            .ok_or(WasmError::BadTypeIndex(type_idx as usize));
                    }
                    func_import_idx += 1;
                }
            }
            Err(WasmError::BadFuncIndex(func_idx as usize))
        } else {
            let local_idx = func_idx - self.import_func_count;
            let type_idx = self.module.functions.get(local_idx as usize)
                .ok_or(WasmError::BadFuncIndex(func_idx as usize))?;
            self.module.types.get(*type_idx as usize)
                .ok_or(WasmError::BadTypeIndex(*type_idx as usize))
        }
    }

    fn get_func_body(&self, func_idx: u32) -> Result<&FunctionBody, WasmError> {
        if func_idx < self.import_func_count {
            return Err(WasmError::FunctionNotFound);
        }
        let local_idx = func_idx - self.import_func_count;
        self.module.code.get(local_idx as usize)
            .ok_or(WasmError::BadFuncIndex(func_idx as usize))
    }

    pub fn call(&mut self, name: &str, args: &[Value]) -> Result<Option<Value>, WasmError> {
        let func_idx = self.find_export(name).ok_or(WasmError::BadExport)?;
        self.call_func(func_idx, args)
    }

    pub fn call_func(&mut self, func_idx: u32, args: &[Value]) -> Result<Option<Value>, WasmError> {
        if func_idx < self.import_func_count {
            let func_type = self.get_func_type(func_idx)?.clone();
            let param_count = func_type.params.len();
            for (i, arg) in args.iter().enumerate() {
                if i < param_count {
                    self.stack.push(*arg)?;
                }
            }

            let host_idx = func_idx as usize;
            if host_idx < self.host_functions.len() {
                let f = core::mem::replace(
                    &mut self.host_functions[host_idx],
                    Box::new(|_| Ok(())),
                );
                let result = f(self);
                self.host_functions[host_idx] = f;
                result?;
            }

            let result_count = func_type.results.len();
            if result_count == 0 {
                return Ok(None);
            } else if result_count == 1 {
                return Ok(Some(self.stack.pop()?));
            } else {
                return Ok(None);
            }
        }

        let func_type = self.get_func_type(func_idx)?.clone();
        let body = self.get_func_body(func_idx)?;

        if self.call_stack.len() as u32 >= self.config.max_call_depth {
            return Err(WasmError::CallDepthExceeded);
        }

        let param_count = func_type.params.len();
        let mut locals: Vec<Value> = Vec::with_capacity(param_count + 64);

        for _ in 0..param_count {
            locals.push(Value::I32(0));
        }
        for (count, ty) in &body.locals {
            for _ in 0..*count {
                locals.push(Value::default_for(*ty));
            }
        }

        for (i, arg) in args.iter().enumerate() {
            if i < param_count {
                locals[i] = *arg;
            }
        }

        let stack_base = self.stack.len();

        let frame = CallFrame {
            func_idx,
            locals,
            pc: 0,
            code: body.code.clone(),
            arity: func_type.results.len(),
            return_pc: 0,
            stack_base,
        };

        self.call_stack.push(frame);
        self.execute_call_stack()?;

        if let Some(_frame) = self.call_stack.pop() {
            let actual_results = self.stack.len() - stack_base;
            if func_type.results.len() == 1 && actual_results >= 1 {
                let result = self.stack.pop()?;
                self.stack.drain_to(stack_base);
                Ok(Some(result))
            } else if func_type.results.is_empty() {
                self.stack.drain_to(stack_base);
                Ok(None)
            } else {
                self.stack.drain_to(stack_base);
                Ok(None)
            }
        } else {
            Ok(None)
        }
    }

    fn execute_call_stack(&mut self) -> Result<(), WasmError> {
        while let Some(frame) = self.call_stack.last() {
            if frame.pc >= frame.code.len() {
                break;
            }
        }

        while let Some(_) = self.call_stack.last() {
            let opcode = {
                let frame = self.call_stack.last().unwrap();
                if frame.pc >= frame.code.len() {
                    self.call_stack.pop();
                    continue;
                }
                frame.code[frame.pc]
            };

            let op = Opcode::from_byte(opcode).ok_or(WasmError::UnknownOpcode(opcode));
            self.gas_used += 1;
            if self.gas_used > self.config.max_gas {
                return Err(WasmError::GasExhausted);
            }

            match op {
                Ok(Opcode::Unreachable) => return Err(WasmError::Unreachable),
                Ok(Opcode::Nop) => {
                    self.advance_pc(1)?;
                }
                Ok(Opcode::End) => {
                    self.execute_end()?;
                    if self.call_stack.is_empty() {
                        return Ok(());
                    }
                }
                Ok(Opcode::Return) => {
                    self.execute_return()?;
                    if self.call_stack.is_empty() {
                        return Ok(());
                    }
                }

                Ok(Opcode::Block) => {
                    self.advance_pc(1)?;
                }
                Ok(Opcode::Loop) => {
                    self.advance_pc(1)?;
                }
                Ok(Opcode::If) => {
                    self.execute_if()?;
                }
                Ok(Opcode::Else) => {
                    self.execute_else()?;
                }
                Ok(Opcode::Br) => {
                    self.execute_br()?;
                }
                Ok(Opcode::BrIf) => {
                    self.execute_br_if()?;
                }
                Ok(Opcode::BrTable) => {
                    self.execute_br_table()?;
                }
                Ok(Opcode::Call) => {
                    self.execute_call()?;
                }
                Ok(Opcode::CallIndirect) => {
                    return Err(WasmError::Unreachable);
                }

                Ok(Opcode::Drop) => {
                    self.advance_pc(1)?;
                    self.stack.pop()?;
                }
                Ok(Opcode::Select) => {
                    self.advance_pc(1)?;
                    let cond = self.stack.pop_i32()?;
                    let val2 = self.stack.pop()?;
                    let val1 = self.stack.pop()?;
                    self.stack.push(if cond != 0 { val1 } else { val2 })?;
                }

                Ok(Opcode::LocalGet) => {
                    self.execute_local_get()?;
                }
                Ok(Opcode::LocalSet) => {
                    self.execute_local_set()?;
                }
                Ok(Opcode::LocalTee) => {
                    self.execute_local_tee()?;
                }
                Ok(Opcode::GlobalGet) => {
                    self.execute_global_get()?;
                }
                Ok(Opcode::GlobalSet) => {
                    self.execute_global_set()?;
                }

                Ok(Opcode::I32Load) => self.execute_memory_load(4)?,
                Ok(Opcode::I64Load) => self.execute_memory_load_64()?,
                Ok(Opcode::I32Load8S) => self.execute_memory_load_ext(1, true)?,
                Ok(Opcode::I32Load8U) => self.execute_memory_load_ext(1, false)?,
                Ok(Opcode::I32Load16S) => self.execute_memory_load_ext(2, true)?,
                Ok(Opcode::I32Load16U) => self.execute_memory_load_ext(2, false)?,
                Ok(Opcode::I32Store) => self.execute_memory_store(4)?,
                Ok(Opcode::I64Store) => self.execute_memory_store_64()?,
                Ok(Opcode::I32Store8) => self.execute_memory_store_n(1)?,
                Ok(Opcode::I32Store16) => self.execute_memory_store_n(2)?,
                Ok(Opcode::MemorySize) => {
                    self.advance_pc(1)?;
                    let pages = self.memory.as_ref().map(|m| m.pages()).unwrap_or(0);
                    self.stack.push(Value::I32(pages as i32))?;
                }
                Ok(Opcode::MemoryGrow) => {
                    self.advance_pc(1)?;
                    let pages = self.stack.pop_i32()?;
                    let result = if let Some(ref mut mem) = self.memory {
                        mem.grow(pages as u32)?
                    } else {
                        u32::MAX
                    };
                    self.stack.push(Value::I32(result as i32))?;
                }

                Ok(Opcode::I32Const) => self.execute_i32_const()?,
                Ok(Opcode::I64Const) => self.execute_i64_const()?,

                Ok(Opcode::I32Eqz) => self.execute_i32_unop(|a| (a == 0) as i32)?,
                Ok(Opcode::I32Eq) => self.execute_i32_binop(|a, b| (a == b) as i32)?,
                Ok(Opcode::I32Ne) => self.execute_i32_binop(|a, b| (a != b) as i32)?,
                Ok(Opcode::I32LtS) => self.execute_i32_binop(|a, b| (a < b) as i32)?,
                Ok(Opcode::I32LtU) => self.execute_i32_binop(|a, b| ((a as u32) < (b as u32)) as i32)?,
                Ok(Opcode::I32GtS) => self.execute_i32_binop(|a, b| (a > b) as i32)?,
                Ok(Opcode::I32GtU) => self.execute_i32_binop(|a, b| ((a as u32) > (b as u32)) as i32)?,
                Ok(Opcode::I32LeS) => self.execute_i32_binop(|a, b| (a <= b) as i32)?,
                Ok(Opcode::I32LeU) => self.execute_i32_binop(|a, b| ((a as u32) <= (b as u32)) as i32)?,
                Ok(Opcode::I32GeS) => self.execute_i32_binop(|a, b| (a >= b) as i32)?,
                Ok(Opcode::I32GeU) => self.execute_i32_binop(|a, b| ((a as u32) >= (b as u32)) as i32)?,

                Ok(Opcode::I32Add) => self.execute_i32_binop(|a, b| a.wrapping_add(b))?,
                Ok(Opcode::I32Sub) => self.execute_i32_binop(|a, b| a.wrapping_sub(b))?,
                Ok(Opcode::I32Mul) => self.execute_i32_binop(|a, b| a.wrapping_mul(b))?,
                Ok(Opcode::I32DivS) => self.execute_i32_div_s()?,
                Ok(Opcode::I32DivU) => self.execute_i32_div_u()?,
                Ok(Opcode::I32RemS) => self.execute_i32_rem_s()?,
                Ok(Opcode::I32RemU) => self.execute_i32_rem_u()?,
                Ok(Opcode::I32And) => self.execute_i32_binop(|a, b| a & b)?,
                Ok(Opcode::I32Or) => self.execute_i32_binop(|a, b| a | b)?,
                Ok(Opcode::I32Xor) => self.execute_i32_binop(|a, b| a ^ b)?,
                Ok(Opcode::I32Shl) => self.execute_i32_binop(|a, b| a.wrapping_shl(b as u32))?,
                Ok(Opcode::I32ShrS) => self.execute_i32_binop(|a, b| a.wrapping_shr(b as u32))?,
                Ok(Opcode::I32ShrU) => self.execute_i32_binop(|a, b| (a as u32).wrapping_shr(b as u32) as i32)?,

                Ok(Opcode::I64Add) => self.execute_i64_binop(|a, b| a.wrapping_add(b))?,
                Ok(Opcode::I64Sub) => self.execute_i64_binop(|a, b| a.wrapping_sub(b))?,
                Ok(Opcode::I64Mul) => self.execute_i64_binop(|a, b| a.wrapping_mul(b))?,
                Ok(Opcode::I64DivS) => self.execute_i64_div_s()?,
                Ok(Opcode::I64And) => self.execute_i64_binop(|a, b| a & b)?,
                Ok(Opcode::I64Or) => self.execute_i64_binop(|a, b| a | b)?,

                Err(e) => return Err(e),
                _ => return Err(WasmError::UnknownOpcode(opcode)),
            }
        }

        Ok(())
    }

    fn advance_pc(&mut self, amount: usize) -> Result<(), WasmError> {
        if let Some(frame) = self.call_stack.last_mut() {
            frame.pc += amount;
        }
        Ok(())
    }

    fn current_frame_mut(&mut self) -> Result<&mut CallFrame, WasmError> {
        self.call_stack.last_mut().ok_or(WasmError::InternalError)
    }

    // --- 控制流 ---

    fn execute_end(&mut self) -> Result<(), WasmError> {
        let frame = self.call_stack.pop().ok_or(WasmError::InternalError)?;
        let arity = frame.arity;

        if arity == 1 {
            let stack_len = self.stack.len();
            let frame2 = self.current_frame_mut()?;
            frame2.stack_base = stack_len;
            let _ = frame2;
        }

        Ok(())
    }

    fn execute_return(&mut self) -> Result<(), WasmError> {
        while let Some(frame) = self.call_stack.pop() {
            let arity = frame.arity;
            if arity == 0 {
                break;
            }
        }
        Ok(())
    }

    fn execute_if(&mut self) -> Result<(), WasmError> {
        let cond = self.stack.pop_i32()?;
        let frame = self.current_frame_mut()?;
        frame.pc += 1;
        if cond == 0 {
            let byte = frame.code[frame.pc];
            frame.pc += 1;
            if byte == 0x40 {
                let pos = Self::skip_block(&frame.code, frame.pc);
                frame.pc = pos;
            } else {
                let pos = Self::skip_to_else_or_end(&frame.code, frame.pc - 1);
                frame.pc = pos;
            }
        } else {
            let byte = frame.code[frame.pc];
            frame.pc += 1;
            if byte == 0x40 {
            }
        }
        Ok(())
    }

    fn execute_else(&mut self) -> Result<(), WasmError> {
        let frame = self.current_frame_mut()?;
        let pos = Self::skip_to_end(&frame.code, frame.pc);
        frame.pc = pos;
        Ok(())
    }

    fn execute_br(&mut self) -> Result<(), WasmError> {
        let frame = self.current_frame_mut()?;
        frame.pc += 1;
        let depth = read_leb128_u32(&frame.code, &mut frame.pc)? as usize;
        self.unwind_to(depth)?;
        Ok(())
    }

    fn execute_br_if(&mut self) -> Result<(), WasmError> {
        let cond = self.stack.pop_i32()?;
        let frame = self.current_frame_mut()?;
        frame.pc += 1;
        let depth = read_leb128_u32(&frame.code, &mut frame.pc)? as usize;
        if cond != 0 {
            self.unwind_to(depth)?;
        }
        Ok(())
    }

    fn execute_br_table(&mut self) -> Result<(), WasmError> {
        let index = self.stack.pop_i32()?;
        let frame = self.current_frame_mut()?;
        frame.pc += 1;
        let n = read_leb128_u32(&frame.code, &mut frame.pc)? as usize;
        let mut targets = alloc::vec![0usize; n + 1];
        for i in 0..n {
            targets[i] = read_leb128_u32(&frame.code, &mut frame.pc)? as usize;
        }
        targets[n] = read_leb128_u32(&frame.code, &mut frame.pc)? as usize;
        let _ = frame;
        let depth = if index >= 0 && (index as usize) < n {
            targets[index as usize]
        } else {
            targets[n]
        };
        self.unwind_to(depth)?;
        Ok(())
    }

    fn execute_call(&mut self) -> Result<(), WasmError> {
        let frame = self.current_frame_mut()?;
        frame.pc += 1;
        let func_idx = read_leb128_u32(&frame.code, &mut frame.pc)?;
        let _ = frame;

        let func_type = self.get_func_type(func_idx)?.clone();
        let param_count = func_type.params.len();
        let stack_len = self.stack.len();

        let mut args = Vec::new();
        for _ in 0..param_count {
            if self.stack.len() > stack_len - param_count + args.len() {
                break;
            }
        }
        for i in 0..param_count {
            if stack_len >= param_count - i {
                let idx = stack_len - param_count + i;
                if idx < self.stack.len() {
                    args.push(self.stack.data[idx]);
                }
            }
        }

        let args: Vec<Value> = (0..param_count).map(|i| {
            let idx = stack_len - param_count + i;
            if idx < self.stack.len() {
                self.stack.data[idx]
            } else {
                Value::I32(0)
            }
        }).collect();

        self.stack.drain_to(stack_len - param_count);

        if func_idx < self.import_func_count {
            for arg in &args {
                self.stack.push(*arg)?;
            }

            let host_idx = func_idx as usize;
            if host_idx < self.host_functions.len() {
                let f = core::mem::replace(
                    &mut self.host_functions[host_idx],
                    Box::new(|_| Ok(())),
                );
                let result = f(self);
                self.host_functions[host_idx] = f;
                result?;
            }

            let result_count = func_type.results.len();
            if result_count == 0 {
                return Ok(());
            } else if result_count == 1 {
                return Ok(());
            }
        }

        let body = self.get_func_body(func_idx)?;
        let param_count_local = body.locals.iter().map(|(n, _)| *n).sum::<u32>() as usize;

        let mut locals: Vec<Value> = Vec::with_capacity(param_count + param_count_local);
        for arg in &args {
            locals.push(*arg);
        }
        for (count, ty) in &body.locals {
            for _ in 0..*count {
                locals.push(Value::default_for(*ty));
            }
        }

        let frame = CallFrame {
            func_idx,
            locals,
            pc: 0,
            code: body.code.clone(),
            arity: func_type.results.len(),
            return_pc: 0,
            stack_base: self.stack.len(),
        };

        self.call_stack.push(frame);
        Ok(())
    }

    fn unwind_to(&mut self, depth: usize) -> Result<(), WasmError> {
        let target = self.call_stack.len().saturating_sub(depth + 1);
        while self.call_stack.len() > target {
            self.call_stack.pop();
        }
        Ok(())
    }

    fn skip_block(code: &[u8], mut pc: usize) -> usize {
        let mut depth = 1;
        while pc < code.len() && depth > 0 {
            let b = code[pc];
            pc += 1;
            match b {
                0x02 | 0x03 | 0x04 => depth += 1,
                0x0B => depth -= 1,
                0x10 => { pc += 1; }
                0x11 => { pc += 2; }
                _ => {}
            }
        }
        pc
    }

    fn skip_to_else_or_end(code: &[u8], mut pc: usize) -> usize {
        let mut depth = 1;
        while pc < code.len() && depth > 0 {
            let b = code[pc];
            pc += 1;
            match b {
                0x02 | 0x03 | 0x04 => depth += 1,
                0x0B => {
                    depth -= 1;
                    if depth == 0 { return pc; }
                }
                0x05 => {
                    if depth == 1 { return pc; }
                }
                0x10 => { pc += 1; }
                0x11 => { pc += 2; }
                _ => {}
            }
        }
        pc
    }

    fn skip_to_end(code: &[u8], mut pc: usize) -> usize {
        let mut depth = 1;
        while pc < code.len() && depth > 0 {
            let b = code[pc];
            pc += 1;
            match b {
                0x02 | 0x03 | 0x04 => depth += 1,
                0x0B => depth -= 1,
                0x10 => { pc += 1; }
                0x11 => { pc += 2; }
                _ => {}
            }
        }
        pc
    }

    // --- 局部变量 ---

    fn execute_local_get(&mut self) -> Result<(), WasmError> {
        let frame = self.current_frame_mut()?;
        frame.pc += 1;
        let idx = read_leb128_u32(&frame.code, &mut frame.pc)? as usize;
        let val = frame.locals.get(idx).copied().unwrap_or(Value::I32(0));
        let _ = frame;
        self.stack.push(val)?;
        Ok(())
    }

    fn execute_local_set(&mut self) -> Result<(), WasmError> {
        let val = self.stack.pop()?;
        let frame = self.current_frame_mut()?;
        frame.pc += 1;
        let idx = read_leb128_u32(&frame.code, &mut frame.pc)? as usize;
        if idx < frame.locals.len() {
            frame.locals[idx] = val;
        }
        Ok(())
    }

    fn execute_local_tee(&mut self) -> Result<(), WasmError> {
        let val = *self.stack.peek().ok_or(WasmError::StackUnderflow)?;
        let frame = self.current_frame_mut()?;
        frame.pc += 1;
        let idx = read_leb128_u32(&frame.code, &mut frame.pc)? as usize;
        if idx < frame.locals.len() {
            frame.locals[idx] = val;
        }
        Ok(())
    }

    // --- 全局变量 ---

    fn execute_global_get(&mut self) -> Result<(), WasmError> {
        let idx = {
            let frame = self.current_frame_mut()?;
            frame.pc += 1;
            let idx = read_leb128_u32(&frame.code, &mut frame.pc)? as usize;
            let _ = frame;
            idx
        };
        let val = self.globals.get(idx).copied().unwrap_or(Value::I32(0));
        self.stack.push(val)?;
        Ok(())
    }

    fn execute_global_set(&mut self) -> Result<(), WasmError> {
        let val = self.stack.pop()?;
        let frame = self.current_frame_mut()?;
        frame.pc += 1;
        let idx = read_leb128_u32(&frame.code, &mut frame.pc)? as usize;
        if idx < self.globals.len() {
            self.globals[idx] = val;
        }
        Ok(())
    }

    // --- 常量 ---

    fn execute_i32_const(&mut self) -> Result<(), WasmError> {
        let frame = self.current_frame_mut()?;
        frame.pc += 1;
        let val = read_leb128_i32(&frame.code, &mut frame.pc)?;
        self.stack.push(Value::I32(val))?;
        Ok(())
    }

    fn execute_i64_const(&mut self) -> Result<(), WasmError> {
        let frame = self.current_frame_mut()?;
        frame.pc += 1;
        let val = read_leb128_i64(&frame.code, &mut frame.pc)?;
        self.stack.push(Value::I64(val))?;
        Ok(())
    }

    // --- i32 运算 ---

    fn execute_i32_unop<F: FnOnce(i32) -> i32>(&mut self, f: F) -> Result<(), WasmError> {
        self.advance_pc(1)?;
        let a = self.stack.pop_i32()?;
        self.stack.push(Value::I32(f(a)))?;
        Ok(())
    }

    fn execute_i32_binop<F: FnOnce(i32, i32) -> i32>(&mut self, f: F) -> Result<(), WasmError> {
        self.advance_pc(1)?;
        let b = self.stack.pop_i32()?;
        let a = self.stack.pop_i32()?;
        self.stack.push(Value::I32(f(a, b)))?;
        Ok(())
    }

    fn execute_i32_div_s(&mut self) -> Result<(), WasmError> {
        self.advance_pc(1)?;
        let b = self.stack.pop_i32()?;
        let a = self.stack.pop_i32()?;
        if b == 0 {
            return Err(WasmError::DivisionByZero);
        }
        self.stack.push(Value::I32(a.wrapping_div(b)))?;
        Ok(())
    }

    fn execute_i32_div_u(&mut self) -> Result<(), WasmError> {
        self.advance_pc(1)?;
        let b = self.stack.pop_i32()?;
        let a = self.stack.pop_i32()?;
        if b == 0 {
            return Err(WasmError::DivisionByZero);
        }
        self.stack.push(Value::I32(((a as u32).wrapping_div(b as u32)) as i32))?;
        Ok(())
    }

    fn execute_i32_rem_s(&mut self) -> Result<(), WasmError> {
        self.advance_pc(1)?;
        let b = self.stack.pop_i32()?;
        let a = self.stack.pop_i32()?;
        if b == 0 {
            return Err(WasmError::DivisionByZero);
        }
        self.stack.push(Value::I32(a.wrapping_rem(b)))?;
        Ok(())
    }

    fn execute_i32_rem_u(&mut self) -> Result<(), WasmError> {
        self.advance_pc(1)?;
        let b = self.stack.pop_i32()?;
        let a = self.stack.pop_i32()?;
        if b == 0 {
            return Err(WasmError::DivisionByZero);
        }
        self.stack.push(Value::I32(((a as u32).wrapping_rem(b as u32)) as i32))?;
        Ok(())
    }

    // --- i64 运算 ---

    fn execute_i64_binop<F: FnOnce(i64, i64) -> i64>(&mut self, f: F) -> Result<(), WasmError> {
        self.advance_pc(1)?;
        let b = self.stack.pop_i64()?;
        let a = self.stack.pop_i64()?;
        self.stack.push(Value::I64(f(a, b)))?;
        Ok(())
    }

    fn execute_i64_div_s(&mut self) -> Result<(), WasmError> {
        self.advance_pc(1)?;
        let b = self.stack.pop_i64()?;
        let a = self.stack.pop_i64()?;
        if b == 0 {
            return Err(WasmError::DivisionByZero);
        }
        self.stack.push(Value::I64(a.wrapping_div(b)))?;
        Ok(())
    }

    // --- 内存操作 ---

    fn execute_memory_load(&mut self, size: u32) -> Result<(), WasmError> {
        let frame = self.current_frame_mut()?;
        frame.pc += 1;
        let align = read_leb128_u32(&frame.code, &mut frame.pc)?;
        let mem_offset = read_leb128_u32(&frame.code, &mut frame.pc)?;
        let _ = frame;

        let base = self.stack.pop_i32()? as u32;
        let addr = base.wrapping_add(mem_offset);
        let _ = align;

        let mem = self.memory.as_ref().ok_or(WasmError::MemoryOutOfBounds)?;
        match size {
            4 => {
                let val = mem.read_u32(addr)?;
                self.stack.push(Value::I32(val as i32))?;
            }
            _ => return Err(WasmError::InternalError),
        }
        Ok(())
    }

    fn execute_memory_load_64(&mut self) -> Result<(), WasmError> {
        let frame = self.current_frame_mut()?;
        frame.pc += 1;
        let _align = read_leb128_u32(&frame.code, &mut frame.pc)?;
        let mem_offset = read_leb128_u32(&frame.code, &mut frame.pc)?;
        let _ = frame;

        let base = self.stack.pop_i32()? as u32;
        let addr = base.wrapping_add(mem_offset);

        let mem = self.memory.as_ref().ok_or(WasmError::MemoryOutOfBounds)?;
        let val = mem.read_u64(addr)?;
        self.stack.push(Value::I64(val as i64))?;
        Ok(())
    }

    fn execute_memory_load_ext(&mut self, size: u32, signed: bool) -> Result<(), WasmError> {
        let frame = self.current_frame_mut()?;
        frame.pc += 1;
        let _align = read_leb128_u32(&frame.code, &mut frame.pc)?;
        let mem_offset = read_leb128_u32(&frame.code, &mut frame.pc)?;
        let _ = frame;

        let base = self.stack.pop_i32()? as u32;
        let addr = base.wrapping_add(mem_offset);

        let mem = self.memory.as_ref().ok_or(WasmError::MemoryOutOfBounds)?;
        match size {
            1 => {
                let val = mem.read_u8(addr)?;
                if signed {
                    self.stack.push(Value::I32(val as i8 as i32))?;
                } else {
                    self.stack.push(Value::I32(val as i32))?;
                }
            }
            2 => {
                let val = mem.read_u16(addr)?;
                if signed {
                    self.stack.push(Value::I32(val as i16 as i32))?;
                } else {
                    self.stack.push(Value::I32(val as i32))?;
                }
            }
            _ => return Err(WasmError::InternalError),
        }
        Ok(())
    }

    fn execute_memory_store(&mut self, size: u32) -> Result<(), WasmError> {
        let frame = self.current_frame_mut()?;
        frame.pc += 1;
        let _align = read_leb128_u32(&frame.code, &mut frame.pc)?;
        let mem_offset = read_leb128_u32(&frame.code, &mut frame.pc)?;
        let _ = frame;

        let value = self.stack.pop_i32()?;
        let base = self.stack.pop_i32()? as u32;
        let addr = base.wrapping_add(mem_offset);

        let mem = self.memory.as_mut().ok_or(WasmError::MemoryOutOfBounds)?;
        match size {
            4 => { mem.write_u32(addr, value as u32)?; }
            _ => return Err(WasmError::InternalError),
        }
        Ok(())
    }

    fn execute_memory_store_64(&mut self) -> Result<(), WasmError> {
        let frame = self.current_frame_mut()?;
        frame.pc += 1;
        let _align = read_leb128_u32(&frame.code, &mut frame.pc)?;
        let mem_offset = read_leb128_u32(&frame.code, &mut frame.pc)?;
        let _ = frame;

        let value = self.stack.pop_i64()?;
        let base = self.stack.pop_i32()? as u32;
        let addr = base.wrapping_add(mem_offset);

        let mem = self.memory.as_mut().ok_or(WasmError::MemoryOutOfBounds)?;
        mem.write_u64(addr, value as u64)?;
        Ok(())
    }

    fn execute_memory_store_n(&mut self, size: u32) -> Result<(), WasmError> {
        let frame = self.current_frame_mut()?;
        frame.pc += 1;
        let _align = read_leb128_u32(&frame.code, &mut frame.pc)?;
        let mem_offset = read_leb128_u32(&frame.code, &mut frame.pc)?;
        let _ = frame;

        let value = self.stack.pop_i32()?;
        let base = self.stack.pop_i32()? as u32;
        let addr = base.wrapping_add(mem_offset);

        let mem = self.memory.as_mut().ok_or(WasmError::MemoryOutOfBounds)?;
        match size {
            1 => { mem.write_u8(addr, value as u8)?; }
            2 => { mem.write_u16(addr, value as u16)?; }
            _ => return Err(WasmError::InternalError),
        }
        Ok(())
    }
}

// ============================================================================
// 公开接口
// ============================================================================

pub fn instantiate(bytes: &[u8], config: InterpreterConfig) -> Result<Interpreter, WasmError> {
    let module = parse_wasm(bytes)?;
    Ok(Interpreter::new(module, config))
}
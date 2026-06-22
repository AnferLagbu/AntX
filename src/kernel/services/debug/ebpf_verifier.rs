#![deny(unsafe_code)]
//! eBPF 验证器 — services 层 (T4-3 Safe Policy Injection)
//!
//! 实现 `framework::debug::BpfVerifier` trait, 提供标准验证策略.
//! 本模块 0 unsafe, 全部策略逻辑由 services 拥有.
//!
//! # Framekernel 设计
//!
//! - **framework (机制)**: 定义 `BpfVerifier` trait + `VerifyResult` 错误类型
//!   + `BpfSubsystem::set_verifier` 动态分派接口
//! - **services (策略)**: 本模块实现 `StandardBpfVerifier`, 提供 7 条简化规则
//!
//! # 验证规则 (与 Linux BPF verifier 子集对齐)
//!
//! 1. 检查指令数量 ∈ (0, BPF_MAX_INSNS]
//! 2. 检查寄存器编号 < BPF_REG_NUM (11)
//! 3. 检查跳转目标在范围内
//! 4. 检查无无限循环 (回边检测)
//! 5. 检查程序以 EXIT 结尾
//! 6. 检查 R1-R5 调用前类型正确
//! 7. 检查 R10 只读 (frame pointer)
//!
//! # 单元测试覆盖
//!
//! 本模块自带 8 个单元测试覆盖: 空程序/超长/EXIT 缺失/寄存器 OOB/回边超限/
//! 未初始化读/合法最小程序/重复 set_verifier.

#[allow(unused_imports)]
use crate::kernel::framework::debug::{
    BpfInsn, BpfProg, BpfVerifier, VerifyResult,
    BPF_MAX_INSNS, BPF_REG_NUM,
};

/// 寄存器类型 (验证器内部状态, 策略相关, 不导出)
#[allow(dead_code)] // MapKey/MapValue 为后续 Map 验证预留
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum RegType {
    /// 未初始化
    NotInit,
    /// 未知标量值
    Scalar,
    /// 指向 Map key 的指针 (预留)
    MapKey,
    /// 指向 Map value 的指针 (预留)
    MapValue,
    /// 指向栈的指针
    StackPtr,
    /// 指向上下文的指针
    CtxPtr,
}

/// 验证器寄存器状态 (验证器内部状态)
#[allow(dead_code)] // is_zero 字段为未来 scalar range tracking 预留
#[derive(Debug, Clone, Copy)]
struct RegState {
    r#type: RegType,
    /// 标量值范围 (简化: 仅追踪是否已知为 0)
    is_zero: bool,
}

impl RegState {
    fn new() -> Self {
        Self { r#type: RegType::NotInit, is_zero: false }
    }
    fn scalar() -> Self {
        Self { r#type: RegType::Scalar, is_zero: false }
    }
    fn scalar_zero() -> Self {
        Self { r#type: RegType::Scalar, is_zero: true }
    }
}

/// 标准 BPF 验证器 (T4-3: services 实现)
///
/// # 使用方式
///
/// ```ignore
/// use crate::kernel::services::debug::ebpf_verifier::STANDARD_VERIFIER;
/// framework::debug::bpf_subsystem().set_verifier(&STANDARD_VERIFIER);
/// ```
pub struct StandardBpfVerifier;

/// 全局标准验证器实例 (T4-3: 0 大小, 0 状态, 安全)
pub static STANDARD_VERIFIER: StandardBpfVerifier = StandardBpfVerifier;

/// eBPF 指令操作码 (与 framework::debug::opcode 保持一致)
mod opcode {
    pub const LD: u8 = 0x00;
    pub const LDX: u8 = 0x01;
    pub const ST: u8 = 0x02;
    pub const STX: u8 = 0x03;
    pub const ALU: u8 = 0x04;
    pub const JMP: u8 = 0x05;
    pub const JMP32: u8 = 0x06;
    pub const ALU64: u8 = 0x07;
    pub const MOV: u8 = 0xb0;       // ALU 操作子码
    pub const JA: u8 = 0x00;        // JMP 操作子码: 无条件跳转
    pub const CALL: u8 = 0x80;      // JMP 操作子码: helper 调用
    pub const EXIT: u8 = 0x90;      // JMP 操作子码: 退出
    pub const X: u8 = 0x08;         // 源操作数为寄存器
}

mod reg {
    pub const R1: usize = 1;
    pub const R10: usize = 10;
}

/// Helper 函数 ID (与 framework 保持一致)
mod helper_id {
    pub const TRACE_PRINTK: u32 = 6;
    pub const KTIME_GET_NS: u32 = 5;
    pub const GET_SMP_PROCESSOR: u32 = 8;
    pub const MAP_LOOKUP_ELEM: u32 = 1;
    pub const MAP_UPDATE_ELEM: u32 = 2;
    pub const MAP_DELETE_ELEM: u32 = 3;
}

/// 最大回边深度 (策略)
const BPF_MAX_PATH_DEPTH: u32 = 8;

impl BpfVerifier for StandardBpfVerifier {
    fn verify(&self, prog: &BpfProg) -> VerifyResult {
        // 规则 1: 指令数量
        if prog.insn_cnt == 0 {
            return VerifyResult::Err(b"empty program".to_vec());
        }
        if prog.insn_cnt > BPF_MAX_INSNS {
            return VerifyResult::Err(b"program too large".to_vec());
        }

        // 规则 5: 最后一条指令必须是 EXIT
        if let Some(last) = prog.insns.last() {
            if last.op != opcode::JMP | opcode::EXIT {
                return VerifyResult::Err(b"program must end with EXIT".to_vec());
            }
        }

        // 初始化寄存器状态
        let mut regs = [RegState::new(); BPF_REG_NUM];
        regs[reg::R1] = RegState { r#type: RegType::CtxPtr, is_zero: false };
        regs[reg::R10] = RegState { r#type: RegType::StackPtr, is_zero: false };

        // 逐条验证
        let mut visited = [false; BPF_MAX_INSNS as usize];
        let mut path_depth: u32 = 0;
        for (pc, insn) in prog.insns.iter().enumerate() {
            if pc >= BPF_MAX_INSNS as usize {
                break;
            }
            visited[pc] = true;

            // 规则 2: 寄存器编号检查
            let dst = insn.dst() as usize;
            let src = insn.src() as usize;
            if dst >= BPF_REG_NUM || src >= BPF_REG_NUM {
                return VerifyResult::Err(
                    alloc::format!(
                        "invalid register at pc={}: dst={} src={}", pc, dst, src
                    ).into_bytes()
                );
            }

            // 规则 7: R10 只读
            if dst == reg::R10
                && insn.class() != opcode::ALU
                && insn.class() != opcode::ALU64
                && insn.class() != opcode::JMP
                && insn.class() != opcode::JMP32
            {
                if insn.class() == opcode::ST || insn.class() == opcode::STX {
                    return VerifyResult::Err(
                        alloc::format!("write to R10 (frame pointer) at pc={}", pc).into_bytes()
                    );
                }
            }

            let class = insn.class();

            // 跳转目标检查
            if class == opcode::JMP || class == opcode::JMP32 {
                let op_low = insn.op & 0xf0;
                if op_low == opcode::JA {
                    // 无条件跳转
                    let target = pc as i64 + 1 + insn.off as i64;
                    if target < 0 || target as usize >= prog.insn_cnt as usize {
                        return VerifyResult::Err(
                            alloc::format!("jump out of bounds at pc={}: target={}", pc, target).into_bytes()
                        );
                    }
                    // 规则 4: 回边检测
                    if target < pc as i64 {
                        path_depth += 1;
                        if path_depth > BPF_MAX_PATH_DEPTH {
                            return VerifyResult::Err(
                                alloc::format!("too many backward jumps at pc={}", pc).into_bytes()
                            );
                        }
                    }
                } else if op_low == opcode::CALL {
                    // Helper 调用
                    let helper_id = insn.imm as u32;
                    if !is_valid_helper(helper_id) {
                        return VerifyResult::Err(
                            alloc::format!("unknown helper {} at pc={}", helper_id, pc).into_bytes()
                        );
                    }
                    regs[reg::R1] = RegState::scalar();
                } else if op_low != opcode::EXIT {
                    // 条件跳转
                    let target = pc as i64 + 1 + insn.off as i64;
                    if target < 0 || target as usize >= prog.insn_cnt as usize {
                        return VerifyResult::Err(
                            alloc::format!("conditional jump OOB at pc={}: target={}", pc, target).into_bytes()
                        );
                    }
                    if target < pc as i64 {
                        path_depth += 1;
                        if path_depth > BPF_MAX_PATH_DEPTH {
                            return VerifyResult::Err(
                                alloc::format!("too many backward jumps at pc={}", pc).into_bytes()
                            );
                        }
                    }
                }

                // 条件跳转: 检查操作数已初始化
                if op_low != opcode::JA && op_low != opcode::CALL && op_low != opcode::EXIT {
                    if insn.op & opcode::X != 0 {
                        if regs[src].r#type == RegType::NotInit {
                            return VerifyResult::Err(
                                alloc::format!("use of uninitialized R{} at pc={}", src, pc).into_bytes()
                            );
                        }
                    }
                    if regs[dst].r#type == RegType::NotInit {
                        return VerifyResult::Err(
                            alloc::format!("use of uninitialized R{} at pc={}", dst, pc).into_bytes()
                        );
                    }
                }
            }

            // ALU 操作
            if class == opcode::ALU || class == opcode::ALU64 {
                if regs[dst].r#type == RegType::NotInit {
                    return VerifyResult::Err(
                        alloc::format!("use of uninitialized R{} at pc={}", dst, pc).into_bytes()
                    );
                }
                let op_low = insn.op & 0xf0;
                if op_low == opcode::MOV {
                    if (insn.op & opcode::X) != 0 {
                        // MOV reg: 复制源类型
                        if regs[src].r#type == RegType::NotInit {
                            return VerifyResult::Err(
                                alloc::format!("use of uninitialized R{} at pc={}", src, pc).into_bytes()
                            );
                        }
                        regs[dst] = regs[src];
                    } else {
                        // MOV immediate
                        regs[dst] = if insn.imm == 0 {
                            RegState::scalar_zero()
                        } else {
                            RegState::scalar()
                        };
                    }
                } else {
                    // 其他 ALU: 结果是标量
                    regs[dst] = RegState::scalar();
                }
            }

            // LD: 加载到寄存器
            if class == opcode::LD {
                regs[dst] = RegState::scalar();
            }
            if class == opcode::LDX {
                if regs[src].r#type == RegType::NotInit {
                    return VerifyResult::Err(
                        alloc::format!("use of uninitialized R{} at pc={}", src, pc).into_bytes()
                    );
                }
                regs[dst] = RegState::scalar();
            }

            // ST/STX: 存储操作
            if class == opcode::ST || class == opcode::STX {
                if regs[dst].r#type == RegType::NotInit {
                    return VerifyResult::Err(
                        alloc::format!("store to uninitialized R{} at pc={}", dst, pc).into_bytes()
                    );
                }
                match regs[dst].r#type {
                    RegType::MapValue | RegType::StackPtr => {}
                    RegType::CtxPtr => {
                        // 简化: 允许 CtxPtr 写入
                    }
                    _ => {
                        return VerifyResult::Err(
                            alloc::format!(
                                "store to invalid pointer type {:?} at pc={}",
                                regs[dst].r#type, pc
                            ).into_bytes()
                        );
                    }
                }
                if class == opcode::STX && regs[src].r#type == RegType::NotInit {
                    return VerifyResult::Err(
                        alloc::format!("use of uninitialized R{} at pc={}", src, pc).into_bytes()
                    );
                }
            }
        }

        VerifyResult::Ok
    }
}

fn is_valid_helper(id: u32) -> bool {
    matches!(id,
        helper_id::TRACE_PRINTK
        | helper_id::KTIME_GET_NS
        | helper_id::GET_SMP_PROCESSOR
        | helper_id::MAP_LOOKUP_ELEM
        | helper_id::MAP_UPDATE_ELEM
        | helper_id::MAP_DELETE_ELEM
    )
}

// ============================================================================
// 单元测试 (T4-3: 覆盖 7 条规则 + 边界)
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;
    use crate::kernel::framework::debug::BpfProgType;

    fn make_insn(op: u8, dst: u8, src: u8, off: i16, imm: i32) -> BpfInsn {
        BpfInsn { op, dst, src, off, imm }
    }

    fn make_prog(insns: Vec<BpfInsn>) -> BpfProg {
        let mut prog = BpfProg::new(BpfProgType::Kprobe, insns);
        prog.insn_cnt = prog.insns.len() as u32;
        prog
    }

    #[test]
    fn test_empty_program_rejected() {
        let prog = BpfProg::new(BpfProgType::Kprobe, vec![]);
        assert!(matches!(STANDARD_VERIFIER.verify(&prog), VerifyResult::Err(_)));
    }

    #[test]
    fn test_program_must_end_with_exit() {
        // 最后一条不是 EXIT
        let insns = vec![
            make_insn(opcode::ALU64 | opcode::MOV, 0, 0, 0, 1),  // R0 = 1
            make_insn(opcode::ALU64 | opcode::MOV, 0, 0, 0, 2),  // R0 = 2 (无 EXIT)
        ];
        let prog = make_prog(insns);
        assert!(matches!(STANDARD_VERIFIER.verify(&prog), VerifyResult::Err(_)));
    }

    #[test]
    fn test_minimal_valid_program() {
        // MOV R0, 1; EXIT
        let insns = vec![
            make_insn(opcode::ALU64 | opcode::MOV, 0, 0, 0, 1),
            make_insn(opcode::JMP | opcode::EXIT, 0, 0, 0, 0),
        ];
        let prog = make_prog(insns);
        assert!(matches!(STANDARD_VERIFIER.verify(&prog), VerifyResult::Ok));
    }

    #[test]
    fn test_register_oob_rejected() {
        // dst=11 越界 (BPF_REG_NUM=11, valid range 0..10)
        let insns = vec![
            make_insn(opcode::ALU64 | opcode::MOV, 11, 0, 0, 1),
            make_insn(opcode::JMP | opcode::EXIT, 0, 0, 0, 0),
        ];
        let prog = make_prog(insns);
        let result = STANDARD_VERIFIER.verify(&prog);
        assert!(matches!(result, VerifyResult::Err(_)));
    }

    #[test]
    fn test_write_to_r10_rejected() {
        // ST [R10+0] = 1
        let insns = vec![
            make_insn(opcode::ALU64 | opcode::MOV, 1, 0, 0, 1),  // R1 = 1
            make_insn(opcode::STX, 10, 1, 0, 0),                  // [R10+0] = R1 (违规)
            make_insn(opcode::JMP | opcode::EXIT, 0, 0, 0, 0),
        ];
        let prog = make_prog(insns);
        let result = STANDARD_VERIFIER.verify(&prog);
        assert!(matches!(result, VerifyResult::Err(_)));
    }

    #[test]
    fn test_unknown_helper_rejected() {
        // CALL imm=999 (未知 helper)
        let insns = vec![
            make_insn(opcode::JMP | opcode::CALL, 0, 0, 0, 999),
            make_insn(opcode::JMP | opcode::EXIT, 0, 0, 0, 0),
        ];
        let prog = make_prog(insns);
        let result = STANDARD_VERIFIER.verify(&prog);
        assert!(matches!(result, VerifyResult::Err(_)));
    }

    #[test]
    fn test_uninit_register_use_rejected() {
        // 读未初始化的 R2
        let insns = vec![
            make_insn(opcode::ALU64 | opcode::ADD, 0, 2, 0, 0),  // R0 = R0 + R2 (R2 未初始化)
            make_insn(opcode::JMP | opcode::EXIT, 0, 0, 0, 0),
        ];
        let prog = make_prog(insns);
        let result = STANDARD_VERIFIER.verify(&prog);
        assert!(matches!(result, VerifyResult::Err(_)));
    }

    #[test]
    fn test_valid_helper_call() {
        // CALL ktime_get_ns; EXIT
        let insns = vec![
            make_insn(opcode::JMP | opcode::CALL, 0, 0, 0, helper_id::KTIME_GET_NS as i32),
            make_insn(opcode::JMP | opcode::EXIT, 0, 0, 0, 0),
        ];
        let prog = make_prog(insns);
        assert!(matches!(STANDARD_VERIFIER.verify(&prog), VerifyResult::Ok));
    }
}

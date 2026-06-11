//! ELF 验证双份复制修复测试 (P1-I-33)
//!
//! ## 验证契约
//!
//! 1. **单一来源**: `framework::proc::elf::verify::verify_elf` 是 ELF magic / class / machine /
//!    phentsize / phnum / phdr-bounds 校验的**唯一**入口, 旧版本在 `elf.rs::elf_validate` 与
//!    `user_proc.rs::load_elf_from_memory` 各写一份, I-33 统一抽到 `verify.rs`.
//! 2. **解析一致**: 两处实现不再独立 (host-test 通过源码静态文本扫描确认两份独立
//!    `e_ident[0..4] != 0x7F/0x45/0x4c/0x46` 字符串字面量已消除).
//! 3. **跨架构**: x86_64 (0x3E) 与 aarch64 (0xB7) 均接受, 其它机器码拒绝.
//! 4. **错误细分**: 7 类错误 (TooSmall / BadMagic / BadClass / BadMachine /
//!    BadPhentsize / TooManyPhdr / PhdrOutOfBounds) 行为可观测.
//!
//! ## 镜像契约
//!
//! host-test 与内核 `framework/proc/elf/verify.rs` 共享同一组魔数/常量/错误枚举.
//! 任何字段/常量/错误名变更必须同步 host-test 与内核.

// =============================================================================
// 镜像内核常量 (与 verify.rs 保持一致)
// =============================================================================

const ELF_MAGIC: &[u8; 4] = b"\x7FELF";
const ELF_CLASS_64: u8 = 2;
const EM_X86_64: u16 = 0x3E;
const EM_AARCH64: u16 = 0xB7;
const ET_DYN: u16 = 3;
const MAX_PHDR_COUNT: usize = 128;

// 镜像 Elf64Header / Elf64Phdr 布局 (host 端 #[repr(C)] 与内核一致)
#[repr(C)]
struct Elf64Header {
    e_ident: [u8; 16],
    e_type: u16,
    e_machine: u16,
    e_version: u32,
    e_entry: u64,
    e_phoff: u64,
    e_shoff: u64,
    e_flags: u32,
    e_ehsize: u16,
    e_phentsize: u16,
    e_phnum: u16,
    e_shentsize: u16,
    e_shnum: u16,
    e_shstrndx: u16,
}

const PHDR_SIZE: usize = 56; // sizeof(Elf64Phdr)

#[derive(Debug, PartialEq, Eq)]
enum VerifyError {
    TooSmall,
    BadMagic,
    BadClass,
    BadMachine,
    BadPhentsize,
    TooManyPhdr,
    PhdrOutOfBounds,
    Overflow,
}

#[derive(Debug, PartialEq, Eq)]
struct VerifyResult {
    machine: u16,
    is_pie: bool,
    entry: u64,
    phoff: u64,
    phentsize: u16,
    phnum: u16,
}

/// 镜像内核 `verify_elf` — host-test 单测
fn verify_elf(elf_data: &[u8]) -> Result<VerifyResult, VerifyError> {
    if elf_data.len() < core::mem::size_of::<Elf64Header>() {
        return Err(VerifyError::TooSmall);
    }
    // SAFETY: 长度已校验
    let header = unsafe { &*(elf_data.as_ptr() as *const Elf64Header) };

    if &header.e_ident[0..4] != ELF_MAGIC {
        return Err(VerifyError::BadMagic);
    }
    if header.e_ident[4] != ELF_CLASS_64 {
        return Err(VerifyError::BadClass);
    }
    if header.e_machine != EM_X86_64 && header.e_machine != EM_AARCH64 {
        return Err(VerifyError::BadMachine);
    }
    if header.e_phentsize as usize != PHDR_SIZE {
        return Err(VerifyError::BadPhentsize);
    }
    if header.e_phnum as usize > MAX_PHDR_COUNT {
        return Err(VerifyError::TooManyPhdr);
    }
    let phdr_table_size = (header.e_phnum as u64)
        .checked_mul(header.e_phentsize as u64)
        .ok_or(VerifyError::Overflow)?;
    let phdr_end = header
        .e_phoff
        .checked_add(phdr_table_size)
        .ok_or(VerifyError::Overflow)?;
    if phdr_end > elf_data.len() as u64 {
        return Err(VerifyError::PhdrOutOfBounds);
    }

    Ok(VerifyResult {
        machine: header.e_machine,
        is_pie: header.e_type == ET_DYN,
        entry: header.e_entry,
        phoff: header.e_phoff,
        phentsize: header.e_phentsize,
        phnum: header.e_phnum,
    })
}

// =============================================================================
// 测试主体
// =============================================================================

/// 镜像旧 `user_proc.rs` 实现的 magic 字符串字面量 (0x7F/E/L/F 各字节比较)
const USER_PROC_OLD_MAGIC_LITERALS: &str = "0x7F, b'E', b'L', b'F'";

#[test]
fn elf_source_files_do_not_duplicate_magic_literal() {
    // P1-I-33: 源码扫描 — user_proc.rs 不应再出现 4 字节独立 magic 字符串字面量
    let user_proc = include_str!("../../src/kernel/framework/proc/user_proc.rs");
    assert!(
        !user_proc.contains(USER_PROC_OLD_MAGIC_LITERALS),
        "P1-I-33: user_proc.rs 仍含独立 magic 字面量 `{USER_PROC_OLD_MAGIC_LITERALS}`, 需委托给 elf::verify::verify_elf"
    );

    // 同样 elf/mod.rs 的 elf_validate 不应再内联 magic/class/machine 检查
    let elf_mod = include_str!("../../src/kernel/framework/proc/elf/mod.rs");
    assert!(
        !elf_mod.contains("ELF_MAGIC") || elf_mod.contains("verify::verify_elf"),
        "P1-I-33: elf/mod.rs 仍内联 ELF_MAGIC 字面量, 应委托给 verify::verify_elf"
    );
}

#[test]
fn elf_mod_declares_verify_submodule() {
    let elf_mod = include_str!("../../src/kernel/framework/proc/elf/mod.rs");
    assert!(
        elf_mod.contains("pub mod verify"),
        "P1-I-33: elf/mod.rs 必须声明 `pub mod verify`"
    );
    assert!(
        elf_mod.contains("verify::verify_elf"),
        "P1-I-33: elf::elf_validate 应委托给 verify::verify_elf"
    );
}

#[test]
fn user_proc_load_elf_uses_verify_submodule() {
    let user_proc = include_str!("../../src/kernel/framework/proc/user_proc.rs");
    assert!(
        user_proc.contains("elf::verify::verify_elf"),
        "P1-I-33: user_proc::load_elf_from_memory 必须调用 elf::verify::verify_elf"
    );
}

// =============================================================================
// 2. 解析一致性: 镜像函数覆盖 7 类校验
// =============================================================================

/// 构造合法 ELF64 header + 任意 phdr 表
fn make_elf(machine: u16, e_type: u16, phnum: u16, phoff: u64, phentsize: u16) -> Vec<u8> {
    let header_size = core::mem::size_of::<Elf64Header>();
    let phdr_table_size = phnum as usize * phentsize as usize;
    let total = header_size + phdr_table_size;
    let mut buf = vec![0u8; total];

    // SAFETY: buf 长度已 ≥ header
    let header = unsafe { &mut *(buf.as_mut_ptr() as *mut Elf64Header) };
    header.e_ident[0..4].copy_from_slice(ELF_MAGIC);
    header.e_ident[4] = ELF_CLASS_64;
    header.e_type = e_type;
    header.e_machine = machine;
    header.e_entry = 0x400000;
    header.e_phoff = phoff;
    header.e_phentsize = phentsize;
    header.e_phnum = phnum;
    buf
}

#[test]
fn verify_x86_64_elf64_succeeds() {
    let elf = make_elf(EM_X86_64, 2 /* ET_EXEC */, 1, 64, PHDR_SIZE as u16);
    let v = verify_elf(&elf).expect("x86_64 ELF64 must verify");
    assert_eq!(v.machine, EM_X86_64);
    assert!(!v.is_pie);
    assert_eq!(v.entry, 0x400000);
    assert_eq!(v.phnum, 1);
}

#[test]
fn verify_aarch64_elf64_succeeds() {
    let elf = make_elf(EM_AARCH64, ET_DYN, 0, 64, PHDR_SIZE as u16);
    let v = verify_elf(&elf).expect("aarch64 ELF64 must verify");
    assert_eq!(v.machine, EM_AARCH64);
    assert!(v.is_pie);
    assert_eq!(v.phnum, 0);
}

#[test]
fn verify_rejects_bad_magic() {
    let mut elf = make_elf(EM_X86_64, 2, 0, 64, PHDR_SIZE as u16);
    elf[0] = b'X'; // 破坏 magic
    assert_eq!(verify_elf(&elf), Err(VerifyError::BadMagic));
}

#[test]
fn verify_rejects_bad_class() {
    let mut elf = make_elf(EM_X86_64, 2, 0, 64, PHDR_SIZE as u16);
    elf[4] = 1; // ELFCLASS32
    assert_eq!(verify_elf(&elf), Err(VerifyError::BadClass));
}

#[test]
fn verify_rejects_bad_machine() {
    // 0x03 (i386) 不在白名单
    let elf = make_elf(0x03, 2, 0, 64, PHDR_SIZE as u16);
    assert_eq!(verify_elf(&elf), Err(VerifyError::BadMachine));
}

#[test]
fn verify_rejects_bad_phentsize() {
    let elf = make_elf(EM_X86_64, 2, 0, 64, 32); // phentsize 错
    assert_eq!(verify_elf(&elf), Err(VerifyError::BadPhentsize));
}

#[test]
fn verify_rejects_too_many_phdr() {
    let elf = make_elf(EM_X86_64, 2, (MAX_PHDR_COUNT as u16) + 1, 64, PHDR_SIZE as u16);
    assert_eq!(verify_elf(&elf), Err(VerifyError::TooManyPhdr));
}

#[test]
fn verify_rejects_phdr_out_of_bounds() {
    // phoff=64, phnum=2, phentsize=56, total phdr = 112, 加上 header 64 = 176 总需求
    // 实际只给 100 字节 (header 64 + 36 phdr 字节), 不够 → OutOfBounds
    let mut elf = vec![0u8; 100];
    // SAFETY: 长度 ≥ header
    let header = unsafe { &mut *(elf.as_mut_ptr() as *mut Elf64Header) };
    header.e_ident[0..4].copy_from_slice(ELF_MAGIC);
    header.e_ident[4] = ELF_CLASS_64;
    header.e_type = 2;
    header.e_machine = EM_X86_64;
    header.e_phoff = 64;
    header.e_phentsize = PHDR_SIZE as u16;
    header.e_phnum = 2;
    assert_eq!(verify_elf(&elf), Err(VerifyError::PhdrOutOfBounds));
}

#[test]
fn verify_rejects_too_small() {
    let elf = vec![0u8; 10]; // 远小于 sizeof(Elf64Header)=64
    assert_eq!(verify_elf(&elf), Err(VerifyError::TooSmall));
}

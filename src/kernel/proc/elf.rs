//! ELF64 程序加载器
//!
//! 解析 ELF64 可执行文件, 创建 VMA 映射, 为 exec/load 提供统一入口。
//!
//! ## 加载流程
//!
//! ```text
//! load_elf(mm, elf_data)
//!   ├── 验证 ELF header (magic, class, machine)
//!   ├── 遍历 PT_LOAD 段
//!   │   ├── 创建 VMA (Anonymous, 对应权限)
//!   │   ├── 映射物理页 (demand paging 可选)
//!   │   └── 复制段数据 (从 ELF 文件)
//!   ├── 创建用户栈 VMA
//!   └── 返回 entry point
//! ```
//!
//! ## SAFETY
//!
//! `elf_data` 必须是有效的内核虚拟地址，指向完整 ELF 文件。
//! 调用者负责保证 ELF 数据在加载期间不被修改。

use crate::kernel::mm::vma::{Vma, VmaType, MmStruct};
use crate::kernel::mm::{PageFlags, PAGE_SIZE, VirtAddr};

const ELF_MAGIC: &[u8; 4] = b"\x7FELF";
const ELF_CLASS_64: u8 = 2;

#[repr(C)]
pub struct Elf64Header {
    pub e_ident: [u8; 16],
    pub e_type: u16,
    pub e_machine: u16,
    pub e_version: u32,
    pub e_entry: u64,
    pub e_phoff: u64,
    pub e_shoff: u64,
    pub e_flags: u32,
    pub e_ehsize: u16,
    pub e_phentsize: u16,
    pub e_phnum: u16,
    pub e_shentsize: u16,
    pub e_shnum: u16,
    pub e_shstrndx: u16,
}

#[repr(C)]
pub struct Elf64Phdr {
    pub p_type: u32,
    pub p_flags: u32,
    pub p_offset: u64,
    pub p_vaddr: u64,
    pub p_paddr: u64,
    pub p_filesz: u64,
    pub p_memsz: u64,
    pub p_align: u64,
}

const PT_LOAD: u32 = 1;
const PT_GNU_STACK: u32 = 0x6474E551;
const PF_X: u32 = 1;
const PF_W: u32 = 2;
const PF_R: u32 = 4;

pub struct ElfLoadResult {
    pub entry: u64,
    pub phdr_addr: u64,
    pub phdr_count: u16,
    pub brk_base: u64,
    pub stack_top: u64,
}

impl ElfLoadResult {
    pub const fn empty() -> Self {
        Self {
            entry: 0,
            phdr_addr: 0,
            phdr_count: 0,
            brk_base: 0,
            stack_top: 0,
        }
    }
}

pub fn elf_validate(elf_data: *const u8, elf_size: u64) -> Option<&'static Elf64Header> {
    if elf_data.is_null() || elf_size < core::mem::size_of::<Elf64Header>() as u64 {
        return None;
    }

    let header = unsafe { &*(elf_data as *const Elf64Header) };

    if &header.e_ident[0..4] != ELF_MAGIC {
        return None;
    }
    if header.e_ident[4] != ELF_CLASS_64 {
        return None;
    }
    if header.e_machine != 0x3E && header.e_machine != 0xB7 {
        return None;
    }
    if header.e_phentsize as usize != core::mem::size_of::<Elf64Phdr>() {
        return None;
    }

    Some(header)
}

pub fn elf_load(
    mm: &MmStruct,
    elf_data: *const u8,
    elf_size: u64,
) -> Result<ElfLoadResult, &'static str> {
    let header = elf_validate(elf_data, elf_size).ok_or("Invalid ELF header")?;

    let entry = header.e_entry;

    if header.e_phoff == 0 || header.e_phnum == 0 {
        return Err("No program headers");
    }

    let phdr_base = unsafe { elf_data.add(header.e_phoff as usize) };
    let phdr_count = header.e_phnum;

    let mut max_vaddr: u64 = 0;
    let mut result = ElfLoadResult {
        entry,
        phdr_addr: phdr_base as u64,
        phdr_count,
        brk_base: 0,
        stack_top: 0x0000_7FFF_FFFF_F000,
    };

    for i in 0..phdr_count {
        let phdr = unsafe {
            &*(phdr_base.add(i as usize * core::mem::size_of::<Elf64Phdr>()) as *const Elf64Phdr)
        };

        if phdr.p_type != PT_LOAD {
            continue;
        }

        if phdr.p_memsz == 0 {
            continue;
        }

        let vaddr_start = phdr.p_vaddr & !(PAGE_SIZE - 1);
        let vaddr_end = ((phdr.p_vaddr + phdr.p_memsz + PAGE_SIZE - 1) & !(PAGE_SIZE - 1)) as usize;
        let filesz = phdr.p_filesz as usize;
        let file_offset = phdr.p_offset as usize;

        if vaddr_end as u64 > max_vaddr {
            max_vaddr = vaddr_end as u64;
        }

        let mut page_flags = PageFlags::USER;
        if phdr.p_flags & PF_R != 0 { page_flags |= PageFlags::PRESENT; }
        if phdr.p_flags & PF_W != 0 { page_flags |= PageFlags::WRITABLE; }
        if phdr.p_flags & PF_X == 0 { page_flags |= PageFlags::NX; }

        let vma = Vma::new(vaddr_start as usize, vaddr_end, page_flags, VmaType::Anonymous);
        mm.insert_vma(vma).map_err(|_| "VMA insertion failed")?;

        // 复制段数据到物理页
        let vmm_inst = crate::kernel::mm::vmm::get_vmm();
        let pml4 = crate::kernel::mm::vmm::get_current_pml4();

        let file_end = file_offset + filesz;
        let mut cur = vaddr_start;

        while cur < vaddr_end as u64 {
            let pmm_inst = crate::kernel::mm::pmm::get_pmm();
            let phys = pmm_inst.alloc_page().ok_or("OOM loading ELF")?;

            let page_virt = phys.to_virt();
            unsafe {
                core::ptr::write_bytes(page_virt.0 as *mut u8, 0, PAGE_SIZE as usize);
            }

            let copy_start = if (cur as usize) < file_end {
                file_offset + (cur - vaddr_start) as usize
            } else {
                file_end
            };
            let copy_len = if copy_start < file_end {
                (file_end - copy_start).min(PAGE_SIZE as usize)
            } else {
                0
            };

            if copy_len > 0 {
                unsafe {
                    core::ptr::copy_nonoverlapping(
                        elf_data.add(copy_start),
                        page_virt.0 as *mut u8,
                        copy_len,
                    );
                }
            }

            vmm_inst.map_page_in_table(pml4, VirtAddr(cur), phys, page_flags | PageFlags::PRESENT);
            cur += PAGE_SIZE;
        }
    }

    result.brk_base = (max_vaddr + PAGE_SIZE - 1) & !(PAGE_SIZE - 1);

    // 连接 MmStruct 到当前进程
    crate::kernel::mm::vma::set_current_mm(mm as *const MmStruct);

    Ok(result)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_elf_magic_validation() {
        let mut data = [0u8; 64];
        // Invalid magic
        assert!(elf_validate(data.as_ptr(), 64).is_none());

        // Valid magic
        data[0] = 0x7F;
        data[1] = b'E';
        data[2] = b'L';
        data[3] = b'F';
        data[4] = 2;   // ELFCLASS64
        assert!(elf_validate(data.as_ptr(), 64).is_none()); // machine=0 not valid

        // Set valid machine
        let hdr = unsafe { &mut *(data.as_mut_ptr() as *mut Elf64Header) };
        hdr.e_machine = 0x3E; // x86_64
        hdr.e_phentsize = core::mem::size_of::<Elf64Phdr>() as u16;
        assert!(elf_validate(data.as_ptr(), 64).is_some());
    }

    #[test]
    fn test_elf64_header_sizes() {
        assert_eq!(core::mem::size_of::<Elf64Header>(), 64);
        assert_eq!(core::mem::size_of::<Elf64Phdr>(), 56);
    }
}
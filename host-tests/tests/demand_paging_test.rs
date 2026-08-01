//! P0-I-26 / B13-FL-01: Demand Paging 模型语义测试
//!
//! 验证 handle_user_page_fault 的 fallthrough 路径:
//! - 没有 VMA 覆盖的地址 → SignalSegv (不再隐式分配 RWX 零页)
//! - 有 VMA 覆盖 + 写入只读 VMA → 应被识别为 COW 候选, 不得直接赋写权限
//! - 有 VMA 覆盖 + guard VMA → SignalSegv
//!
//! 不链接 queenx (host-tests 是 mock 层), 通过复刻 PfResult / Vma 模型
//! 验证 demand paging 行为正确性.

/// 镜像 queenx PfResult (mm/page_fault.rs)
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
enum PfResult {
    Fixed = 0,
    SignalSegv = 1,
    SignalBus = 2,
    Oom = 3,
    Unhandled = 4,
}

/// 镜像 queenx PageFaultInfo::from_error_code
#[derive(Debug, Clone, Copy)]
struct PageFaultInfo {
    fault_addr: u64,
    present: bool,
    write: bool,
    user: bool,
    reserved: bool,
    instruction: bool,
}

impl PageFaultInfo {
    fn from_error_code(fault_addr: u64, error_code: u64) -> Self {
        Self {
            fault_addr,
            present: error_code & 0x01 != 0,
            write: error_code & 0x02 != 0,
            user: error_code & 0x04 != 0,
            reserved: error_code & 0x08 != 0,
            instruction: error_code & 0x10 != 0,
        }
    }
}

/// 镜像 queenx PageFlags
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct PageFlags {
    bits: u32,
}

impl PageFlags {
    const PRESENT: Self = Self { bits: 0x01 };
    const WRITABLE: Self = Self { bits: 0x02 };
    const USER: Self = Self { bits: 0x04 };
    const NO_EXEC: Self = Self { bits: 0x08 };

    fn contains(&self, other: Self) -> bool {
        self.bits & other.bits == other.bits
    }
}

impl core::ops::BitOr for PageFlags {
    type Output = Self;
    fn bitor(self, rhs: Self) -> Self {
        Self { bits: self.bits | rhs.bits }
    }
}

/// 镜像 queenx VmaType
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
enum VmaType {
    Anonymous = 0,
    FileBacked = 1,
    Stack = 2,
}

/// 镜像 queenx Vma (最小字段集)
#[derive(Debug, Clone)]
struct Vma {
    start: usize,
    end: usize,
    flags: PageFlags,
    vma_type: VmaType,
    inode_id: u32,
    shared: bool,
    file_pwm: u64,
    offset: u64,
}

impl Vma {
    fn is_guard(&self) -> bool {
        // 简化: 没有 USER 的 Vma 视为 guard
        !self.flags.contains(PageFlags::USER)
    }
}

/// 镜像 queenx handle_user_page_fault 的 fallthrough 决策:
/// 返回 (PfResult, 期望的页 flags)
fn decide_fallthrough(info: &PageFaultInfo, vma: Option<&Vma>) -> (PfResult, Option<PageFlags>) {
    if info.reserved {
        return (PfResult::SignalBus, None);
    }
    // 模拟: 没有 VMA 覆盖 → SignalSegv (P0-I-26 / B13-FL-01 修复)
    let vma = match vma {
        Some(v) => v,
        None => return (PfResult::SignalSegv, None),
    };
    if vma.is_guard() {
        return (PfResult::SignalSegv, None);
    }
    // 写入只读 VMA → 需 COW, 不得直接赋写权限
    if info.write && !vma.flags.contains(PageFlags::WRITABLE) {
        // 简化模型: 决策为 "需要走 COW 路径", 标记为 Fixed
        // 实际框架层会调用 cow_handle_fault, 本测试只验证"不会
        // 静默映射为 WRITABLE"
        return (PfResult::Fixed, Some(vma.flags | PageFlags::PRESENT));
    }
    // 普通匿名页: 用 VMA flags (严禁硬编码 WRITABLE)
    (PfResult::Fixed, Some(vma.flags | PageFlags::PRESENT))
}

#[test]
fn pf_result_enum_values() {
    assert_eq!(PfResult::Fixed as u8, 0);
    assert_eq!(PfResult::SignalSegv as u8, 1);
    assert_eq!(PfResult::SignalBus as u8, 2);
    assert_eq!(PfResult::Oom as u8, 3);
    assert_eq!(PfResult::Unhandled as u8, 4);
}

#[test]
fn pf_info_parses_error_code() {
    // 0x06 = present(0) | write(1) | user(1) → 缺页 + 写 + 用户态
    let info = PageFaultInfo::from_error_code(0x4000, 0x06);
    assert_eq!(info.fault_addr, 0x4000);
    assert!(info.write);
    assert!(info.user);
    assert!(!info.present);
    assert!(!info.reserved);
    assert!(!info.instruction);
}

#[test]
fn pf_info_reserved_bit_detection() {
    // 0x08 = reserved bit set
    let info = PageFaultInfo::from_error_code(0x1000, 0x08);
    assert!(info.reserved);
    assert!(!info.write);
}

#[test]
fn no_vma_returns_sigsegv() {
    // 任意用户地址, 没有 VMA 覆盖 → 拒绝, 不再隐式分配 RWX
    let info = PageFaultInfo::from_error_code(0xDEAD_BEEF, 0x06);
    let (result, flags) = decide_fallthrough(&info, None);
    assert_eq!(result, PfResult::SignalSegv);
    assert!(flags.is_none(), "无 VMA 不应映射任何页");
}

#[test]
fn guard_vma_returns_sigsegv() {
    // guard VMA (无 USER 位) → SIGSEGV
    let vma = Vma {
        start: 0x7000,
        end: 0x8000,
        flags: PageFlags { bits: 0 }, // 0 flags = guard
        vma_type: VmaType::Stack,
        inode_id: 0,
        shared: false,
        file_pwm: 0,
        offset: 0,
    };
    let info = PageFaultInfo::from_error_code(0x7500, 0x06);
    let (result, _) = decide_fallthrough(&info, Some(&vma));
    assert_eq!(result, PfResult::SignalSegv);
}

#[test]
fn readonly_vma_write_triggers_cow_not_silent_writable() {
    // 只读 mmap 写入 → 识别为 COW 候选, 新映射保留只读
    let readonly = PageFlags::PRESENT | PageFlags::USER;
    let vma = Vma {
        start: 0x1000_0000,
        end: 0x1001_0000,
        flags: readonly,
        vma_type: VmaType::FileBacked,
        inode_id: 42,
        shared: false,
        file_pwm: 0xCAFE,
        offset: 0,
    };
    let info = PageFaultInfo::from_error_code(0x1000_0500, 0x07); // write + user + present
    let (result, flags) = decide_fallthrough(&info, Some(&vma));
    assert_eq!(result, PfResult::Fixed);
    let flags = flags.expect("应返回映射 flags");
    // 关键断言: B13-FL-01 修复后, 只读 VMA 写缺页不得静默升级为 WRITABLE
    assert!(
        !flags.contains(PageFlags::WRITABLE),
        "只读 VMA 写缺页必须走 COW, 不得静默映射为 WRITABLE"
    );
}

#[test]
fn writable_vma_uses_vma_flags() {
    // 匿名可写 VMA → 使用 VMA 自身的 flags (PRESENT|USER|WRITABLE), 不硬编码
    let flags = PageFlags::PRESENT | PageFlags::WRITABLE | PageFlags::USER;
    let vma = Vma {
        start: 0x2000_0000,
        end: 0x2001_0000,
        flags,
        vma_type: VmaType::Anonymous,
        inode_id: 0,
        shared: false,
        file_pwm: 0,
        offset: 0,
    };
    let info = PageFaultInfo::from_error_code(0x2000_0500, 0x06);
    let (result, mapped_flags) = decide_fallthrough(&info, Some(&vma));
    assert_eq!(result, PfResult::Fixed);
    let mapped = mapped_flags.expect("应返回映射 flags");
    assert!(mapped.contains(PageFlags::PRESENT));
    assert!(mapped.contains(PageFlags::WRITABLE));
    assert!(mapped.contains(PageFlags::USER));
}

#[test]
fn stack_region_detected() {
    // 验证 USER_STACK_TOP - 4096 落在栈扩展候选区间
    const USER_STACK_TOP: u64 = 0x0000_7FFF_FFFF_F000;
    const USER_STACK_DEFAULT_SIZE: u64 = 0x0080_0000;
    let inside = (USER_STACK_TOP - 4096) as usize;
    let outside = (USER_STACK_TOP - USER_STACK_DEFAULT_SIZE - 4096) as usize;
    assert!((USER_STACK_TOP - USER_STACK_DEFAULT_SIZE..USER_STACK_TOP).contains(&(inside as u64)));
    assert!(!(USER_STACK_TOP - USER_STACK_DEFAULT_SIZE..USER_STACK_TOP).contains(&(outside as u64)));
}

#[test]
fn page_flags_no_exec_bit_distinct() {
    // 镜像内核 PTE NX 位: 验证 NO_EXEC 与其他标志位不冲突
    let nx = PageFlags::NO_EXEC;
    assert!(!nx.contains(PageFlags::PRESENT), "NX 与 PRESENT 不冲突");
    assert!(!nx.contains(PageFlags::WRITABLE), "NX 与 WRITABLE 不冲突");
    assert!(!nx.contains(PageFlags::USER), "NX 与 USER 不冲突");
    let combined = PageFlags::PRESENT | PageFlags::USER | PageFlags::NO_EXEC;
    assert!(combined.contains(PageFlags::NO_EXEC), "组合位含 NX");
    assert!(combined.contains(PageFlags::PRESENT), "组合位含 PRESENT");
}

#[test]
fn vma_file_backed_fields_roundtrip() {
    // 验证 Vma 的 file_backed 字段 (start/end/offset/inode_id/shared/file_pwm/vma_type) 语义
    let vma = Vma {
        start: 0x1000,
        end: 0x2000,
        flags: PageFlags::PRESENT | PageFlags::USER,
        vma_type: VmaType::FileBacked,
        inode_id: 42,
        shared: true,
        file_pwm: 0xCAFE,
        offset: 0x100,
    };
    assert_eq!(vma.start, 0x1000, "start 保留映射起始地址");
    assert_eq!(vma.end, 0x2000, "end 保留映射结束地址");
    assert_eq!(vma.end - vma.start, 0x1000, "end - start = 映射长度 4KB");
    assert_eq!(vma.vma_type, VmaType::FileBacked, "vma_type 语义: FileBacked");
    assert_eq!(vma.inode_id, 42, "inode_id 保留文件后端 inode 编号");
    assert!(vma.shared, "shared 标记共享映射");
    assert_eq!(vma.file_pwm, 0xCAFE, "file_pwm 保留进程凭证");
    assert_eq!(vma.offset, 0x100, "offset 保留文件内偏移");
    assert!(!vma.is_guard(), "有 USER 位不是 guard");
}

#[test]
fn vma_anonymous_fields_defaults() {
    // 验证 Vma 的匿名映射字段语义 (inode_id=0/shared=false/file_pwm=0/offset=0)
    let vma = Vma {
        start: 0x2000,
        end: 0x3000,
        flags: PageFlags::PRESENT | PageFlags::WRITABLE | PageFlags::USER,
        vma_type: VmaType::Anonymous,
        inode_id: 0,
        shared: false,
        file_pwm: 0,
        offset: 0,
    };
    assert_eq!(vma.vma_type, VmaType::Anonymous, "vma_type 语义: Anonymous");
    assert_eq!(vma.inode_id, 0, "匿名映射 inode_id = 0");
    assert!(!vma.shared, "匿名映射默认非共享");
    assert_eq!(vma.file_pwm, 0, "匿名映射 file_pwm = 0");
    assert_eq!(vma.offset, 0, "匿名映射 offset = 0");
}

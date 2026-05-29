use crate::kernel::chitin::devtree::{
    devtree_get_node,
    devtree_set_user_mapped, devtree_clear_user_mapped, devtree_get_user_mapped,
    PropertyValue, NodeId,
};
use crate::kernel::chitin::{ChitinProto, DeviceState};
use crate::kernel::credo::capability::{
    CAP_DOMAIN_DEVICE, DEVICE_CAP_MMIO, DEVICE_CAP_IRQ, DEVICE_CAP_BIND,
};
use crate::kernel::credo::types::{CapDomain, CapBits};
use crate::kernel::credo::engine;
use crate::kernel::mm::{VirtAddr, PhysAddr, PageFlags, PAGE_SIZE};
use crate::kernel::mm::vmm::get_vmm;
use crate::kernel::mm::vma::{MmStruct, Vma, VmaType};
use crate::kernel::proc::process::PROCESS_TABLE;
use crate::klog_info;
use crate::klog_warn;
use core::sync::atomic::Ordering;

const MAX_MMIO_SIZE: usize = 256 * 1024 * 1024;

pub struct UserDriverError {
    code: i32,
}

impl UserDriverError {
    pub const fn new(code: i32) -> Self { Self { code } }
    pub const fn code(&self) -> i32 { self.code }
}

pub const ERR_OK: i32 = 0;
pub const ERR_NOT_FOUND: i32 = -1;
pub const ERR_NOT_AUTHORIZED: i32 = -2;
pub const ERR_INVALID_STATE: i32 = -3;
pub const ERR_NO_MMIO: i32 = -4;
pub const ERR_PID_MISMATCH: i32 = -5;
pub const ERR_OOM: i32 = -6;

fn has_device_cap(pwm: u64, required: u64) -> bool {
    engine::check(pwm, CapDomain(CAP_DOMAIN_DEVICE), CapBits(required))
}

pub fn devtree_bind_user_device(
    node_id: NodeId,
    pid: u32,
    pwm: u64,
) -> Result<(), UserDriverError> {
    if !has_device_cap(pwm, DEVICE_CAP_BIND) {
        klog_warn!(Driver, "Chitin: PWM {} lacks DEVICE_CAP_BIND for node {}", pwm, node_id);
        return Err(UserDriverError::new(ERR_NOT_AUTHORIZED));
    }

    if PROCESS_TABLE.get(pid).is_none() {
        return Err(UserDriverError::new(ERR_NOT_FOUND));
    }

    let node = match devtree_get_node(node_id) {
        Some(n) => n,
        None => return Err(UserDriverError::new(ERR_NOT_FOUND)),
    };

    if node.state != DeviceState::Ready {
        return Err(UserDriverError::new(ERR_INVALID_STATE));
    }

    if node.proto == ChitinProto::Bus {
        return Err(UserDriverError::new(ERR_INVALID_STATE));
    }

    if node.user_mapped.is_some() {
        return Err(UserDriverError::new(ERR_INVALID_STATE));
    }

    devtree_set_user_mapped(node_id, pid);

    klog_info!(Driver, "Chitin: device node {} bound to user pid={} pwm={}",
        node_id, pid, pwm);
    Ok(())
}

pub fn devtree_unbind_user_device(
    node_id: NodeId,
    pid: u32,
    pwm: u64,
) -> Result<(), UserDriverError> {
    if !has_device_cap(pwm, DEVICE_CAP_BIND) {
        return Err(UserDriverError::new(ERR_NOT_AUTHORIZED));
    }

    let node = match devtree_get_node(node_id) {
        Some(n) => n,
        None => return Err(UserDriverError::new(ERR_NOT_FOUND)),
    };

    match node.user_mapped {
        Some(mapped_pid) if mapped_pid == pid => {
            devtree_clear_user_mapped(node_id);
            Ok(())
        }
        Some(_) => Err(UserDriverError::new(ERR_PID_MISMATCH)),
        None => Err(UserDriverError::new(ERR_INVALID_STATE)),
    }
}

pub fn devtree_map_user_device(
    node_id: NodeId,
    pid: u32,
    pwm: u64,
    mm: &MmStruct,
) -> Result<usize, UserDriverError> {
    if !has_device_cap(pwm, DEVICE_CAP_MMIO) {
        return Err(UserDriverError::new(ERR_NOT_AUTHORIZED));
    }

    let node = match devtree_get_node(node_id) {
        Some(n) => n,
        None => return Err(UserDriverError::new(ERR_NOT_FOUND)),
    };

    match node.user_mapped {
        Some(mapped_pid) if mapped_pid == pid => {}
        Some(_) => return Err(UserDriverError::new(ERR_PID_MISMATCH)),
        None => return Err(UserDriverError::new(ERR_INVALID_STATE)),
    }

    let phys_addr: u64 = match node.get_prop("reg") {
        Some(PropertyValue::U64(addr)) => *addr,
        Some(PropertyValue::U32(addr)) => *addr as u64,
        _ => return Err(UserDriverError::new(ERR_NO_MMIO)),
    };

    let size: u64 = match node.get_prop("size") {
        Some(PropertyValue::U64(sz)) => *sz,
        Some(PropertyValue::U32(sz)) => *sz as u64,
        _ => 4096,
    };

    if size == 0 || phys_addr == 0 {
        return Err(UserDriverError::new(ERR_NO_MMIO));
    }

    let clamped_size = (size as usize).min(MAX_MMIO_SIZE);
    let pages = (clamped_size + PAGE_SIZE as usize - 1) / PAGE_SIZE as usize;

    let map_base = match mm.find_free_range(pages * PAGE_SIZE as usize) {
        Some(addr) => addr,
        None => return Err(UserDriverError::new(ERR_OOM)),
    };

    let page_flags = PageFlags::PRESENT
        | PageFlags::WRITABLE
        | PageFlags::USER
        | PageFlags::CACHE_DISABLE;

    let cr3 = match PROCESS_TABLE.with_process(pid, |proc| proc.cr3.load(Ordering::SeqCst)) {
        Some(c) => c,
        None => return Err(UserDriverError::new(ERR_NOT_FOUND)),
    };

    let vmm = get_vmm();
    for i in 0..pages {
        let page_phys = PhysAddr(phys_addr + (i as u64 * PAGE_SIZE));
        let page_virt = VirtAddr((map_base + i * PAGE_SIZE as usize) as u64);
        vmm.map_page_in_table(cr3, page_virt, page_phys, page_flags);
    }

    let vma = Vma::new(
        map_base,
        map_base + pages * PAGE_SIZE as usize,
        page_flags,
        VmaType::Anonymous,
    );
    if mm.insert_vma(vma).is_err() {
        for i in 0..pages {
            let unmap_virt = VirtAddr((map_base + i * PAGE_SIZE as usize) as u64);
            vmm.map_page_in_table(cr3, unmap_virt, PhysAddr(0), PageFlags::empty());
        }
        return Err(UserDriverError::new(ERR_OOM));
    }

    klog_info!(Driver, "Chitin: mapped MMIO for node {} (phys=0x{:X} size={}) → user VA=0x{:X}",
        node_id, phys_addr, clamped_size, map_base);
    Ok(map_base)
}

pub fn devtree_unmap_user_device(
    node_id: NodeId,
    pid: u32,
    pwm: u64,
    mm: &MmStruct,
    virt_addr: usize,
    size: usize,
) -> Result<(), UserDriverError> {
    if !has_device_cap(pwm, DEVICE_CAP_MMIO) {
        return Err(UserDriverError::new(ERR_NOT_AUTHORIZED));
    }

    let node = match devtree_get_node(node_id) {
        Some(n) => n,
        None => return Err(UserDriverError::new(ERR_NOT_FOUND)),
    };

    match node.user_mapped {
        Some(mapped_pid) if mapped_pid == pid => {}
        Some(_) => return Err(UserDriverError::new(ERR_PID_MISMATCH)),
        None => return Err(UserDriverError::new(ERR_INVALID_STATE)),
    }

    let pages = (size + PAGE_SIZE as usize - 1) / PAGE_SIZE as usize;

    let cr3 = match PROCESS_TABLE.with_process(pid, |proc| proc.cr3.load(Ordering::SeqCst)) {
        Some(c) => c,
        None => return Err(UserDriverError::new(ERR_NOT_FOUND)),
    };

    let vmm = get_vmm();
    for i in 0..pages {
        let page_virt = VirtAddr((virt_addr + i * PAGE_SIZE as usize) as u64);
        vmm.map_page_in_table(cr3, page_virt, PhysAddr(0), PageFlags::empty());
    }

    mm.remove_range(virt_addr, virt_addr + pages * PAGE_SIZE as usize)
        .map_err(|_| UserDriverError::new(ERR_OOM))?;

    Ok(())
}

pub fn chitin_forward_irq(node_id: NodeId) -> bool {
    let pid = match devtree_get_user_mapped(node_id) {
        Some(p) => p,
        None => return false,
    };

    let pwm = match PROCESS_TABLE.with_process(pid, |proc| proc.get_pwm()) {
        Some(p) => p,
        None => return false,
    };

    if !has_device_cap(pwm, DEVICE_CAP_IRQ) {
        return false;
    }

    process_signal_pending_set(pid, 10);

    true
}

#[no_mangle]
pub extern "C" fn user_driver_bind_c(node_id: u32, pid: u32, pwm: u64) -> i32 {
    match devtree_bind_user_device(node_id, pid, pwm) {
        Ok(()) => 0,
        Err(e) => e.code(),
    }
}

#[no_mangle]
pub extern "C" fn user_driver_unbind_c(node_id: u32, pid: u32, pwm: u64) -> i32 {
    match devtree_unbind_user_device(node_id, pid, pwm) {
        Ok(()) => 0,
        Err(e) => e.code(),
    }
}

/// Stub for user-driver interrupt signal delivery.
/// Sets SIGUSR1 pending on the target process. Full signal dispatch
/// integration will deliver it on return to userspace.
#[no_mangle]
pub extern "C" fn process_signal_pending_set(pid: u32, sig: u32) {
    let _ = (pid, sig);
    // TODO: integrate with signal dispatch framework in kernel/ipc/signal.rs
    // For now, this is a no-op — the user driver IRQ forwarding path
    // is ready and will activate when signal delivery is implemented.
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_user_driver_error_codes() {
        assert_ne!(ERR_OK, ERR_NOT_FOUND);
        assert_ne!(ERR_NOT_AUTHORIZED, ERR_PID_MISMATCH);
        assert_ne!(ERR_INVALID_STATE, ERR_OOM);
    }

    #[test]
    fn test_user_driver_error_display() {
        let e = UserDriverError::new(ERR_NOT_FOUND);
        assert_eq!(e.code(), ERR_NOT_FOUND);
    }
}

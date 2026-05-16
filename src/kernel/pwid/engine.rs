use super::types::*;
use super::table;
use core::sync::atomic::Ordering;

pub fn check(pwid: u64, domain: CapDomain, required: CapBits) -> bool {
    let entry = match table::find(pwid) {
        Some(e) => e,
        None => return false,
    };

    if entry.has_flag(PwidFlags::DISABLED) {
        return false;
    }

    entry.has_capability(domain, required)
}

pub fn check_privilege(operator_pwid: u64, target_pwid: u64) -> bool {
    let operator = match table::find(operator_pwid) {
        Some(e) => e,
        None => return false,
    };
    let target = match table::find(target_pwid) {
        Some(e) => e,
        None => return false,
    };

    let op_level = operator.privilege_level.load(Ordering::Acquire);
    let tgt_level = target.privilege_level.load(Ordering::Acquire);

    op_level < tgt_level
}

pub fn get_privilege_level(pwid: u64) -> u8 {
    match table::find(pwid) {
        Some(e) => e.privilege_level.load(Ordering::Acquire),
        None => 0xFF,
    }
}

pub fn get_creator(pwid: u64) -> u64 {
    match table::find(pwid) {
        Some(e) => e.creator_pwid.load(Ordering::Acquire),
        None => 0,
    }
}

pub fn get_caps(pwid: u64, domain: impl Into<CapDomain>) -> CapBits {
    let domain = domain.into();
    match table::find(pwid) {
        Some(e) => e.load_caps(domain),
        None => CapBits::NONE,
    }
}

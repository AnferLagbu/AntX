use super::types::*;
use super::identity;
use core::sync::atomic::Ordering;

pub fn check(pwm: u64, domain: CapDomain, required: CapBits) -> bool {
    let entry = match identity::find(pwm) {
        Some(e) => e,
        None => return false,
    };

    if entry.has_flag(PwmFlags::DISABLED) {
        return false;
    }

    entry.has_capability(domain, required)
}

pub fn check_privilege(operator_pwm: u64, target_pwm: u64) -> bool {
    let operator = match identity::find(operator_pwm) {
        Some(e) => e,
        None => return false,
    };
    let target = match identity::find(target_pwm) {
        Some(e) => e,
        None => return false,
    };

    let op_level = operator.privilege_level.load(Ordering::Acquire);
    let tgt_level = target.privilege_level.load(Ordering::Acquire);

    op_level < tgt_level
}

pub fn get_privilege_level(pwm: u64) -> u8 {
    match identity::find(pwm) {
        Some(e) => e.privilege_level.load(Ordering::Acquire),
        None => 0xFF,
    }
}

pub fn get_creator(pwm: u64) -> u64 {
    match identity::find(pwm) {
        Some(e) => e.creator_pwm.load(Ordering::Acquire),
        None => 0,
    }
}

pub fn get_caps(pwm: u64, domain: impl Into<CapDomain>) -> CapBits {
    let domain = domain.into();
    match identity::find(pwm) {
        Some(e) => e.load_caps(domain),
        None => CapBits::NONE,
    }
}

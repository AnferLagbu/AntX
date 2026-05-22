use crate::kernel::pwid::sha256;
use crate::kernel::pwid::types::*;
use crate::kernel::pwid::engine;
use crate::kernel::pwid::capability;
use crate::kernel::tests::{runner, TestResult};
use super::check;

fn test_sha256_vectors() -> TestResult {
    let hash = sha256::sha256(b"");
    check!(hash[0] == 0xe3, "SHA-256('') byte 0 mismatch");
    check!(hash[1] == 0xb0, "SHA-256('') byte 1 mismatch");
    check!(hash[2] == 0xc4, "SHA-256('') byte 2 mismatch");

    let hash2 = sha256::sha256(b"abc");
    check!(hash2[0] == 0xba, "SHA-256('abc') byte 0 mismatch");
    check!(hash2[1] == 0x78, "SHA-256('abc') byte 1 mismatch");
    TestResult::Pass
}

fn test_pwid_id_newtype() -> TestResult {
    let id = PwidId(42);
    check!(id.is_valid(), "non-zero PwidId should be valid");
    check!(id.as_u64() == 42, "as_u64 mismatch");

    let zero = PwidId::ZERO;
    check!(!zero.is_valid(), "zero PwidId should be invalid");
    check!(zero.as_u64() == 0, "ZERO as_u64 mismatch");

    let test = PwidId::TEST;
    check!(test.is_valid(), "TEST PwidId should be valid");
    TestResult::Pass
}

fn test_cap_domain_newtype() -> TestResult {
    let fs = CapDomain::FS;
    check!(fs.as_u16() == 1, "FS domain should be 1");
    check!(fs.as_usize() == 1, "FS domain usize should be 1");

    let from_raw: CapDomain = 2u16.into();
    check!(from_raw == CapDomain::NET, "u16->CapDomain should be NET");

    let sys = CapDomain::SYSTEM;
    check!(sys.as_usize() == 0, "SYSTEM domain usize should be 0");
    TestResult::Pass
}

fn test_cap_bits_newtype() -> TestResult {
    let none = CapBits::NONE;
    check!(none.as_u64() == 0, "NONE should be 0");

    let all = CapBits::ALL;
    check!(all.as_u64() == u64::MAX, "ALL should be u64::MAX");

    let read = CapBits(capability::FS_CAP_READ);
    let write = CapBits(capability::FS_CAP_WRITE);
    let rw = read | write;
    check!(rw.contains(read), "rw should contain read");
    check!(rw.contains(write), "rw should contain write");
    check!(!read.contains(write), "read should not contain write");

    let mut caps = CapBits::NONE;
    caps |= read;
    check!(caps.contains(read), "after |= should contain read");
    caps &= !read;
    check!(!caps.contains(read), "after &= ! should not contain read");
    TestResult::Pass
}

fn test_pwidentry_caps() -> TestResult {
    let entry = PwidEntry::new();
    check!(!entry.is_valid(), "new entry should not be valid");

    entry.pwid.store(123, core::sync::atomic::Ordering::Release);
    check!(entry.is_valid(), "entry with pwid should be valid");
    check!(entry.get_pwid() == PwidId(123), "get_pwid mismatch");

    let fs_caps = entry.load_caps(CapDomain::FS);
    check!(fs_caps == CapBits::NONE, "new entry fs caps should be NONE");

    entry.fetch_or_caps(CapDomain::FS, CapBits(capability::FS_CAP_READ | capability::FS_CAP_WRITE));
    let after = entry.load_caps(CapDomain::FS);
    check!(after.contains(CapBits(capability::FS_CAP_READ)), "should have FS_READ");
    check!(after.contains(CapBits(capability::FS_CAP_WRITE)), "should have FS_WRITE");
    check!(!after.contains(CapBits(capability::FS_CAP_EXECUTE)), "should not have FS_EXEC");

    check!(entry.has_capability(CapDomain::FS, CapBits(capability::FS_CAP_READ)), "has_capability should be true");
    check!(!entry.has_capability(CapDomain::FS, CapBits(capability::FS_CAP_DELETE)), "has_capability DELETE should be false");
    TestResult::Pass
}

fn test_pwidentry_flags() -> TestResult {
    let entry = PwidEntry::new();
    check!(!entry.has_flag(PwidFlags::DISABLED), "new entry should not be disabled");

    entry.add_flags(PwidFlags::DISABLED);
    check!(entry.has_flag(PwidFlags::DISABLED), "should be disabled after add");

    check!(engine::check(entry.pwid.load(core::sync::atomic::Ordering::Acquire), CapDomain::FS, CapBits::ALL) == false, "disabled entry should fail check");

    entry.remove_flags(PwidFlags::DISABLED);
    check!(!entry.has_flag(PwidFlags::DISABLED), "should not be disabled after remove");
    TestResult::Pass
}

fn test_grant_record() -> TestResult {
    let rec = GrantRecord {
        grantor_pwid: PwidId(1),
        grantee_pwid: PwidId(2),
        domain: CapDomain::FS,
        caps: CapBits(capability::FS_CAP_READ),
        granted_at: 100,
    };
    check!(!rec.is_empty(), "filled record should not be empty");

    let empty = GrantRecord::EMPTY;
    check!(empty.is_empty(), "EMPTY record should be empty");
    check!(empty.grantor_pwid == PwidId::ZERO, "EMPTY grantor should be ZERO");
    TestResult::Pass
}

fn test_audit_entry() -> TestResult {
    let entry = AuditEntry {
        timestamp: 1000,
        pwid: PwidId(42),
        action: AuditAction::Create,
        result: AuditResult::Success,
        target_pwid: PwidId(0),
        details: 0,
    };
    check!(entry.pwid.as_u64() == 42, "audit pwid mismatch");
    check!(entry.action.as_u32() == 3, "Create action should be 3");
    check!(entry.result.as_u32() == 0, "Success result should be 0");
    TestResult::Pass
}

fn test_pwidentry_note() -> TestResult {
    let mut entry = PwidEntry::new();
    entry.set_note("test-identity");
    let note = entry.get_note_str();
    check!(note == "test-identity", "note mismatch");
    TestResult::Pass
}

fn test_viable_floor() -> TestResult {
    check!(capability::VIABLE_FLOOR[CapDomain::FS.as_usize()] != 0, "FS viable floor should be non-zero");
    check!(capability::VIABLE_FLOOR[CapDomain::PROC.as_usize()] != 0, "PROC viable floor should be non-zero");
    check!(capability::VIABLE_FLOOR[CapDomain::SYSTEM.as_usize()] == 0, "SYSTEM viable floor should be zero");
    TestResult::Pass
}

#[cfg(target_arch = "x86_64")]
fn test_pwidentry_cow_bp() -> TestResult {
    use crate::kernel::fs::hvfs::bp::HvBlockPointer;
    use crate::kernel::fs::hvfs::dmu::HvDmuObject;

    let mut obj = HvDmuObject::new_file(1, 0);
    let bp = HvBlockPointer::null();
    obj.cow_bp(bp, 5);
    check!(obj.birth_txg == 5, "birth txg should be 5 after cow_bp");
    TestResult::Pass
}

pub fn register_pwid_tests() {
    let r = runner();
    r.register("pwid::sha256", "known_vectors", test_sha256_vectors);
    r.register("pwid::types", "pwid_id_newtype", test_pwid_id_newtype);
    r.register("pwid::types", "cap_domain_newtype", test_cap_domain_newtype);
    r.register("pwid::types", "cap_bits_newtype", test_cap_bits_newtype);
    r.register("pwid::entry", "caps", test_pwidentry_caps);
    r.register("pwid::entry", "flags", test_pwidentry_flags);
    r.register("pwid::grant_record", "basic", test_grant_record);
    r.register("pwid::audit", "entry", test_audit_entry);
    r.register("pwid::entry", "note", test_pwidentry_note);
    r.register("pwid::capability", "viable_floor", test_viable_floor);
    #[cfg(target_arch = "x86_64")]
    r.register("pwid::dmu", "cow_bp", test_pwidentry_cow_bp);
}

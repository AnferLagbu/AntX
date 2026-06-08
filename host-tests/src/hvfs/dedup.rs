use crate::kernel::fs::hvfs::bp::HvBlockPointer;
use crate::kernel::fs::hvfs::bp::HvCksumType;
use crate::kernel::fs::hvfs::checksum::HvChecksum;
use core::sync::atomic::{AtomicU64, Ordering};
use std::collections::BTreeMap;
use std::sync::Mutex;
use std::vec::Vec;

pub const CAS_HASH_SIZE: usize = 32;
pub type CasHash = [u8; CAS_HASH_SIZE];

pub struct CasIndex {
    pub hash_to_dva: Mutex<BTreeMap<[u8; 32], Vec<HvBlockPointer>>>,
    pub ref_counts: Mutex<BTreeMap<[u8; 32], u64>>,
    pub hits: AtomicU64,
    pub misses: AtomicU64,
    pub synced: AtomicU64,
}

impl Default for CasIndex {
    fn default() -> Self {
        Self::new()
    }
}

impl CasIndex {
    pub fn new() -> Self {
        Self {
            hash_to_dva: Mutex::new(BTreeMap::new()),
            ref_counts: Mutex::new(BTreeMap::new()),
            hits: AtomicU64::new(0),
            misses: AtomicU64::new(0),
            synced: AtomicU64::new(0),
        }
    }

    pub fn lookup(&self, hash: &CasHash) -> Option<HvBlockPointer> {
        let index = self.hash_to_dva.lock().unwrap();
        if let Some(dvas) = index.get(hash) {
            if let Some(bp) = dvas.first() {
                self.hits.fetch_add(1, Ordering::Relaxed);
                return Some(*bp);
            }
        }
        self.misses.fetch_add(1, Ordering::Relaxed);
        None
    }

    pub fn insert(&self, hash: CasHash, bp: HvBlockPointer) {
        let mut index = self.hash_to_dva.lock().unwrap();
        index.entry(hash).or_default().push(bp);
        let mut refs = self.ref_counts.lock().unwrap();
        *refs.entry(hash).or_insert(0) += 1;
        self.synced.fetch_add(1, Ordering::Relaxed);
    }

    pub fn ref_inc(&self, hash: &CasHash) -> u64 {
        let mut refs = self.ref_counts.lock().unwrap();
        let count = refs.entry(*hash).or_insert(0);
        *count += 1;
        self.hits.fetch_add(1, Ordering::Relaxed);
        *count
    }

    pub fn ref_dec(&self, hash: &CasHash) -> u64 {
        // 锁顺序: 先 index 后 refs, 与 insert/ref_inc 一致, 避免 AB-BA 死锁
        let mut index = self.hash_to_dva.lock().unwrap();
        let mut refs = self.ref_counts.lock().unwrap();
        if let Some(count) = refs.get_mut(hash) {
            *count = count.saturating_sub(1);
            if *count == 0 {
                refs.remove(hash);
                index.remove(hash);
                return 0;
            }
            return *count;
        }
        0
    }

    pub fn ref_count(&self, hash: &CasHash) -> u64 {
        let refs = self.ref_counts.lock().unwrap();
        refs.get(hash).copied().unwrap_or(0)
    }

    pub fn is_known(&self, hash: &CasHash) -> bool {
        self.ref_counts.lock().unwrap().contains_key(hash)
    }

    pub fn get_stats(&self) -> (u64, u64, u64) {
        (
            self.hits.load(Ordering::Relaxed),
            self.misses.load(Ordering::Relaxed),
            self.synced.load(Ordering::Relaxed),
        )
    }

    pub fn invalidate(&self, hash: &CasHash) {
        let mut index = self.hash_to_dva.lock().unwrap();
        index.remove(hash);
        let mut refs = self.ref_counts.lock().unwrap();
        refs.remove(hash);
    }
}

use std::sync::OnceLock;
static CAS_INDEX: OnceLock<CasIndex> = OnceLock::new();

pub fn get_cas() -> &'static CasIndex {
    CAS_INDEX.get_or_init(CasIndex::new)
}

pub fn cas_init() {
    get_cas();
}

pub fn sha256(data: &[u8]) -> CasHash {
    let ck = HvChecksum::compute(HvCksumType::SHA256, data);
    let mut hash = [0u8; 32];
    hash[0..8].copy_from_slice(&ck.value[0].to_le_bytes());
    hash[8..16].copy_from_slice(&ck.value[1].to_le_bytes());
    hash[16..24].copy_from_slice(&ck.value[2].to_le_bytes());
    hash[24..32].copy_from_slice(&ck.value[3].to_le_bytes());
    hash
}

use alloc::boxed::Box;
use alloc::collections::VecDeque;
use alloc::vec::Vec;
use core::sync::atomic::{AtomicBool, AtomicU64, AtomicU32, Ordering};
use crate::kernel::sync::mutex::Mutex;

pub const ZV_ARC_DEFAULT_SIZE: usize = 256;
pub const ZV_ARC_MAX_SIZE: usize = 4096;
pub const ZV_ARC_BUF_SIZE: usize = 4096;
pub const ZV_ARC_META_SIZE: usize = 16384;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum ZvArcState {
    Anon = 0,
    Mru = 1,
    Mfu = 2,
    MruGhost = 3,
    MfuGhost = 4,
    L2Cache = 5,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum ZvArcBufType {
    Data = 0,
    Metadata = 1,
}

#[derive(Debug, Clone, Copy)]
pub struct ZvArcKey {
    pub vdev_id: u16,
    pub offset: u64,
    pub birth_txg: u64,
}

impl ZvArcKey {
    pub fn new(vdev_id: u16, offset: u64, birth_txg: u64) -> Self {
        Self { vdev_id, offset, birth_txg }
    }

    pub fn hash(&self) -> u64 {
        let mut h: u64 = 14695981039346656037;
        h ^= self.vdev_id as u64;
        h = h.wrapping_mul(1099511628211);
        h ^= self.offset;
        h = h.wrapping_mul(1099511628211);
        h ^= self.birth_txg;
        h = h.wrapping_mul(1099511628211);
        h
    }
}

pub struct ZvArcBuf {
    pub key: ZvArcKey,
    pub data: Box<[u8]>,
    pub size: usize,
    pub buf_type: ZvArcBufType,
    pub state: ZvArcState,
    pub ref_count: AtomicU32,
    pub access_count: u32,
    pub dirty: bool,
    pub l2cached: bool,
    pub compressed: bool,
}

unsafe impl Send for ZvArcBuf {}
unsafe impl Sync for ZvArcBuf {}

impl ZvArcBuf {
    pub fn new(key: ZvArcKey, size: usize, buf_type: ZvArcBufType) -> Self {
        let mut data = Vec::with_capacity(size);
        data.resize(size, 0);
        Self {
            key,
            data: data.into_boxed_slice(),
            size,
            buf_type,
            state: ZvArcState::Anon,
            ref_count: AtomicU32::new(1),
            access_count: 1,
            dirty: false,
            l2cached: false,
            compressed: false,
        }
    }

    pub fn is_referenced(&self) -> bool {
        self.ref_count.load(Ordering::Acquire) > 0
    }

    pub fn add_ref(&self) {
        self.ref_count.fetch_add(1, Ordering::AcqRel);
    }

    pub fn release(&self) -> u32 {
        self.ref_count.fetch_sub(1, Ordering::AcqRel).saturating_sub(1)
    }
}

pub struct ZvArcStats {
    pub hits: AtomicU64,
    pub misses: AtomicU64,
    pub mru_hits: AtomicU64,
    pub mfu_hits: AtomicU64,
    pub ghost_hits: AtomicU64,
    pub evicts: AtomicU64,
    pub size: AtomicU64,
    pub mru_size: AtomicU64,
    pub mfu_size: AtomicU64,
    pub ghost_mru_size: AtomicU64,
    pub ghost_mfu_size: AtomicU64,
    pub data_size: AtomicU64,
    pub meta_size: AtomicU64,
}

impl ZvArcStats {
    pub fn new() -> Self {
        Self {
            hits: AtomicU64::new(0), misses: AtomicU64::new(0),
            mru_hits: AtomicU64::new(0), mfu_hits: AtomicU64::new(0),
            ghost_hits: AtomicU64::new(0), evicts: AtomicU64::new(0),
            size: AtomicU64::new(0), mru_size: AtomicU64::new(0),
            mfu_size: AtomicU64::new(0), ghost_mru_size: AtomicU64::new(0),
            ghost_mfu_size: AtomicU64::new(0), data_size: AtomicU64::new(0),
            meta_size: AtomicU64::new(0),
        }
    }
}

struct ZvArcInner {
    mru: VecDeque<usize>,
    mfu: VecDeque<usize>,
    ghost_mru: VecDeque<ZvArcKey>,
    ghost_mfu: VecDeque<ZvArcKey>,
    buffers: Vec<Option<ZvArcBuf>>,
    max_size: usize,
    p: usize,
}

pub struct ZvArc {
    inner: Mutex<ZvArcInner>,
    stats: ZvArcStats,
    initialized: AtomicBool,
}

unsafe impl Send for ZvArc {}
unsafe impl Sync for ZvArc {}

impl ZvArc {
    pub fn new() -> Self {
        Self {
            inner: Mutex::new(ZvArcInner {
                mru: VecDeque::new(),
                mfu: VecDeque::new(),
                ghost_mru: VecDeque::new(),
                ghost_mfu: VecDeque::new(),
                buffers: Vec::new(),
                max_size: ZV_ARC_DEFAULT_SIZE,
                p: 0,
            }),
            stats: ZvArcStats::new(),
            initialized: AtomicBool::new(false),
        }
    }

    pub fn init(&self, max_size: usize) {
        let max = if max_size == 0 { ZV_ARC_DEFAULT_SIZE } else { max_size.min(ZV_ARC_MAX_SIZE) };
        let mut inner = self.inner.lock();
        inner.max_size = max;
        inner.buffers.clear();
        inner.mru.clear();
        inner.mfu.clear();
        inner.ghost_mru.clear();
        inner.ghost_mfu.clear();
        inner.p = max / 2;
        self.initialized.store(true, Ordering::Release);
    }

    pub fn lookup(&self, key: &ZvArcKey) -> Option<*const u8> {
        let mut inner = self.inner.lock();
        let mut found_idx: Option<usize> = None;
        let mut found_ptr: Option<*const u8> = None;
        let mut promote_to_mfu = false;
        for (idx, slot) in inner.buffers.iter_mut().enumerate() {
            if let Some(ref mut buf) = slot {
                if buf.key.vdev_id == key.vdev_id
                    && buf.key.offset == key.offset
                    && buf.key.birth_txg == key.birth_txg
                {
                    self.stats.hits.fetch_add(1, Ordering::Relaxed);
                    if buf.state == ZvArcState::Mru {
                        self.stats.mru_hits.fetch_add(1, Ordering::Relaxed);
                    } else if buf.state == ZvArcState::Mfu {
                        self.stats.mfu_hits.fetch_add(1, Ordering::Relaxed);
                    }
                    buf.access_count += 1;
                    if buf.state == ZvArcState::Mru {
                        promote_to_mfu = true;
                    }
                    buf.add_ref();
                    found_ptr = Some(buf.data.as_ptr());
                    found_idx = Some(idx);
                    break;
                }
            }
        }
        if let Some(idx) = found_idx {
            if promote_to_mfu {
                inner.mru.retain(|&i| i != idx);
                if let Some(ref mut buf) = inner.buffers[idx] {
                    buf.state = ZvArcState::Mfu;
                }
                inner.mfu.push_back(idx);
            }
            return found_ptr;
        }
        if let Some(pos) = inner.ghost_mru.iter().position(|k| k.vdev_id == key.vdev_id && k.offset == key.offset && k.birth_txg == key.birth_txg) {
            inner.ghost_mru.remove(pos);
            let delta = if inner.mru.len() + inner.ghost_mru.len() > 0 {
                (inner.max_size * inner.ghost_mru.len()) / (inner.mru.len() + inner.ghost_mru.len())
            } else { 1 };
            inner.p = (inner.p + delta).min(inner.max_size);
            self.stats.ghost_hits.fetch_add(1, Ordering::Relaxed);
        } else if let Some(pos) = inner.ghost_mfu.iter().position(|k| k.vdev_id == key.vdev_id && k.offset == key.offset && k.birth_txg == key.birth_txg) {
            inner.ghost_mfu.remove(pos);
            let delta = if inner.mfu.len() + inner.ghost_mfu.len() > 0 {
                (inner.max_size * inner.ghost_mfu.len()) / (inner.mfu.len() + inner.ghost_mfu.len())
            } else { 1 };
            inner.p = inner.p.saturating_sub(delta);
            self.stats.ghost_hits.fetch_add(1, Ordering::Relaxed);
        }
        self.stats.misses.fetch_add(1, Ordering::Relaxed);
        None
    }

    pub fn insert(&self, key: ZvArcKey, data: &[u8], buf_type: ZvArcBufType) -> Option<*const u8> {
        let mut inner = self.inner.lock();
        self.evict_if_needed(&mut inner, data.len());
        let mut buf = ZvArcBuf::new(key, data.len(), buf_type);
        buf.data[..data.len()].copy_from_slice(data);
        buf.state = ZvArcState::Mru;
        let slot_idx = inner.buffers.iter().position(|s| s.is_none());
        let idx = match slot_idx {
            Some(i) => {
                inner.buffers[i] = Some(buf);
                i
            }
            None => {
                inner.buffers.push(Some(buf));
                inner.buffers.len() - 1
            }
        };
        inner.mru.push_back(idx);
        self.stats.size.fetch_add(data.len() as u64, Ordering::Relaxed);
        if buf_type == ZvArcBufType::Data {
            self.stats.data_size.fetch_add(data.len() as u64, Ordering::Relaxed);
        } else {
            self.stats.meta_size.fetch_add(data.len() as u64, Ordering::Relaxed);
        }
        self.stats.mru_size.fetch_add(data.len() as u64, Ordering::Relaxed);
        inner.buffers[idx].as_ref().map(|b| {
            b.add_ref();
            b.data.as_ptr()
        })
    }

    fn evict_if_needed(&self, inner: &mut ZvArcInner, incoming_size: usize) {
        let current: usize = inner.mru.len() + inner.mfu.len();
        while current + 1 > inner.max_size {
            if inner.mru.len() > inner.p {
                if let Some(idx) = inner.mru.pop_front() {
                    if let Some(buf) = inner.buffers[idx].take() {
                        inner.ghost_mru.push_back(buf.key);
                        self.stats.mru_size.fetch_sub(buf.size as u64, Ordering::Relaxed);
                        self.stats.size.fetch_sub(buf.size as u64, Ordering::Relaxed);
                        self.stats.evicts.fetch_add(1, Ordering::Relaxed);
                    }
                }
            } else {
                if let Some(idx) = inner.mfu.pop_front() {
                    if let Some(buf) = inner.buffers[idx].take() {
                        inner.ghost_mfu.push_back(buf.key);
                        self.stats.mfu_size.fetch_sub(buf.size as u64, Ordering::Relaxed);
                        self.stats.size.fetch_sub(buf.size as u64, Ordering::Relaxed);
                        self.stats.evicts.fetch_add(1, Ordering::Relaxed);
                    }
                }
            }
            if inner.ghost_mru.len() > inner.max_size {
                inner.ghost_mru.pop_front();
            }
            if inner.ghost_mfu.len() > inner.max_size {
                inner.ghost_mfu.pop_front();
            }
            break;
        }
    }

    pub fn release(&self, key: &ZvArcKey) {
        let mut inner = self.inner.lock();
        for slot in inner.buffers.iter_mut() {
            if let Some(ref mut buf) = slot {
                if buf.key.vdev_id == key.vdev_id
                    && buf.key.offset == key.offset
                    && buf.key.birth_txg == key.birth_txg
                {
                    buf.release();
                    break;
                }
            }
        }
    }

    pub fn mark_dirty(&self, key: &ZvArcKey) {
        let mut inner = self.inner.lock();
        for slot in inner.buffers.iter_mut() {
            if let Some(ref mut buf) = slot {
                if buf.key.vdev_id == key.vdev_id
                    && buf.key.offset == key.offset
                    && buf.key.birth_txg == key.birth_txg
                {
                    buf.dirty = true;
                    break;
                }
            }
        }
    }

    pub fn flush_dirty(&self) -> usize {
        let inner = self.inner.lock();
        let mut count = 0;
        for slot in inner.buffers.iter() {
            if let Some(ref buf) = slot {
                if buf.dirty {
                    count += 1;
                }
            }
        }
        count
    }

    pub fn get_stats(&self) -> (u64, u64, u64, u64) {
        (
            self.stats.hits.load(Ordering::Relaxed),
            self.stats.misses.load(Ordering::Relaxed),
            self.stats.size.load(Ordering::Relaxed),
            self.stats.evicts.load(Ordering::Relaxed),
        )
    }

    pub fn is_initialized(&self) -> bool {
        self.initialized.load(Ordering::Acquire)
    }
}

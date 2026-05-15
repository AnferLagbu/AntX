use alloc::vec::Vec;
use core::sync::atomic::{AtomicBool, AtomicU64, AtomicU32, Ordering};
use crate::kernel::sync::mutex::Mutex;
use crate::kernel::fs::zvfs::bp::ZvBlockPointer;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum ZvTxgState {
    Open = 0,
    Quiescing = 1,
    Syncing = 2,
    Committed = 3,
}

pub const ZV_TXG_SIZE: usize = 3;

#[derive(Debug, Clone)]
pub struct ZvIo {
    pub bp: ZvBlockPointer,
    pub offset: u64,
    pub size: u32,
    pub io_type: ZvIoType,
    pub priority: u8,
    pub ready: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum ZvIoType {
    Read = 0,
    Write = 1,
    Free = 2,
    Claim = 3,
}

pub struct ZvTxg {
    pub txg_id: u64,
    pub state: ZvTxgState,
    pub birth_time: u64,
    pub nwrites: AtomicU64,
    pub nalloc: AtomicU64,
    pub nfree: AtomicU64,
    pub space_delta: AtomicU64,
    pub dirty_bps: Mutex<Vec<ZvBlockPointer>>,
    pub free_bps: Mutex<Vec<ZvBlockPointer>>,
    pub io_list: Mutex<Vec<ZvIo>>,
    pub synced: AtomicBool,
}

unsafe impl Send for ZvTxg {}
unsafe impl Sync for ZvTxg {}

impl ZvTxg {
    pub fn new(txg_id: u64) -> Self {
        Self {
            txg_id,
            state: ZvTxgState::Open,
            birth_time: 0,
            nwrites: AtomicU64::new(0),
            nalloc: AtomicU64::new(0),
            nfree: AtomicU64::new(0),
            space_delta: AtomicU64::new(0),
            dirty_bps: Mutex::new(Vec::new()),
            free_bps: Mutex::new(Vec::new()),
            io_list: Mutex::new(Vec::new()),
            synced: AtomicBool::new(false),
        }
    }

    pub fn open(&mut self) {
        self.state = ZvTxgState::Open;
        self.synced.store(false, Ordering::Release);
    }

    pub fn quiesce(&mut self) {
        self.state = ZvTxgState::Quiescing;
    }

    pub fn sync_start(&mut self) {
        self.state = ZvTxgState::Syncing;
    }

    pub fn commit(&mut self) {
        self.state = ZvTxgState::Committed;
        self.synced.store(true, Ordering::Release);
    }

    pub fn is_open(&self) -> bool {
        self.state == ZvTxgState::Open
    }

    pub fn is_quiescing(&self) -> bool {
        self.state == ZvTxgState::Quiescing
    }

    pub fn is_syncing(&self) -> bool {
        self.state == ZvTxgState::Syncing
    }

    pub fn add_dirty(&self, bp: ZvBlockPointer) {
        self.dirty_bps.lock().push(bp);
        self.nwrites.fetch_add(1, Ordering::Relaxed);
    }

    pub fn add_free(&self, bp: ZvBlockPointer) {
        self.free_bps.lock().push(bp);
        self.nfree.fetch_add(1, Ordering::Relaxed);
    }

    pub fn add_io(&self, io: ZvIo) {
        self.io_list.lock().push(io);
    }

    pub fn drain_dirty(&self) -> Vec<ZvBlockPointer> {
        let mut dirty = self.dirty_bps.lock();
        let drained: Vec<ZvBlockPointer> = dirty.drain(..).collect();
        drained
    }

    pub fn drain_free(&self) -> Vec<ZvBlockPointer> {
        let mut free = self.free_bps.lock();
        let drained: Vec<ZvBlockPointer> = free.drain(..).collect();
        drained
    }

    pub fn drain_io(&self) -> Vec<ZvIo> {
        let mut io = self.io_list.lock();
        io.drain(..).collect()
    }
}

pub struct ZvTxgGroup {
    pub txgs: [Option<ZvTxg>; ZV_TXG_SIZE],
    pub current: AtomicU64,
    pub open_txg: AtomicU32,
    pub quiescing_txg: AtomicU32,
    pub syncing_txg: AtomicU32,
    pub sync_in_progress: AtomicBool,
    pub total_syncs: AtomicU64,
    pub total_dirty: AtomicU64,
}

unsafe impl Send for ZvTxgGroup {}
unsafe impl Sync for ZvTxgGroup {}

impl ZvTxgGroup {
    pub fn new() -> Self {
        Self {
            txgs: [const { None }, const { None }, const { None }],
            current: AtomicU64::new(1),
            open_txg: AtomicU32::new(0),
            quiescing_txg: AtomicU32::new(0),
            syncing_txg: AtomicU32::new(0),
            sync_in_progress: AtomicBool::new(false),
            total_syncs: AtomicU64::new(0),
            total_dirty: AtomicU64::new(0),
        }
    }

    pub fn init(&mut self, start_txg: u64) {
        self.current.store(start_txg, Ordering::Release);
        for i in 0..ZV_TXG_SIZE {
            self.txgs[i] = Some(ZvTxg::new(start_txg + i as u64));
        }
        self.open_txg.store(0, Ordering::Release);
        self.quiescing_txg.store(1, Ordering::Release);
        self.syncing_txg.store(2, Ordering::Release);
        if let Some(ref mut txg) = self.txgs[0] { txg.open(); }
        if let Some(ref mut txg) = self.txgs[1] { txg.quiesce(); }
        if let Some(ref mut txg) = self.txgs[2] { txg.sync_start(); }
    }

    pub fn get_open_txg(&self) -> Option<&ZvTxg> {
        let idx = self.open_txg.load(Ordering::Acquire) as usize;
        if idx < ZV_TXG_SIZE { self.txgs[idx].as_ref() } else { None }
    }

    pub fn get_open_txg_mut(&mut self) -> Option<&mut ZvTxg> {
        let idx = self.open_txg.load(Ordering::Acquire) as usize;
        if idx < ZV_TXG_SIZE { self.txgs[idx].as_mut() } else { None }
    }

    pub fn get_syncing_txg(&self) -> Option<&ZvTxg> {
        let idx = self.syncing_txg.load(Ordering::Acquire) as usize;
        if idx < ZV_TXG_SIZE { self.txgs[idx].as_ref() } else { None }
    }

    pub fn transition(&mut self) -> u64 {
        let old_open = self.open_txg.load(Ordering::Acquire) as usize;
        let old_quiescing = self.quiescing_txg.load(Ordering::Acquire) as usize;
        let old_syncing = self.syncing_txg.load(Ordering::Acquire) as usize;
        if let Some(ref mut txg) = self.txgs[old_open] {
            txg.quiesce();
        }
        if let Some(ref mut txg) = self.txgs[old_quiescing] {
            txg.sync_start();
        }
        if let Some(ref mut txg) = self.txgs[old_syncing] {
            txg.commit();
        }
        let new_txg_id = self.current.fetch_add(1, Ordering::AcqRel) + ZV_TXG_SIZE as u64;
        let new_open = old_syncing;
        if let Some(ref mut txg) = self.txgs[new_open] {
            *txg = ZvTxg::new(new_txg_id);
            txg.open();
        }
        self.open_txg.store(new_open as u32, Ordering::Release);
        self.quiescing_txg.store(old_open as u32, Ordering::Release);
        self.syncing_txg.store(old_quiescing as u32, Ordering::Release);
        self.total_syncs.fetch_add(1, Ordering::Relaxed);
        new_txg_id
    }

    pub fn current_txg(&self) -> u64 {
        self.current.load(Ordering::Acquire)
    }

    pub fn add_dirty_to_open(&self, bp: ZvBlockPointer) {
        if let Some(txg) = self.get_open_txg() {
            txg.add_dirty(bp);
            self.total_dirty.fetch_add(1, Ordering::Relaxed);
        }
    }

    pub fn add_free_to_open(&self, bp: ZvBlockPointer) {
        if let Some(txg) = self.get_open_txg() {
            txg.add_free(bp);
        }
    }
}

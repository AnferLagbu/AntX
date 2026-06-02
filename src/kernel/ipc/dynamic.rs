//! 动态 IPC 命名空间 (Dynamic IPC Namespace)
//!
//! 替换现有静态数组为 `Vec` 动态分配，消除编译期容量上限。
//!
//! ## 与旧版区别
//!
//! | 特性 | 静态 (IpcNamespace) | 动态 (DynIpcNamespace) |
//! |------|---------------------|------------------------|
//! | 管道上限 | `IPC_MAX_PIPES` (64) | 物理内存限制 |
//! | 消息队列上限 | `IPC_MAX_MSG_QUEUES` (32) | 物理内存限制 |
//! | 共享内存段上限 | `IPC_MAX_SHM_SEGS` (16) | 物理内存限制 |
//! | 信号量上限 | `IPC_MAX_SEMAPHORES` (64) | 物理内存限制 |
//! | 扩容方式 | 不可扩容 | 按需分配 |
//!
//! ## 迁移策略
//!
//! 旧版 `IpcNamespace` 保持不变（向后兼容）。
//! 新代码使用 `DynIpcNamespace`，通过 FFI 桥接暴露给 C。

use alloc::alloc::dealloc;
use alloc::vec::Vec;
use core::alloc::Layout;
use spin::Mutex;

use super::types::*;

pub struct DynIpcNamespace {
    pub pipes: Mutex<Vec<Pipe>>,
    pub shm_segs: Mutex<Vec<ShmSegment>>,
    pub msg_queues: Mutex<Vec<MsgQueue>>,
    pub semaphores: Mutex<Vec<Semaphore>>,
    pub next_id: Mutex<IpcId>,
}

impl DynIpcNamespace {
    pub fn new() -> Self {
        Self {
            pipes: Mutex::new(Vec::new()),
            shm_segs: Mutex::new(Vec::new()),
            msg_queues: Mutex::new(Vec::new()),
            semaphores: Mutex::new(Vec::new()),
            next_id: Mutex::new(1),
        }
    }

    fn allocate_id(&self) -> IpcId {
        let mut next = self.next_id.lock();
        let id = *next;
        *next = id.wrapping_add(1);
        id
    }

    // ─── Pipe ────────────────────────────────────────────────

    pub fn pipe_create(&self, read_pid: u32, write_pid: u32) -> IpcId {
        let id = self.allocate_id();
        let mut pipe = Pipe::new();
        pipe.id = id;
        pipe.read_pid = read_pid;
        pipe.write_pid = write_pid;
        pipe.readers = 1;
        pipe.writers = 1;
        self.pipes.lock().push(pipe);
        id
    }

    pub fn pipe_destroy(&self, id: IpcId) -> Result<(), i32> {
        let mut pipes = self.pipes.lock();
        let pos = pipes.iter().position(|p| p.id == id).ok_or(-1)?;
        pipes.remove(pos);
        Ok(())
    }

    pub fn pipe_exists(&self, id: IpcId) -> bool {
        self.pipes.lock().iter().any(|p| p.id == id)
    }

    pub fn pipe_count(&self) -> usize {
        self.pipes.lock().len()
    }

    // ─── Shared Memory ─────────────────────────────────────

    pub fn shm_create(&self, owner_pid: u32, size: u64) -> Result<IpcId, i32> {
        if size == 0 || size > SHM_MAX_SIZE {
            return Err(-1);
        }

        let pages = (size as usize).div_ceil(4096);
        let phys = crate::kernel::mm::api::pmm_alloc_pages(pages);
        if phys.is_null() {
            return Err(-3);
        }

        let id = self.allocate_id();
        let seg = ShmSegment {
            id,
            creator: owner_pid,
            phys_addr: phys as u64,
            size,
            perm: 0o666,
            ref_count: 0,
            attach_count: 0,
            flags: 0,
            attached_pids: [0u32; 16],
        };
        self.shm_segs.lock().push(seg);
        Ok(id)
    }

    pub fn shm_destroy(&self, id: IpcId) -> Result<(), i32> {
        let mut segs = self.shm_segs.lock();
        let pos = segs.iter().position(|s| s.id == id).ok_or(-1)?;
        let seg = segs.remove(pos);
        let pages = (seg.size as usize).div_ceil(4096);
        crate::kernel::mm::api::pmm_free_pages(seg.phys_addr as *mut u8, pages);
        Ok(())
    }

    pub fn shm_count(&self) -> usize {
        self.shm_segs.lock().len()
    }

    // ─── Message Queue ─────────────────────────────────────

    pub fn msgq_create(&self, owner_pid: u32, max_msgs: u32, max_size: u32) -> Result<IpcId, i32> {
        let id = self.allocate_id();
        let mq = MsgQueue {
            id,
            owner: owner_pid,
            head: core::ptr::null_mut(),
            tail: core::ptr::null_mut(),
            count: 0,
            max_msgs,
            max_size,
            send_wait: WaitQueue::new(),
            recv_wait: WaitQueue::new(),
            flags: 0,
            perm: 0o666,
        };
        self.msg_queues.lock().push(mq);
        Ok(id)
    }

    pub fn msgq_destroy(&self, id: IpcId) -> Result<(), i32> {
        let mut queues = self.msg_queues.lock();
        let pos = queues.iter().position(|q| q.id == id).ok_or(-1)?;

        let mq = queues.remove(pos);

        let mut cur = mq.head;
        while !cur.is_null() {
            let next = unsafe { (*cur).next };
            unsafe {
                let layout = Layout::new::<Message>();
                dealloc(cur as *mut u8, layout);
            }
            cur = next;
        }

        Ok(())
    }

    pub fn msgq_exists(&self, id: IpcId) -> bool {
        self.msg_queues.lock().iter().any(|q| q.id == id)
    }

    pub fn msgq_count(&self) -> usize {
        self.msg_queues.lock().len()
    }

    // ─── Semaphore ─────────────────────────────────────────

    pub fn sem_create(&self, owner_pid: u32, count: u32, max_count: u32) -> Result<IpcId, i32> {
        let id = self.allocate_id();
        let sem = Semaphore {
            id,
            owner: owner_pid,
            count: count as i32,
            max_count,
            wait: WaitQueue::new(),
            flags: 0,
            perm: 0o666,
        };
        self.semaphores.lock().push(sem);
        Ok(id)
    }

    pub fn sem_destroy(&self, id: IpcId) -> Result<(), i32> {
        let mut sems = self.semaphores.lock();
        let pos = sems.iter().position(|s| s.id == id).ok_or(-1)?;
        sems.remove(pos);
        Ok(())
    }

    pub fn sem_count(&self) -> usize {
        self.semaphores.lock().len()
    }

    pub fn total_count(&self) -> usize {
        self.pipe_count() + self.shm_count() + self.msgq_count() + self.sem_count()
    }
}

static mut DYN_IPC: Option<DynIpcNamespace> = None;

fn dyn_ipc_init_impl() {
    // SAFETY: single-threaded boot path; one-time initialization
    unsafe {
        DYN_IPC = Some(DynIpcNamespace::new());
    }
}

pub fn get_dyn_ipc() -> &'static DynIpcNamespace {
    // SAFETY: DYN_IPC is set by dyn_ipc_init before any concurrent access
    unsafe { DYN_IPC.as_ref().expect("DynIpcNamespace not initialized") }
}

#[no_mangle]
pub fn dyn_ipc_init() {
    dyn_ipc_init_impl();
}

#[no_mangle]
pub fn dyn_ipc_pipe_create(read_pid: u32, write_pid: u32) -> u32 {
    get_dyn_ipc().pipe_create(read_pid, write_pid)
}

#[no_mangle]
pub fn dyn_ipc_pipe_destroy(id: u32) -> i32 {
    match get_dyn_ipc().pipe_destroy(id) {
        Ok(()) => 0,
        Err(e) => e,
    }
}

#[no_mangle]
pub fn dyn_ipc_msgq_create(owner_pid: u32, max_msgs: u32, max_size: u32) -> u32 {
    match get_dyn_ipc().msgq_create(owner_pid, max_msgs, max_size) {
        Ok(id) => id,
        Err(_) => 0,
    }
}

#[no_mangle]
pub fn dyn_ipc_shm_create(owner_pid: u32, size: u64) -> u32 {
    match get_dyn_ipc().shm_create(owner_pid, size) {
        Ok(id) => id,
        Err(_) => 0,
    }
}

#[no_mangle]
pub fn dyn_ipc_sem_create(owner_pid: u32, count: u32, max_count: u32) -> u32 {
    match get_dyn_ipc().sem_create(owner_pid, count, max_count) {
        Ok(id) => id,
        Err(_) => 0,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_dyn_pipe_no_limit() {
        let ns = DynIpcNamespace::new();

        let mut ids = Vec::new();
        for _ in 0..200 {
            let id = ns.pipe_create(1000, 2000);
            assert_ne!(id, 0);
            ids.push(id);
        }
        assert_eq!(ids.len(), 200);
        assert_eq!(ns.pipe_count(), 200);

        for id in ids {
            ns.pipe_destroy(id).unwrap();
        }
        assert_eq!(ns.pipe_count(), 0);
    }

    #[test]
    fn test_dyn_msgq_growth() {
        let ns = DynIpcNamespace::new();

        for i in 0..100 {
            let id = ns.msgq_create(1000, 64, 4096).unwrap();
            assert!(ns.msgq_exists(id));
            assert_eq!(ns.msgq_count(), 1);
            ns.msgq_destroy(id).unwrap();
            assert_eq!(ns.msgq_count(), 0);
        }
    }

    #[test]
    fn test_dyn_shm_alloc_and_free() {
        let ns = DynIpcNamespace::new();

        let id = ns.shm_create(2000, 8192).unwrap();
        assert_ne!(id, 0);
        assert_eq!(ns.shm_count(), 1);

        ns.shm_destroy(id).unwrap();
        assert_eq!(ns.shm_count(), 0);
    }
}

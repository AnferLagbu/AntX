use spin::Mutex;
use core::sync::atomic::{AtomicU32, Ordering};

extern "C" {
    fn serial_putc(port: u16, c: u8);
}

fn log(s: &str) {
    unsafe {
        for c in s.bytes() {
            serial_putc(0x3F8, c);
        }
    }
}

pub const PROCFS_MAX_ENTRIES: usize = 32;
pub const PROCFS_MAX_NAME: usize = 32;

#[derive(Debug, Clone, Copy)]
pub struct ProcfsEntry {
    pub name: [u8; PROCFS_MAX_NAME],
    pub pid: u32,
    pub entry_type: u8,
    pub used: bool,
}

impl ProcfsEntry {
    pub const fn new() -> Self {
        Self {
            name: [0; PROCFS_MAX_NAME],
            pid: 0,
            entry_type: 0,
            used: false,
        }
    }
}

pub struct ProcfsData {
    entries: Mutex<[ProcfsEntry; PROCFS_MAX_ENTRIES]>,
    entry_count: AtomicU32,
}

unsafe impl Send for ProcfsData {}
unsafe impl Sync for ProcfsData {}

impl ProcfsData {
    pub const fn new() -> Self {
        Self {
            entries: Mutex::new([const { ProcfsEntry::new() }; PROCFS_MAX_ENTRIES]),
            entry_count: AtomicU32::new(0),
        }
    }
    
    fn set_name(entry: &mut ProcfsEntry, name: &str) {
        let bytes = name.as_bytes();
        let len = bytes.len().min(PROCFS_MAX_NAME - 1);
        entry.name[..len].copy_from_slice(&bytes[..len]);
        entry.name[len] = 0;
    }
    
    pub fn mount(&self, path: &str) -> i32 {
        let mut entries = self.entries.lock();
        
        Self::set_name(&mut entries[0], "self");
        entries[0].pid = 0;
        entries[0].entry_type = 2;
        entries[0].used = true;
        
        Self::set_name(&mut entries[1], "cpuinfo");
        entries[1].pid = 0;
        entries[1].entry_type = 1;
        entries[1].used = true;
        
        Self::set_name(&mut entries[2], "meminfo");
        entries[2].pid = 0;
        entries[2].entry_type = 1;
        entries[2].used = true;
        
        self.entry_count.store(3, Ordering::SeqCst);
        
        0
    }
    
    pub fn add_process(&self, pid: u32, name: &str) -> i32 {
        let mut entries = self.entries.lock();
        
        for entry in entries.iter_mut() {
            if !entry.used {
                Self::set_name(entry, name);
                entry.pid = pid;
                entry.entry_type = 3;
                entry.used = true;
                
                self.entry_count.fetch_add(1, Ordering::SeqCst);
                return 0;
            }
        }
        
        -1
    }
    
    pub fn remove_process(&self, pid: u32) -> i32 {
        let mut entries = self.entries.lock();
        
        for entry in entries.iter_mut() {
            if entry.used && entry.pid == pid {
                entry.used = false;
                entry.pid = 0;
                self.entry_count.fetch_sub(1, Ordering::SeqCst);
                return 0;
            }
        }
        
        -1
    }
    
    pub fn read(&self, name: &str, buf: &mut [u8]) -> i32 {
        if name == "cpuinfo" {
            let info = b"CPU: x86_64\nVendor: AntX\n";
            let len = info.len().min(buf.len());
            buf[..len].copy_from_slice(&info[..len]);
            return len as i32;
        }
        
        if name == "meminfo" {
            let info = b"Memory: 128 MB\nFree: 64 MB\n";
            let len = info.len().min(buf.len());
            buf[..len].copy_from_slice(&info[..len]);
            return len as i32;
        }
        
        if name == "self" {
            let info = b"PID: 0\n";
            let len = info.len().min(buf.len());
            buf[..len].copy_from_slice(&info[..len]);
            return len as i32;
        }
        
        -1
    }
    
    pub fn readdir(&self, index: usize) -> Option<([u8; 32], u32, u8)> {
        let entries = self.entries.lock();
        let mut count = 0;
        
        for entry in entries.iter() {
            if entry.used {
                if count == index {
                    let mut name = [0u8; 32];
                    let end = entry.name.iter().position(|&b| b == 0).unwrap_or(32);
                    name[..end].copy_from_slice(&entry.name[..end]);
                    return Some((name, entry.pid, entry.entry_type));
                }
                count += 1;
            }
        }
        None
    }
    
    pub fn entry_count(&self) -> u32 {
        self.entry_count.load(Ordering::SeqCst)
    }
}

pub static PROCFS_DATA: ProcfsData = ProcfsData::new();

pub fn init() {
}

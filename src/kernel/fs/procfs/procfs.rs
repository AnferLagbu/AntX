use core::sync::atomic::{AtomicU32, Ordering};
use spin::Mutex;

extern "C" {
    fn pmm_get_total_pages() -> u64;
    fn pmm_get_free_pages() -> u64;
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

// SAFETY: ProcfsData uses Mutex for entries and AtomicU32 for entry_count.
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

    pub fn mount(&self, _path: &str) -> i32 {
        let mut entries = self.entries.lock();

        Self::set_name(&mut entries[0], "current");
        entries[0].pid = 0;
        entries[0].entry_type = 2;
        entries[0].used = true;

        Self::set_name(&mut entries[1], "sys/cpu");
        entries[1].pid = 0;
        entries[1].entry_type = 1;
        entries[1].used = true;

        Self::set_name(&mut entries[2], "sys/memory");
        entries[2].pid = 0;
        entries[2].entry_type = 1;
        entries[2].used = true;

        Self::set_name(&mut entries[3], "sys/config");
        entries[3].pid = 0;
        entries[3].entry_type = 1;
        entries[3].used = true;

        self.entry_count.store(4, Ordering::SeqCst);

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
        if name == "sys/cpu" {
            let mut pos = 0usize;
            let write_str = |buf: &mut [u8], pos: &mut usize, s: &str| {
                let b = s.as_bytes();
                let end = (*pos + b.len()).min(buf.len());
                let len = end - *pos;
                buf[*pos..end].copy_from_slice(&b[..len]);
                *pos += len;
            };

            let cpu_info = crate::kernel::cpu::get_cpu_info();
            match cpu_info {
                Some(info) => {
                    write_str(buf, &mut pos, "CPU: ");
                    write_str(buf, &mut pos, info.brand_name());
                    write_str(buf, &mut pos, "\nVendor: ");
                    write_str(buf, &mut pos, info.vendor.name());
                    write_str(buf, &mut pos, "\nCores: ");
                    let cores = info.topology.physical_cores;
                    if cores >= 10 {
                        buf[pos] = (cores / 10) + b'0';
                        pos += 1;
                    }
                    buf[pos] = (cores % 10) + b'0';
                    pos += 1;
                    write_str(buf, &mut pos, "\n");
                }
                None => {
                    let info = b"CPU: x86_64 (Unknown)\nVendor: N/A\nCores: 1\n";
                    let len = info.len().min(buf.len());
                    buf[..len].copy_from_slice(&info[..len]);
                    return len as i32;
                }
            }
            return pos as i32;
        }

        if name == "sys/memory" {
            let total = unsafe { pmm_get_total_pages() };
            let free = unsafe { pmm_get_free_pages() };
            let total_mb = total * 4 / 1024;
            let free_mb = free * 4 / 1024;

            let mut pos = 0usize;
            let write_str = |buf: &mut [u8], pos: &mut usize, s: &str| {
                let b = s.as_bytes();
                let end = (*pos + b.len()).min(buf.len());
                let len = end - *pos;
                buf[*pos..end].copy_from_slice(&b[..len]);
                *pos += len;
            };
            let write_u64 = |buf: &mut [u8], pos: &mut usize, val: u64| {
                if val == 0 && *pos < buf.len() {
                    buf[*pos] = b'0';
                    *pos += 1;
                    return;
                }
                let mut tmp = [0u8; 20];
                let mut i = 20;
                let mut v = val;
                while v > 0 && i > 0 {
                    i -= 1;
                    tmp[i] = (v % 10) as u8 + b'0';
                    v /= 10;
                }
                let end = (*pos + (20 - i)).min(buf.len());
                let len = end - *pos;
                buf[*pos..end].copy_from_slice(&tmp[i..i + len]);
                *pos += len;
            };

            write_str(buf, &mut pos, "Total: ");
            write_u64(buf, &mut pos, total);
            write_str(buf, &mut pos, " pages (");
            write_u64(buf, &mut pos, total_mb);
            write_str(buf, &mut pos, " MB)\nFree:  ");
            write_u64(buf, &mut pos, free);
            write_str(buf, &mut pos, " pages (");
            write_u64(buf, &mut pos, free_mb);
            write_str(buf, &mut pos, " MB)\n");

            return pos as i32;
        }

        if name == "current" {
            let info = b"PID: 0\nName: kernel\n";
            let len = info.len().min(buf.len());
            buf[..len].copy_from_slice(&info[..len]);
            return len as i32;
        }

        if name == "sys/config" {
            return crate::kernel::config::procfs::read_sys_config(buf) as i32;
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

pub fn init() {}

use spin::Mutex;
use core::sync::atomic::{AtomicU32, Ordering};

extern "C" {
    fn klog_ffi_info(msg: *const u8);
}

pub const DEVFS_MAX_DEVICES: usize = 16;
pub const DEVFS_MAX_NAME: usize = 32;

fn write_u32_dec(buf: &mut [u8], mut off: usize, mut val: u32) -> usize {
    if val == 0 {
        if off < buf.len() { buf[off] = b'0'; off += 1; }
        return off;
    }
    let mut digits = [0u8; 10];
    let mut i = 0;
    while val > 0 {
        digits[i] = b'0' + (val % 10) as u8;
        val /= 10;
        i += 1;
    }
    for j in (0..i).rev() {
        if off < buf.len() { buf[off] = digits[j]; off += 1; }
    }
    off
}

const DEV_TYPE_NULL: u8 = 0;
const DEV_TYPE_ZERO: u8 = 1;
const DEV_TYPE_CONSOLE: u8 = 2;
const DEV_TYPE_TTY: u8 = 3;
const DEV_TYPE_CREDO: u8 = 4;

#[derive(Debug, Clone, Copy)]
pub struct DevfsDevice {
    pub name: [u8; DEVFS_MAX_NAME],
    pub dev_type: u8,
    pub used: bool,
}

impl DevfsDevice {
    pub const fn new() -> Self {
        Self {
            name: [0; DEVFS_MAX_NAME],
            dev_type: 0,
            used: false,
        }
    }
}

pub struct DevfsData {
    devices: Mutex<[DevfsDevice; DEVFS_MAX_DEVICES]>,
    device_count: AtomicU32,
}

// SAFETY: DevfsData uses Mutex for devices and AtomicU32 for device_count.
unsafe impl Send for DevfsData {}
unsafe impl Sync for DevfsData {}

impl DevfsData {
    pub const fn new() -> Self {
        Self {
            devices: Mutex::new([const { DevfsDevice::new() }; DEVFS_MAX_DEVICES]),
            device_count: AtomicU32::new(0),
        }
    }
    
    fn set_name(device: &mut DevfsDevice, name: &str) {
        let bytes = name.as_bytes();
        let len = bytes.len().min(DEVFS_MAX_NAME - 1);
        device.name[..len].copy_from_slice(&bytes[..len]);
        device.name[len] = 0;
    }
    
    pub fn register_device(&self, name: &str, dev_type: u8) -> i32 {
        let mut devices = self.devices.lock();
        for device in devices.iter() {
            if device.used {
                let end = device.name.iter().position(|&b| b == 0).unwrap_or(DEVFS_MAX_NAME);
                let existing = core::str::from_utf8(&device.name[..end]).unwrap_or("");
                if existing == name {
                    return -1;
                }
            }
        }
        for device in devices.iter_mut() {
            if !device.used {
                Self::set_name(device, name);
                device.dev_type = dev_type;
                device.used = true;
                self.device_count.fetch_add(1, Ordering::SeqCst);
                return 0;
            }
        }
        -1
    }

    pub fn unregister_device(&self, name: &str) -> i32 {
        let mut devices = self.devices.lock();
        for device in devices.iter_mut() {
            if device.used {
                let end = device.name.iter().position(|&b| b == 0).unwrap_or(DEVFS_MAX_NAME);
                let existing = core::str::from_utf8(&device.name[..end]).unwrap_or("");
                if existing == name {
                    device.used = false;
                    device.dev_type = 0;
                    self.device_count.fetch_sub(1, Ordering::SeqCst);
                    return 0;
                }
            }
        }
        -1
    }

    pub fn mount(&self, _path: &str) -> i32 {
        let mut devices = self.devices.lock();
        
        Self::set_name(&mut devices[0], "null");
        devices[0].dev_type = DEV_TYPE_NULL;
        devices[0].used = true;
        
        Self::set_name(&mut devices[1], "zero");
        devices[1].dev_type = DEV_TYPE_ZERO;
        devices[1].used = true;
        
        Self::set_name(&mut devices[2], "console");
        devices[2].dev_type = DEV_TYPE_CONSOLE;
        devices[2].used = true;
        
        Self::set_name(&mut devices[3], "tty");
        devices[3].dev_type = DEV_TYPE_TTY;
        devices[3].used = true;
        
        Self::set_name(&mut devices[4], "credo");
        devices[4].dev_type = DEV_TYPE_CREDO;
        devices[4].used = true;
        
        self.device_count.store(5, Ordering::SeqCst);
        
        0
    }
    
    pub fn open(&self, path: &str) -> Option<(u32, u8)> {
        let dev_name = path.trim_start_matches('/');
        
        let devices = self.devices.lock();
        for device in devices.iter() {
            if device.used {
                let end = device.name.iter().position(|&b| b == 0).unwrap_or(DEVFS_MAX_NAME);
                let name = core::str::from_utf8(&device.name[..end]).unwrap_or("");
                if name == dev_name {
                    return Some((device.dev_type as u32, device.dev_type));
                }
            }
        }
        None
    }
    
    pub fn read(&self, dev_type: u8, buf: &mut [u8]) -> i32 {
        match dev_type {
            DEV_TYPE_NULL => 0,
            DEV_TYPE_ZERO => {
                buf.fill(0);
                buf.len() as i32
            }
            DEV_TYPE_CONSOLE | DEV_TYPE_TTY => 0,
            DEV_TYPE_CREDO => {
                let pwm = crate::kernel::credo::session::get_current_pwm();
                let euid = crate::kernel::credo::session::get_euid();
                let uid = crate::kernel::credo::session::get_current_uid();
                if pwm != 0 {
                    let mut off = 0;
                    let blen = buf.len();
                    if off < blen { buf[off] = b'O'; off += 1; }
                    if off < blen { buf[off] = b'K'; off += 1; }
                    if off < blen { buf[off] = b' '; off += 1; }
                    for &b in b"pwm=0x" {
                        if off < blen { buf[off] = b; off += 1; }
                    }
                    let hex = b"0123456789ABCDEF";
                    for shift in (0..64).rev().step_by(4) {
                        let nibble = ((pwm >> shift) & 0xF) as usize;
                        if off < blen { buf[off] = hex[nibble]; off += 1; }
                    }
                    for &b in b" uid=" {
                        if off < blen { buf[off] = b; off += 1; }
                    }
                    off = write_u32_dec(buf, off, uid);
                    for &b in b" euid=" {
                        if off < blen { buf[off] = b; off += 1; }
                    }
                    off = write_u32_dec(buf, off, euid);
                    if off < blen { buf[off] = b'\n'; off += 1; }
                    off as i32
                } else {
                    let msg = b"ERR not_authenticated\n";
                    let len = msg.len().min(buf.len());
                    buf[..len].copy_from_slice(&msg[..len]);
                    len as i32
                }
            }
            _ => -1,
        }
    }
    
    pub fn write(&self, dev_type: u8, buf: &[u8]) -> i32 {
        match dev_type {
            DEV_TYPE_NULL | DEV_TYPE_ZERO => buf.len() as i32,
            DEV_TYPE_CONSOLE | DEV_TYPE_TTY => {
                unsafe {
                    klog_ffi_info(buf.as_ptr());
                }
                buf.len() as i32
            }
            DEV_TYPE_CREDO => {
                let input = core::str::from_utf8(buf).unwrap_or("");
                let input = input.trim_end_matches(|c| c == '\n' || c == '\r' || c == '\0');
                let mut parts = input.splitn(2, '\n');
                let note = parts.next().unwrap_or("").trim();
                let password = parts.next().unwrap_or("").trim();
                if note.is_empty() || password.is_empty() {
                    return -1;
                }
                match crate::kernel::credo::session::login(note, password) {
                    Ok(_pwm) => buf.len() as i32,
                    Err(_) => -1,
                }
            }
            _ => -1,
        }
    }
    
    pub fn readdir(&self, index: usize) -> Option<([u8; 32], u8)> {
        let devices = self.devices.lock();
        let mut count = 0;
        
        for device in devices.iter() {
            if device.used {
                if count == index {
                    let mut name = [0u8; 32];
                    let end = device.name.iter().position(|&b| b == 0).unwrap_or(32);
                    name[..end].copy_from_slice(&device.name[..end]);
                    return Some((name, device.dev_type));
                }
                count += 1;
            }
        }
        None
    }
    
    pub fn device_count(&self) -> u32 {
        self.device_count.load(Ordering::SeqCst)
    }
}

pub static DEVFS_DATA: DevfsData = DevfsData::new();

pub fn init() {
}

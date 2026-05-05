use spin::Mutex;
use core::sync::atomic::{AtomicU32, AtomicU64, Ordering};

extern "C" {
    fn klog_ffi_info(msg: *const u8);
}

fn log(s: &str) {
    unsafe {
        klog_ffi_info(s.as_ptr());
    }
}

pub const DEVFS_MAX_DEVICES: usize = 16;
pub const DEVFS_MAX_NAME: usize = 32;

const DEV_TYPE_NULL: u8 = 0;
const DEV_TYPE_ZERO: u8 = 1;
const DEV_TYPE_CONSOLE: u8 = 2;
const DEV_TYPE_TTY: u8 = 3;

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
    
    pub fn mount(&self, path: &str) -> i32 {
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
        
        self.device_count.store(4, Ordering::SeqCst);
        
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

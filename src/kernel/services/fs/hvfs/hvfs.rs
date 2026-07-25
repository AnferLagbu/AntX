#![deny(unsafe_code)]
//! HvFS (Hypervisor File System) — 模块入口
//!
//! 热插拔监听 + 公共类型重导出.

extern crate alloc;

use alloc::boxed::Box;
use crate::kernel::framework::driver::hotplug::{HotplugListener, HotplugEvent};

// 公共类型重导出 (必须在 HotplugListener 之前, 因为热插拔代码使用 get_hvfs)
pub use super::hvfs_data::*;
pub use super::hvfs_inode::*;

/// HvFS 热插拔监听器 — 将块设备热插拔事件转发到 HvFS
struct HvfsHotplugListener;

impl HotplugListener for HvfsHotplugListener {
    fn on_device_added(&self, event: &HotplugEvent) -> bool {
        if let HotplugEvent::DeviceAdded { location } = event {
            // 使用 slot 作为 drive_id (PCI 热插拔槽位号)
            let drive_id = location.slot;
            crate::slog_info!(FS, "[HvFS] HOTPLUG: device added (slot={}, bus={}/{}",
                drive_id, location.bus, location.device);
            get_hvfs().hotplug_add_disk(drive_id)
        } else {
            false
        }
    }

    fn on_device_removed(&self, event: &HotplugEvent) {
        if let HotplugEvent::DeviceRemoved { location } = event {
            let drive_id = location.slot;
            crate::slog_info!(FS, "[HvFS] HOTPLUG: device removed (slot={})", drive_id);
            get_hvfs().hotplug_remove_disk(drive_id);
        }
    }
}

/// 注册 HvFS 热插拔监听器到全局热插拔管理器
pub fn hvfs_hotplug_register() {
    use crate::kernel::framework::driver::hotplug::HOTPLUG_MANAGER;
    HOTPLUG_MANAGER.register_listener(Box::new(HvfsHotplugListener));
}

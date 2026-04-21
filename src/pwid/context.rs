use super::types::*;

#[derive(Debug, Clone, Copy)]
pub struct TimeContext {
    pub time_of_day: TimeOfDay,
    pub day_of_week: u8,
    pub is_holiday: bool,
}

impl Default for TimeContext {
    fn default() -> Self {
        Self {
            time_of_day: TimeOfDay::Any,
            day_of_week: 0,
            is_holiday: false,
        }
    }
}

impl TimeContext {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn from_tsc(tsc: u64) -> Self {
        let seconds = (tsc / 3_000_000_000) % 86400;
        let hour = (seconds / 3600) as u8;
        
        let time_of_day = match hour {
            9..=17 => TimeOfDay::WorkHours,
            _ => TimeOfDay::OffHours,
        };

        let day_of_week = ((tsc / (86400 * 3_000_000_000)) % 7) as u8;

        Self {
            time_of_day,
            day_of_week,
            is_holiday: false,
        }
    }

    pub fn matches_mask(&self, mask: u8) -> bool {
        mask == 0 || (mask & (1 << self.time_of_day as u8)) != 0
    }
}

#[derive(Debug, Clone)]
pub struct LocationContext {
    pub current_path: [u8; 256],
    pub mount_point: [u8; 128],
    pub depth_from_root: u8,
}

impl Default for LocationContext {
    fn default() -> Self {
        Self {
            current_path: [0; 256],
            mount_point: [0; 128],
            depth_from_root: 0,
        }
    }
}

impl LocationContext {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn set_path(&mut self, path: &str) {
        let bytes = path.as_bytes();
        let len = bytes.len().min(255);
        self.current_path[..len].copy_from_slice(&bytes[..len]);
        self.current_path[len] = 0;
        self.depth_from_root = path.matches('/').count() as u8;
    }

    pub fn get_path(&self) -> &str {
        let end = self.current_path.iter().position(|&b| b == 0).unwrap_or(256);
        core::str::from_utf8(&self.current_path[..end]).unwrap_or("/")
    }

    pub fn starts_with(&self, prefix: &str) -> bool {
        self.get_path().starts_with(prefix)
    }

    pub fn set_mount_point(&mut self, mount: &str) {
        let bytes = mount.as_bytes();
        let len = bytes.len().min(127);
        self.mount_point[..len].copy_from_slice(&bytes[..len]);
        self.mount_point[len] = 0;
    }

    pub fn get_mount_point(&self) -> &str {
        let end = self.mount_point.iter().position(|&b| b == 0).unwrap_or(128);
        core::str::from_utf8(&self.mount_point[..end]).unwrap_or("")
    }
}

#[derive(Debug, Clone, Copy)]
pub struct SessionContext {
    pub session_type: SessionType,
    pub login_method: LoginMethod,
    pub consecutive_failures: u8,
    pub risk_score: u8,
}

impl Default for SessionContext {
    fn default() -> Self {
        Self {
            session_type: SessionType::Local,
            login_method: LoginMethod::Password,
            consecutive_failures: 0,
            risk_score: 0,
        }
    }
}

impl SessionContext {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn is_high_risk(&self) -> bool {
        self.risk_score > 70
    }

    pub fn is_suspicious(&self) -> bool {
        self.consecutive_failures >= 3 || self.risk_score > 50
    }
}

#[derive(Debug, Clone)]
pub struct DeviceContext {
    pub device_id: [u8; 32],
    pub device_type: u8,
    pub is_trusted_device: bool,
}

impl Default for DeviceContext {
    fn default() -> Self {
        Self {
            device_id: [0; 32],
            device_type: 0,
            is_trusted_device: false,
        }
    }
}

impl DeviceContext {
    pub fn new() -> Self {
        Self::default()
    }
}

#[derive(Debug, Clone)]
pub struct PermissionContext {
    pub time_context: TimeContext,
    pub location_context: LocationContext,
    pub session_context: SessionContext,
    pub device_context: DeviceContext,
}

impl Default for PermissionContext {
    fn default() -> Self {
        Self {
            time_context: TimeContext::new(),
            location_context: LocationContext::new(),
            session_context: SessionContext::new(),
            device_context: DeviceContext::new(),
        }
    }
}

impl PermissionContext {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn from_current() -> Self {
        let tsc: u64;
        unsafe { core::arch::asm!("rdtsc", out("rax") tsc, out("rdx") _, options(nomem, nostack)); }
        
        Self {
            time_context: TimeContext::from_tsc(tsc),
            location_context: LocationContext::new(),
            session_context: SessionContext::new(),
            device_context: DeviceContext::new(),
        }
    }

    pub fn with_location(mut self, path: &str) -> Self {
        self.location_context.set_path(path);
        self
    }

    pub fn with_session(mut self, session_type: SessionType, method: LoginMethod) -> Self {
        self.session_context.session_type = session_type;
        self.session_context.login_method = method;
        self
    }

    pub fn with_risk_score(mut self, score: u8) -> Self {
        self.session_context.risk_score = score;
        self
    }

    pub fn get_combined_risk(&self) -> u8 {
        let mut risk = self.session_context.risk_score;
        
        if self.session_context.consecutive_failures >= 3 {
            risk = risk.saturating_add(20);
        }
        
        if !self.device_context.is_trusted_device {
            risk = risk.saturating_add(10);
        }

        if self.session_context.login_method == LoginMethod::Elevated {
            risk = risk.saturating_sub(10);
        }

        risk.min(100)
    }
}

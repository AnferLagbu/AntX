pub const VFS_MAX_PATH: usize = 128;
pub const VFS_MAX_NAME: usize = 64;
pub const VFS_MAX_FDS: usize = 32;
pub const VFS_MAX_MOUNTS: usize = 8;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum KernelError {
    NotFound,
    AlreadyExists,
    NoSpace,
    PermissionDenied,
    InvalidArgument,
    NotInitialized,
    IoError,
    OutOfMemory,
    Busy,
    NotSupported,
    NotADirectory,
    IsDirectory,
    ReadOnly,
    Overflow,
}

impl KernelError {
    pub fn as_i32(self) -> i32 {
        match self {
            Self::NotFound => -2,
            Self::AlreadyExists => -17,
            Self::NoSpace => -28,
            Self::PermissionDenied => -13,
            Self::InvalidArgument => -22,
            Self::NotInitialized => -5,
            Self::IoError => -5,
            Self::OutOfMemory => -12,
            Self::Busy => -16,
            Self::NotSupported => -95,
            Self::NotADirectory => -20,
            Self::IsDirectory => -21,
            Self::ReadOnly => -30,
            Self::Overflow => -75,
        }
    }
}

pub type KernelResult<T> = Result<T, KernelError>;

pub trait IntoI32 {
    fn as_i32(self) -> i32;
}

impl IntoI32 for Result<(), KernelError> {
    fn as_i32(self) -> i32 {
        match self {
            Ok(()) => 0,
            Err(e) => e.as_i32(),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum VfsFileType {
    File,
    Dir,
    Dev,
    Symlink,
}

impl VfsFileType {
    pub fn from_u8(value: u8) -> Self {
        match value {
            0 => VfsFileType::File,
            1 => VfsFileType::Dir,
            2 => VfsFileType::Dev,
            3 => VfsFileType::Symlink,
            _ => VfsFileType::File,
        }
    }

    pub fn as_u8(self) -> u8 {
        match self {
            VfsFileType::File => 0,
            VfsFileType::Dir => 1,
            VfsFileType::Dev => 2,
            VfsFileType::Symlink => 3,
        }
    }
}

pub const VFS_PERM_R: u16 = 0x04;
pub const VFS_PERM_W: u16 = 0x02;
pub const VFS_PERM_X: u16 = 0x01;

bitflags::bitflags! {
    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    pub struct VfsOpenFlags: u32 {
        const RDONLY = 0x0001;
        const WRONLY = 0x0002;
        const RDWR   = 0x0004;
        const CREAT  = 0x0100;
        const TRUNC  = 0x0200;
        const APPEND = 0x0400;
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum VfsSeekWhence {
    Set,
    Cur,
    End,
}

impl VfsSeekWhence {
    pub fn from_u32(value: u32) -> Self {
        match value {
            0 => VfsSeekWhence::Set,
            1 => VfsSeekWhence::Cur,
            2 => VfsSeekWhence::End,
            _ => VfsSeekWhence::Set,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FsType {
    RamFs,
    HvFs,
    Unknown,
}

impl FsType {
    pub fn from_name(name: &str) -> Self {
        match name {
            "ramfs" => FsType::RamFs,
            "hvfs" => FsType::HvFs,
            _ => FsType::Unknown,
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            FsType::RamFs => "ramfs",
            FsType::HvFs => "hvfs",
            FsType::Unknown => "unknown",
        }
    }
}

#[derive(Debug, Clone, Copy)]
#[repr(C)]
pub struct VfsStat {
    pub node_id: u32,
    pub mode: u16,
    pub size: u32,
    pub atime: u64,
    pub mtime: u64,
    pub ctime: u64,
    pub owner_pwm: u64,
    pub perm: u16,
    pub file_type: u8,
    pub reserved: u8,
}

impl Default for VfsStat {
    fn default() -> Self {
        Self {
            node_id: 0,
            mode: 0,
            size: 0,
            atime: 0,
            mtime: 0,
            ctime: 0,
            owner_pwm: 0,
            perm: 0,
            file_type: 0,
            reserved: 0,
        }
    }
}

#[derive(Debug, Clone)]
#[repr(C)]
pub struct VfsDirEntry {
    pub node: u32,
    pub file_type: u8,
    pub name: [u8; VFS_MAX_NAME],
}

impl VfsDirEntry {
    pub fn new() -> Self {
        Self {
            node: 0,
            file_type: 0,
            name: [0; VFS_MAX_NAME],
        }
    }

    pub fn set_name(&mut self, name: &str) {
        let bytes = name.as_bytes();
        let len = bytes.len().min(VFS_MAX_NAME - 1);
        self.name[..len].copy_from_slice(&bytes[..len]);
        self.name[len] = 0;
    }

    pub fn get_name(&self) -> &str {
        let end = self.name.iter().position(|&b| b == 0).unwrap_or(VFS_MAX_NAME);
        core::str::from_utf8(&self.name[..end]).unwrap_or("")
    }
}

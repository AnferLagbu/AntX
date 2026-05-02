pub const VFS_MAX_PATH: usize = 128;
pub const VFS_MAX_NAME: usize = 64;
pub const VFS_MAX_FDS: usize = 32;
pub const VFS_MAX_MOUNTS: usize = 8;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum VfsFileType {
    File = 0,
    Dir = 1,
    Dev = 2,
    Symlink = 3,
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
#[repr(i32)]
pub enum VfsSeekWhence {
    Set = 0,
    Cur = 1,
    End = 2,
}

impl VfsSeekWhence {
    pub fn from_i32(value: i32) -> Self {
        match value {
            0 => VfsSeekWhence::Set,
            1 => VfsSeekWhence::Cur,
            2 => VfsSeekWhence::End,
            _ => VfsSeekWhence::Set,
        }
    }
}

#[derive(Debug, Clone, Copy)]
#[repr(C)]
pub struct VfsStat {
    pub inode_num: u32,
    pub mode: u16,
    pub size: u32,
    pub atime: u64,
    pub mtime: u64,
    pub ctime: u64,
    pub owner_pwid: u64,
    pub perm: u16,
    pub file_type: u8,
    pub reserved: u8,
}

impl Default for VfsStat {
    fn default() -> Self {
        Self {
            inode_num: 0,
            mode: 0,
            size: 0,
            atime: 0,
            mtime: 0,
            ctime: 0,
            owner_pwid: 0,
            perm: 0,
            file_type: 0,
            reserved: 0,
        }
    }
}

#[derive(Debug, Clone)]
#[repr(C)]
pub struct VfsDirent {
    pub inode: u32,
    pub file_type: u8,
    pub name: [u8; VFS_MAX_NAME],
}

impl VfsDirent {
    pub fn new() -> Self {
        Self {
            inode: 0,
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

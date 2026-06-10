//! HvFS — framework 层 re-export 模块
//!
//! E6-6 阶段 2: HvFS 业务逻辑已迁移到 services::fs::hvfs.
//! 本模块仅保留:
//! - `arc_safe`: ARC 缓存裸指针→切片的 safe 封装 (框架层必要 unsafe)
//! - re-export: 透传 services 层公开类型, 保持外部引用兼容

pub mod arc_safe;

// Re-export services 层 HvFS 公共类型
pub use crate::kernel::services::fs::hvfs::arc;
pub use crate::kernel::services::fs::hvfs::bp;
pub use crate::kernel::services::fs::hvfs::checksum;
pub use crate::kernel::services::fs::hvfs::compress;
pub use crate::kernel::services::fs::hvfs::dataset;
pub use crate::kernel::services::fs::hvfs::dedup;
pub use crate::kernel::services::fs::hvfs::dmu;
pub use crate::kernel::services::fs::hvfs::dva;
pub use crate::kernel::services::fs::hvfs::hvfs;
pub use crate::kernel::services::fs::hvfs::metaslab;
pub use crate::kernel::services::fs::hvfs::raidz;
pub use crate::kernel::services::fs::hvfs::snapshot;
pub use crate::kernel::services::fs::hvfs::spa;
pub use crate::kernel::services::fs::hvfs::txg;
pub use crate::kernel::services::fs::hvfs::vdev;
pub use crate::kernel::services::fs::hvfs::zap;
pub use crate::kernel::services::fs::hvfs::zil;
pub use crate::kernel::services::fs::hvfs::zil_persist;

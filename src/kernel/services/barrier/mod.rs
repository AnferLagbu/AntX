//! 故障恢复 — 栏栈恢复 (services 层)

pub mod attribution;

pub use attribution::{
    AddrRange, CrossLayerHandler, DomainFailureRecord, FaultAttribution, FaultAttributor,
    TcbModule, MAX_SERVICE_DOMAINS, SERVICE_RANGES, TCB_RANGES,
};

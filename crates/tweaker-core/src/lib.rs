//! Platform-neutral core of WinTweaker.
//!
//! This crate knows nothing about the Windows API or about the UI toolkit.
//! Everything Windows-specific lives in `tweaker-windows`, everything visual in
//! `tweaker-app`. The clean-room rules put the catalog and the
//! apply/detect/revert engine here, so they can be tested on fake adapters.

pub mod action;
pub mod adapters;
pub mod catalog;
pub mod engine;
pub mod fake;
pub mod journal;
pub mod lang;
pub mod logging;
pub mod model;
pub mod paths;
pub mod product;
pub mod sections;

#[cfg(test)]
mod test_support;

pub use action::{Action, RegistryKey, RegistryRoot, RegistryValue, ServiceStartMode};
pub use adapters::{
    AdapterError, FirewallDirection, FirewallRule, IAppx, IFileSystem, IFirewall, IProcessRunner,
    IRegistry, IScheduler, IServiceControl, ISystemInfo, OsVersion, ProcessOutput, ServiceState,
    ServiceStatus,
};
pub use catalog::{CatalogError, load_privacy};
pub use engine::{ApplyReport, Engine, FailedAction, PlanError, RevertReport};
pub use journal::{AppliedOp, Journal, JournalRecord, Phase, Snapshot};
pub use lang::Lang;
pub use logging::{Level, Logger, LoggerConfig};
pub use model::{Plan, PlanStep, RiskLevel, TweakDefinition, TweakState};
pub use paths::ProductPaths;
pub use product::{PRODUCT_DIR_NAME, PRODUCT_NAME, PRODUCT_VERSION};
pub use sections::{Section, SectionKind};

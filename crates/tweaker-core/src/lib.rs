//! Platform-neutral core of WinTweaker.
//!
//! This crate knows nothing about the Windows API or about the UI toolkit.
//! Everything Windows-specific lives in `tweaker-windows`, everything visual in
//! `tweaker-app`. The rules of the clean-room package put the catalog and the
//! apply/detect/revert engine here, so they can be tested on fake adapters.

pub mod lang;
pub mod logging;
pub mod paths;
pub mod product;
pub mod sections;

pub use lang::Lang;
pub use logging::{Level, Logger, LoggerConfig};
pub use paths::ProductPaths;
pub use product::{PRODUCT_DIR_NAME, PRODUCT_NAME, PRODUCT_VERSION};
pub use sections::{Section, SectionKind};

#[cfg(test)]
mod test_support;

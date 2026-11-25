//! Windows adapters for WinTweaker.
//!
//! This crate is the only place that talks to Win32 and to the real file system:
//! elevation, system language, OS information, registry, files, processes.
//! The core stays platform-neutral and testable.
//!
//! Every function here must be a thin wrapper with an explicit effect. Starting
//! the program does not change the system (SPEC.md 4.9).

#[cfg(not(windows))]
compile_error!("tweaker-windows supports Windows only");

pub mod appx;
pub mod elevation;
pub mod files;
pub mod locale;
pub mod message;
pub mod os;
pub mod process;
pub mod registry;
pub mod scheduler;
pub mod services;
pub mod single_instance;
pub mod startup;

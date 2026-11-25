//! Windows adapters for WinTweaker.
//!
//! This crate is the only place that talks to Win32: elevation, system language,
//! OS information. The core stays platform-neutral and testable.
//!
//! Every function here must be a thin, side-effect-free wrapper unless its name
//! says otherwise. Starting the program does not change the system (SPEC.md 4.9).

#[cfg(not(windows))]
compile_error!("tweaker-windows supports Windows only");

pub mod elevation;
pub mod locale;
pub mod os;

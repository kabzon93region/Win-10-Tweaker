//! Adapter interfaces: the only bridge between the engine and the real system.
//!
//! The core crate never calls Win32 directly. Every effect goes through one of
//! these traits, so the whole engine can be tested on in-memory fakes
//! (`crate::fake`) and the same code runs on the real machine through
//! `tweaker-windows`.

use std::fmt;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::action::{RegistryKey, RegistryValue, ServiceStartMode};

/// Failure reported by an adapter.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AdapterError {
    /// Error text shown to the user and written to the journal.
    pub message: String,
}

impl AdapterError {
    /// Builds an error from any message.
    pub fn new(message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
        }
    }
}

impl fmt::Display for AdapterError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.message)
    }
}

impl std::error::Error for AdapterError {}

impl From<std::io::Error> for AdapterError {
    fn from(error: std::io::Error) -> Self {
        Self::new(error.to_string())
    }
}

/// Current run state of a Windows service.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ServiceStatus {
    /// Running.
    Running,
    /// Stopped.
    Stopped,
    /// Paused.
    Paused,
    /// A start or continue is in progress.
    StartPending,
    /// A stop is in progress.
    StopPending,
    /// The service manager reported something unexpected.
    Unknown,
}

/// Full state of a service that the engine needs to snapshot and restore.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct ServiceState {
    /// Current status.
    pub status: ServiceStatus,
    /// Configured start mode.
    pub start_mode: ServiceStartMode,
}

/// Result of a program run.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProcessOutput {
    /// Exit code, or `None` when the process was terminated by a signal/job.
    pub code: Option<i32>,
    /// Captured standard output.
    pub stdout: String,
    /// Captured standard error.
    pub stderr: String,
}

/// Windows version reported by the system.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct OsVersion {
    /// Major version.
    pub major: u32,
    /// Minor version.
    pub minor: u32,
    /// Build number.
    pub build: u32,
}

/// Direction of a firewall rule.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FirewallDirection {
    /// Inbound traffic.
    Inbound,
    /// Outbound traffic.
    Outbound,
}

/// A firewall rule owned by the product (names always carry the product prefix).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FirewallRule {
    /// Rule name; must start with the product prefix.
    pub name: String,
    /// Program the rule applies to.
    pub program: PathBuf,
    /// Direction.
    pub direction: FirewallDirection,
    /// `true` blocks, `false` allows.
    pub block: bool,
}

/// Registry access.
pub trait IRegistry {
    /// Reads a value; `Ok(None)` means the value does not exist.
    fn read(&self, key: &RegistryKey, name: &str) -> Result<Option<RegistryValue>, AdapterError>;
    /// Writes a value, creating the key when needed.
    fn write(
        &self,
        key: &RegistryKey,
        name: &str,
        value: &RegistryValue,
    ) -> Result<(), AdapterError>;
    /// Deletes a value; a missing value is not an error.
    fn delete(&self, key: &RegistryKey, name: &str) -> Result<(), AdapterError>;
}

/// File access. Only text files are handled by the catalog today.
pub trait IFileSystem {
    /// Whether the path exists.
    fn exists(&self, path: &Path) -> bool;
    /// Reads UTF-8 text; `Ok(None)` when the file is missing.
    fn read_text(&self, path: &Path) -> Result<Option<String>, AdapterError>;
    /// Writes UTF-8 text, creating parent directories.
    fn write_text(&self, path: &Path, content: &str) -> Result<(), AdapterError>;
    /// Deletes a file; a missing file is not an error.
    fn delete(&self, path: &Path) -> Result<(), AdapterError>;
}

/// Windows service control.
pub trait IServiceControl {
    /// Reads the current state; `Ok(None)` when the service is not installed.
    fn query(&self, service: &str) -> Result<Option<ServiceState>, AdapterError>;
    /// Starts a service.
    fn start(&self, service: &str) -> Result<(), AdapterError>;
    /// Stops a service.
    fn stop(&self, service: &str) -> Result<(), AdapterError>;
    /// Changes the start mode.
    fn set_start_mode(&self, service: &str, mode: ServiceStartMode) -> Result<(), AdapterError>;
}

/// Running external programs.
pub trait IProcessRunner {
    /// Runs a program to completion and captures its output.
    fn run(&self, program: &str, args: &[String]) -> Result<ProcessOutput, AdapterError>;
}

/// Windows Firewall rules owned by the product.
pub trait IFirewall {
    /// Creates or updates a rule with the product prefix.
    fn ensure_rule(&self, rule: &FirewallRule) -> Result<(), AdapterError>;
    /// Removes a rule by name.
    fn remove_rule(&self, name: &str) -> Result<(), AdapterError>;
    /// Whether a rule exists.
    fn has_rule(&self, name: &str) -> Result<bool, AdapterError>;
}

/// One scheduled task known to the system.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TaskInfo {
    /// Full task path, e.g. `\Microsoft\Office\OfficeTelemetryAgentLogOn`.
    pub path: String,
    /// Whether the task is enabled.
    pub enabled: bool,
}

/// Task Scheduler.
pub trait IScheduler {
    /// Full paths of tasks in one folder (and its subfolders), without states.
    ///
    /// Listing names is cheap; the enabled state is read only for the tasks the
    /// engine actually needs, so a folder with hundreds of tasks stays fast.
    fn list(&self, folder: &str) -> Result<Vec<String>, AdapterError>;
    /// Reads the enabled state; `Ok(None)` when the task does not exist.
    fn is_enabled(&self, task_path: &str) -> Result<Option<bool>, AdapterError>;
    /// Enables or disables a stored task by its full path.
    fn set_enabled(&self, task_path: &str, enabled: bool) -> Result<(), AdapterError>;
}

/// Microsoft Store packages.
pub trait IAppx {
    /// Whether a package with this family name is installed for the user.
    fn installed(&self, package_family: &str) -> Result<bool, AdapterError>;
    /// Removes a package for the current user.
    fn remove(&self, package_family: &str) -> Result<(), AdapterError>;
}

/// Read-only information about the operating system.
pub trait ISystemInfo {
    /// Windows version.
    fn os_version(&self) -> Result<OsVersion, AdapterError>;
}

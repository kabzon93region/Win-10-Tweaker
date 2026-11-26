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

/// Result of a cleanup: how much was removed and what had to be skipped.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct CleanStats {
    /// Bytes of deleted files.
    pub freed_bytes: u64,
    /// Files that were in use and were left alone.
    pub skipped_files: usize,
}

/// One registry key with everything below it.
///
/// Used when an action removes a whole key: the snapshot must keep the values and
/// subkeys so the revert can recreate them.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RegistryTree {
    /// Values of this key, default value included (empty name).
    pub values: Vec<(String, RegistryValue)>,
    /// Subkeys with their own trees, sorted by name.
    pub subkeys: Vec<(String, RegistryTree)>,
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
    /// Reads the whole key tree; `Ok(None)` when the key does not exist.
    fn read_tree(&self, key: &RegistryKey) -> Result<Option<RegistryTree>, AdapterError>;
    /// Deletes a key with all values and subkeys; a missing key is not an error.
    fn delete_tree(&self, key: &RegistryKey) -> Result<(), AdapterError>;
}

/// File access. Only text files are handled by the catalog today.
pub trait IFileSystem {
    /// Whether the path exists.
    fn exists(&self, path: &Path) -> bool;
    /// Reads UTF-8 text; `Ok(None)` when the file is missing.
    fn read_text(&self, path: &Path) -> Result<Option<String>, AdapterError>;
    /// Writes UTF-8 text, creating parent directories.
    fn write_text(&self, path: &Path, content: &str) -> Result<(), AdapterError>;
    /// Reads raw bytes; `Ok(None)` when the file is missing.
    fn read_bytes(&self, path: &Path) -> Result<Option<Vec<u8>>, AdapterError>;
    /// Writes raw bytes, creating parent directories.
    fn write_bytes(&self, path: &Path, bytes: &[u8]) -> Result<(), AdapterError>;
    /// Deletes a file; a missing file is not an error.
    fn delete(&self, path: &Path) -> Result<(), AdapterError>;
    /// Total size of the files under these paths, for the cleanup preview.
    fn measure(&self, paths: &[PathBuf]) -> Result<u64, AdapterError>;
    /// Deletes files under these paths and keeps the roots; locked files are skipped.
    fn clean(&self, paths: &[PathBuf]) -> Result<CleanStats, AdapterError>;
}

/// Where a startup entry comes from.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum StartupSource {
    /// `HKCU\...\Run`.
    HkcuRun,
    /// `HKLM\...\Run`.
    HklmRun,
    /// `HKLM\SOFTWARE\WOW6432Node\...\Run`.
    HklmRun32,
    /// A shortcut in the user's or the common Startup folder.
    StartupFolder,
    /// A task in the Task Scheduler library (non-Microsoft folder).
    ScheduledTask,
}

impl StartupSource {
    /// Stable label used by the UI and the journal preview.
    pub fn label(self) -> &'static str {
        match self {
            StartupSource::HkcuRun => "hkcu_run",
            StartupSource::HklmRun => "hklm_run",
            StartupSource::HklmRun32 => "hklm_run32",
            StartupSource::StartupFolder => "startup_folder",
            StartupSource::ScheduledTask => "scheduled_task",
        }
    }
}

/// One program that starts with Windows.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StartupEntry {
    /// Where the entry is registered.
    pub source: StartupSource,
    /// Value name, file name or full task path.
    pub name: String,
    /// Command line or file path shown to the user.
    pub command: String,
    /// Whether the entry starts with Windows.
    pub enabled: bool,
    /// File or folder to open in Explorer.
    pub location: PathBuf,
}

/// Startup entries as shown in the autorun section.
pub trait IStartup {
    /// All entries from the five sources.
    fn entries(&self) -> Result<Vec<StartupEntry>, AdapterError>;
    /// Opens the entry location in Explorer.
    fn open_location(&self, entry: &StartupEntry) -> Result<(), AdapterError>;
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

/// An installed Store package, enough to remove and restore it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AppxPackage {
    /// Package family name, e.g. `Microsoft.WindowsStore_8wekyb3d8bbwe`.
    pub family: String,
    /// Full package name used by the removal API.
    pub full_name: String,
    /// Path to `AppxManifest.xml` inside the installed location.
    pub manifest: PathBuf,
}

/// Microsoft Store packages.
pub trait IAppx {
    /// Whether a package matching this family token is installed for the user.
    fn installed(&self, package_family: &str) -> Result<bool, AdapterError>;
    /// Package details; `Ok(None)` when no package matches the token.
    fn package(&self, package_family: &str) -> Result<Option<AppxPackage>, AdapterError>;
    /// Removes a package for the current user.
    fn remove(&self, package_family: &str) -> Result<(), AdapterError>;
    /// Registers an existing package manifest for the current user.
    fn register(&self, manifest: &Path) -> Result<(), AdapterError>;
}

/// Read-only information about the operating system.
pub trait ISystemInfo {
    /// Windows version.
    fn os_version(&self) -> Result<OsVersion, AdapterError>;
}

//! Declarative actions: the only way the engine is allowed to change the system.
//!
//! A tweak is data: an ordered list of actions. The engine takes a snapshot of
//! the old state, executes the action through an adapter, re-reads the state and
//! writes everything into the journal. Actions themselves never touch Win32.

use std::fmt;
use std::path::PathBuf;

use serde::{Deserialize, Serialize};

/// Registry root the action works under.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum RegistryRoot {
    /// `HKEY_CURRENT_USER`.
    Hkcu,
    /// `HKEY_LOCAL_MACHINE`.
    Hklm,
    /// `HKEY_CLASSES_ROOT`.
    Hkcr,
    /// `HKEY_USERS`.
    Hku,
}

impl RegistryRoot {
    /// Short name used in `FEATURES.md` and in the UI.
    pub fn as_str(self) -> &'static str {
        match self {
            RegistryRoot::Hkcu => "HKCU",
            RegistryRoot::Hklm => "HKLM",
            RegistryRoot::Hkcr => "HKCR",
            RegistryRoot::Hku => "HKU",
        }
    }

    /// Parses `HKCU`, `HKLM`, `HKCR`, `HKU` (case-insensitive).
    pub fn parse(text: &str) -> Option<Self> {
        match text.trim().to_ascii_uppercase().as_str() {
            "HKCU" | "HKEY_CURRENT_USER" => Some(RegistryRoot::Hkcu),
            "HKLM" | "HKEY_LOCAL_MACHINE" => Some(RegistryRoot::Hklm),
            "HKCR" | "HKEY_CLASSES_ROOT" => Some(RegistryRoot::Hkcr),
            "HKU" | "HKEY_USERS" => Some(RegistryRoot::Hku),
            _ => None,
        }
    }
}

impl fmt::Display for RegistryRoot {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

/// Full path to a registry key: root plus the path below it.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct RegistryKey {
    /// Hive.
    pub root: RegistryRoot,
    /// Path without the root, e.g. `Software\Microsoft\Windows\CurrentVersion`.
    pub path: String,
}

impl RegistryKey {
    /// Builds a key from a root and a path.
    pub fn new(root: RegistryRoot, path: impl Into<String>) -> Self {
        Self {
            root,
            path: path.into().trim_start_matches('\\').to_owned(),
        }
    }
}

impl fmt::Display for RegistryKey {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}\\{}", self.root, self.path)
    }
}

/// Typed registry value. The type is preserved on revert.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", content = "data", rename_all = "snake_case")]
pub enum RegistryValue {
    /// `REG_DWORD`.
    Dword(u32),
    /// `REG_QWORD`.
    Qword(u64),
    /// `REG_SZ`.
    String(String),
    /// `REG_EXPAND_SZ`.
    ExpandString(String),
    /// `REG_MULTI_SZ`.
    MultiString(Vec<String>),
    /// `REG_BINARY`.
    Binary(Vec<u8>),
}

impl RegistryValue {
    /// Short human-readable form for the preview and the journal.
    pub fn preview(&self) -> String {
        match self {
            RegistryValue::Dword(value) => format!("{value} (DWORD)"),
            RegistryValue::Qword(value) => format!("{value} (QWORD)"),
            RegistryValue::String(value) => format!("\"{value}\""),
            RegistryValue::ExpandString(value) => format!("\"{value}\" (expand)"),
            RegistryValue::MultiString(items) => format!("[{}]", items.join(", ")),
            RegistryValue::Binary(bytes) => format!("{} bytes", bytes.len()),
        }
    }
}

/// Service start mode as stored in `HKLM\SYSTEM\CurrentControlSet\Services`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ServiceStartMode {
    /// Loaded by the boot loader.
    Boot,
    /// Started by the kernel.
    System,
    /// Started automatically.
    Automatic,
    /// Started on demand.
    Manual,
    /// Disabled.
    Disabled,
}

impl ServiceStartMode {
    /// Lower-case name for logs and preview.
    pub fn as_str(self) -> &'static str {
        match self {
            ServiceStartMode::Boot => "boot",
            ServiceStartMode::System => "system",
            ServiceStartMode::Automatic => "automatic",
            ServiceStartMode::Manual => "manual",
            ServiceStartMode::Disabled => "disabled",
        }
    }

    /// Numeric value used by the Windows service manager.
    pub fn to_windows(self) -> u32 {
        match self {
            ServiceStartMode::Boot => 0,
            ServiceStartMode::System => 1,
            ServiceStartMode::Automatic => 2,
            ServiceStartMode::Manual => 3,
            ServiceStartMode::Disabled => 4,
        }
    }

    /// Parses the numeric value used by the Windows service manager.
    pub fn from_windows(value: u32) -> Option<Self> {
        match value {
            0 => Some(ServiceStartMode::Boot),
            1 => Some(ServiceStartMode::System),
            2 => Some(ServiceStartMode::Automatic),
            3 => Some(ServiceStartMode::Manual),
            4 => Some(ServiceStartMode::Disabled),
            _ => None,
        }
    }
}

impl fmt::Display for ServiceStartMode {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

/// One reversible change of the system.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Action {
    /// Create or overwrite a registry value.
    RegistrySet {
        /// Key to write into.
        key: RegistryKey,
        /// Value name (empty string means the default value).
        name: String,
        /// New value.
        value: RegistryValue,
    },
    /// Delete a registry value; missing value is not an error.
    RegistryDelete {
        /// Key to delete from.
        key: RegistryKey,
        /// Value name.
        name: String,
    },
    /// Write a UTF-8 text file, creating parent folders if needed.
    FileWrite {
        /// Target file.
        path: PathBuf,
        /// New contents.
        content: String,
    },
    /// Delete a file; missing file is not an error.
    FileDelete {
        /// Target file.
        path: PathBuf,
    },
    /// Start a service.
    ServiceStart {
        /// Service name.
        service: String,
    },
    /// Stop a service.
    ServiceStop {
        /// Service name.
        service: String,
    },
    /// Change the start mode of a service.
    ServiceStartMode {
        /// Service name.
        service: String,
        /// New start mode.
        mode: ServiceStartMode,
    },
    /// Run an external program once. Not revertible.
    ProcessRun {
        /// Executable.
        program: String,
        /// Arguments.
        args: Vec<String>,
    },
}

impl Action {
    /// Technical one-line description for the preview list.
    ///
    /// Deliberately not localized: it shows keys, values and commands as they are.
    pub fn describe(&self) -> String {
        match self {
            Action::RegistrySet { key, name, value } => {
                format!("{key}: {name} = {}", value.preview())
            }
            Action::RegistryDelete { key, name } => format!("{key}: delete value {name}"),
            Action::FileWrite { path, content } => {
                format!("write {} ({} bytes)", path.display(), content.len())
            }
            Action::FileDelete { path } => format!("delete {}", path.display()),
            Action::ServiceStart { service } => format!("start service {service}"),
            Action::ServiceStop { service } => format!("stop service {service}"),
            Action::ServiceStartMode { service, mode } => {
                format!("service {service}: start mode = {mode}")
            }
            Action::ProcessRun { program, args } => {
                let mut text = program.clone();
                for arg in args {
                    text.push(' ');
                    text.push_str(arg);
                }
                format!("run {text}")
            }
        }
    }

    /// `false` for actions with no safe way back (external programs).
    pub fn is_reversible(&self) -> bool {
        !matches!(self, Action::ProcessRun { .. })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn root_parsing_is_case_insensitive() {
        assert_eq!(RegistryRoot::parse("hkcu"), Some(RegistryRoot::Hkcu));
        assert_eq!(
            RegistryRoot::parse("HKEY_LOCAL_MACHINE"),
            Some(RegistryRoot::Hklm)
        );
        assert_eq!(RegistryRoot::parse("nope"), None);
    }

    #[test]
    fn key_display_has_no_double_backslash() {
        let key = RegistryKey::new(RegistryRoot::Hkcu, r"\Software\Test");
        assert_eq!(key.to_string(), r"HKCU\Software\Test");
    }

    #[test]
    fn start_mode_round_trips_through_windows_values() {
        for mode in [
            ServiceStartMode::Boot,
            ServiceStartMode::System,
            ServiceStartMode::Automatic,
            ServiceStartMode::Manual,
            ServiceStartMode::Disabled,
        ] {
            assert_eq!(
                ServiceStartMode::from_windows(mode.to_windows()),
                Some(mode)
            );
        }
        assert_eq!(ServiceStartMode::from_windows(9), None);
    }

    #[test]
    fn descriptions_are_not_empty() {
        let actions = [
            Action::RegistrySet {
                key: RegistryKey::new(RegistryRoot::Hkcu, "Software\\X"),
                name: "Y".into(),
                value: RegistryValue::Dword(1),
            },
            Action::RegistryDelete {
                key: RegistryKey::new(RegistryRoot::Hklm, "Software\\X"),
                name: "Y".into(),
            },
            Action::FileWrite {
                path: PathBuf::from(r"C:\Temp\x.txt"),
                content: "hi".into(),
            },
            Action::FileDelete {
                path: PathBuf::from(r"C:\Temp\x.txt"),
            },
            Action::ServiceStart {
                service: "Spooler".into(),
            },
            Action::ServiceStop {
                service: "Spooler".into(),
            },
            Action::ServiceStartMode {
                service: "Spooler".into(),
                mode: ServiceStartMode::Disabled,
            },
            Action::ProcessRun {
                program: "cmd.exe".into(),
                args: vec!["/c".into(), "echo hi".into()],
            },
        ];
        for action in actions {
            assert!(!action.describe().is_empty());
        }
    }

    #[test]
    fn only_process_run_is_irreversible() {
        assert!(
            !Action::ProcessRun {
                program: "cmd".into(),
                args: vec![],
            }
            .is_reversible()
        );
        assert!(
            Action::FileDelete {
                path: PathBuf::from("x")
            }
            .is_reversible()
        );
    }
}

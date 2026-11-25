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
    /// Delete a whole registry key with its values and subkeys.
    ///
    /// Missing key is not an error. The snapshot keeps the full tree so the
    /// revert can recreate it.
    RegistryDeleteKey {
        /// Key to delete.
        key: RegistryKey,
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
    /// Delete a file whose contents may be binary (shortcuts, icons).
    ///
    /// The snapshot keeps the raw bytes so the revert restores the file exactly.
    FileDeleteAny {
        /// Target file.
        path: PathBuf,
    },
    /// Write a raw binary file, creating parent folders if needed.
    FileWriteBytes {
        /// Target file.
        path: PathBuf,
        /// File contents.
        bytes: Vec<u8>,
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
    /// Run an external program whose effect is undone by another command.
    ///
    /// Used for documented `netsh`/PowerShell toggles: the engine cannot read the
    /// result back, so apply and revert are considered successful when the command
    /// exits with code 0.
    ProcessRunReversible {
        /// Executable for the forward command.
        program: String,
        /// Arguments of the forward command.
        args: Vec<String>,
        /// Executable for the revert command.
        revert_program: String,
        /// Arguments of the revert command.
        revert_args: Vec<String>,
    },
    /// Insert a managed block into a hosts-style file.
    ///
    /// Only the block between the product markers is replaced; all other lines
    /// stay untouched. Applying twice produces the same file.
    HostsBlock {
        /// File to edit, normally `C:\Windows\System32\drivers\etc\hosts`.
        path: PathBuf,
        /// Domains redirected to `0.0.0.0`.
        domains: Vec<String>,
    },
    /// Enable or disable one scheduled task by its full path.
    TaskSetEnabled {
        /// Full task path, e.g. `\Microsoft\Office\OfficeTelemetryAgentLogOn`.
        path: String,
        /// Desired enabled state.
        enabled: bool,
    },
    /// Enable or disable every task in a folder whose name starts with one of the prefixes.
    ///
    /// This mirrors the wildcard notation of the feature list (`NvTmRep*`) without
    /// hard-coding GUID-suffixed task names.
    TaskSetEnabledMatching {
        /// Folder to scan, e.g. `\Microsoft\Office`; `\` scans the whole tree.
        folder: String,
        /// Name prefixes to match; an empty list matches every task in the folder.
        prefixes: Vec<String>,
        /// Desired enabled state.
        enabled: bool,
        /// Treat "no task matches" as nothing to do instead of an error.
        ///
        /// Used for tasks that belong to optional software (Office, NVIDIA), so a
        /// machine without that software does not become "unavailable".
        #[serde(default)]
        optional: bool,
    },
}

/// First line of the managed hosts block.
pub const HOSTS_BLOCK_START: &str = "# >>> WinTweaker telemetry block >>>";
/// Last line of the managed hosts block.
pub const HOSTS_BLOCK_END: &str = "# <<< WinTweaker telemetry block <<<";

/// Renders the managed block with CRLF line endings, ready to append to a hosts file.
pub fn render_hosts_block(domains: &[String]) -> String {
    let mut text = String::new();
    text.push_str(HOSTS_BLOCK_START);
    text.push_str("\r\n");
    for domain in domains {
        text.push_str("0.0.0.0 ");
        text.push_str(domain);
        text.push_str("\r\n");
    }
    text.push_str(HOSTS_BLOCK_END);
    text.push_str("\r\n");
    text
}

/// Removes the managed block, keeping every other byte as it was.
///
/// Only the span from the start marker line through the end marker line is cut
/// out; line endings, trailing spaces and the bytes after the block are untouched.
/// If the end marker is missing, nothing is removed: a half-written block must not
/// corrupt the file.
pub fn strip_hosts_block(content: &str) -> String {
    let mut result = String::with_capacity(content.len());
    let mut rest = content;
    loop {
        let Some(start) = find_marker_line(rest, HOSTS_BLOCK_START) else {
            result.push_str(rest);
            return result;
        };
        result.push_str(&rest[..start]);
        let after_start = &rest[start..];
        let Some(end) = find_marker_line(after_start, HOSTS_BLOCK_END) else {
            // Broken block: keep the file as it is instead of guessing.
            result.push_str(after_start);
            return result;
        };
        let end_line_start = start + end;
        let end_line_end = line_end_including_eol(&rest[end_line_start..]);
        rest = &rest[end_line_start + end_line_end..];
    }
}

/// Offset of the first line that equals the marker, at a line boundary.
fn find_marker_line(text: &str, marker: &str) -> Option<usize> {
    let mut offset = 0;
    for line in text.split_inclusive('\n') {
        let trimmed = line.trim_end_matches(['\r', '\n']);
        if trimmed == marker {
            return Some(offset);
        }
        offset += line.len();
    }
    None
}

/// Length of the first line including its terminating `\n` (if any).
fn line_end_including_eol(text: &str) -> usize {
    match text.find('\n') {
        Some(position) => position + 1,
        None => text.len(),
    }
}

/// Whether the file already contains exactly this managed block.
pub fn hosts_block_applied(content: &str, domains: &[String]) -> bool {
    content.contains(&render_hosts_block(domains))
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
            Action::RegistryDeleteKey { key } => format!("{key}: delete key with all subkeys"),
            Action::FileWrite { path, content } => {
                format!("write {} ({} bytes)", path.display(), content.len())
            }
            Action::FileWriteBytes { path, bytes } => {
                format!("write {} ({} bytes, binary)", path.display(), bytes.len())
            }
            Action::FileDelete { path } | Action::FileDeleteAny { path } => {
                format!("delete {}", path.display())
            }
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
            Action::ProcessRunReversible {
                program,
                args,
                revert_program,
                revert_args,
            } => {
                let mut text = program.clone();
                for arg in args {
                    text.push(' ');
                    text.push_str(arg);
                }
                text.push_str(" (revert: ");
                text.push_str(revert_program);
                for arg in revert_args {
                    text.push(' ');
                    text.push_str(arg);
                }
                text.push(')');
                text
            }
            Action::HostsBlock { path, domains } => {
                format!(
                    "hosts block: {} entries in {}",
                    domains.len(),
                    path.display()
                )
            }
            Action::TaskSetEnabled { path, enabled } => {
                format!("task {path}: enabled = {enabled}")
            }
            Action::TaskSetEnabledMatching {
                folder,
                prefixes,
                enabled,
                ..
            } => {
                if prefixes.is_empty() {
                    format!("tasks in {folder}: enabled = {enabled}")
                } else {
                    format!(
                        "tasks in {folder} with prefixes {}: enabled = {enabled}",
                        prefixes.join(", ")
                    )
                }
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
        assert!(
            Action::HostsBlock {
                path: PathBuf::from("hosts"),
                domains: vec!["example.com".into()],
            }
            .is_reversible()
        );
    }

    #[test]
    fn hosts_block_round_trips_and_keeps_other_lines() {
        let domains = vec!["a.example".to_owned(), "b.example".to_owned()];
        let original = "127.0.0.1 localhost\r\n# user line\r\n";
        let block = render_hosts_block(&domains);
        let with_block = format!("{original}{block}");
        assert!(hosts_block_applied(&with_block, &domains));

        // Every byte outside the block must survive: same prefix and suffix.
        let stripped = strip_hosts_block(&with_block);
        assert_eq!(stripped, original);
        assert!(!stripped.contains(HOSTS_BLOCK_START));
        assert!(stripped.contains("user line"));
    }

    #[test]
    fn hosts_strip_keeps_the_tail_after_the_block() {
        let block = render_hosts_block(&["a.example".to_owned()]);
        let tail = "10.0.0.1 after.example\r\n";
        let content = format!("{block}{tail}");
        assert_eq!(strip_hosts_block(&content), tail);
    }

    #[test]
    fn hosts_strip_keeps_everything_without_an_end_marker() {
        let content = format!("{HOSTS_BLOCK_START}\r\n0.0.0.0 a.example\r\n");
        assert_eq!(strip_hosts_block(&content), content);
    }

    #[test]
    fn hosts_block_is_idempotent() {
        let domains = vec!["a.example".to_owned()];
        let block = render_hosts_block(&domains);
        let once = format!("{}{block}", strip_hosts_block(""));
        let twice = format!("{}{block}", strip_hosts_block(&once));
        assert_eq!(once, twice);
        assert!(hosts_block_applied(&twice, &domains));
    }

    #[test]
    fn hosts_block_with_other_domains_is_not_applied() {
        let domains = vec!["a.example".to_owned()];
        let block = render_hosts_block(&["b.example".to_owned()]);
        assert!(!hosts_block_applied(&block, &domains));
    }
}

//! Read-only view of the five autorun sources.
//!
//! Toggling and deleting are done through the engine (`RegistrySet` on
//! `StartupApproved`, `FileDeleteAny`, `TaskSetEnabled`), so every change keeps a
//! snapshot and a journal record. This module only reads the current state and
//! opens the entry location.

use std::fs;
use std::io::ErrorKind;
use std::path::PathBuf;

use tweaker_core::adapters::{AdapterError, IScheduler, IStartup, StartupEntry, StartupSource};
use tweaker_core::logging;
use winreg::RegKey;
use winreg::enums::{HKEY_CURRENT_USER, HKEY_LOCAL_MACHINE};

use crate::scheduler::WindowsScheduler;

const RUN: &str = r"Software\Microsoft\Windows\CurrentVersion\Run";
const RUN_MACHINE: &str = r"SOFTWARE\Microsoft\Windows\CurrentVersion\Run";
const RUN_WOW: &str = r"SOFTWARE\WOW6432Node\Microsoft\Windows\CurrentVersion\Run";
const APPROVED: &str = r"Software\Microsoft\Windows\CurrentVersion\Explorer\StartupApproved";

/// Autorun entries of the current machine.
#[derive(Debug, Default, Clone, Copy)]
pub struct WindowsStartup;

impl IStartup for WindowsStartup {
    fn entries(&self) -> Result<Vec<StartupEntry>, AdapterError> {
        let mut entries = Vec::new();
        let mut read_sources = 0usize;
        let mut failures: Vec<String> = Vec::new();
        for source in [
            StartupSource::HkcuRun,
            StartupSource::HklmRun,
            StartupSource::HklmRun32,
        ] {
            let Some((path, approval)) = run_spec(source) else {
                continue;
            };
            match run_entries(path, source, approval) {
                Ok(found) => {
                    read_sources += 1;
                    entries.extend(found);
                }
                Err(error) => failures.push(error.message),
            }
        }
        match startup_folder_entries() {
            Ok(found) => {
                read_sources += 1;
                entries.extend(found);
            }
            Err(error) => failures.push(error.message),
        }
        match task_entries() {
            Ok(found) => {
                read_sources += 1;
                entries.extend(found);
            }
            Err(error) => failures.push(error.message),
        }
        if read_sources == 0 {
            return Err(AdapterError::new(format!(
                "cannot read any autorun source: {}",
                failures.join("; ")
            )));
        }
        for failure in failures {
            logging::warn(&format!("autorun: {failure}"));
        }
        entries.sort_by(|left, right| {
            left.source
                .label()
                .cmp(right.source.label())
                .then_with(|| left.name.to_lowercase().cmp(&right.name.to_lowercase()))
        });
        Ok(entries)
    }

    fn open_location(&self, entry: &StartupEntry) -> Result<(), AdapterError> {
        if entry.location.is_file() {
            std::process::Command::new("explorer.exe")
                .arg(format!("/select,{}", entry.location.display()))
                .spawn()
                .map_err(|error| AdapterError::new(format!("cannot open Explorer: {error}")))?;
            return Ok(());
        }
        if entry.source == StartupSource::StartupFolder {
            let folder = entry
                .location
                .parent()
                .map(PathBuf::from)
                .unwrap_or_else(|| entry.location.clone());
            std::process::Command::new("explorer.exe")
                .arg(folder)
                .spawn()
                .map_err(|error| AdapterError::new(format!("cannot open Explorer: {error}")))?;
            return Ok(());
        }
        Err(AdapterError::new(
            "this entry has no file to show in Explorer".to_owned(),
        ))
    }
}

/// Registry path and `StartupApproved` subkey of one Run source.
fn run_spec(source: StartupSource) -> Option<(&'static str, &'static str)> {
    match source {
        StartupSource::HkcuRun => Some((RUN, "Run")),
        StartupSource::HklmRun => Some((RUN_MACHINE, "Run")),
        StartupSource::HklmRun32 => Some((RUN_WOW, "Run32")),
        StartupSource::StartupFolder | StartupSource::ScheduledTask => None,
    }
}

fn run_entries(
    path: &str,
    source: StartupSource,
    approval: &str,
) -> Result<Vec<StartupEntry>, AdapterError> {
    let hive = match source {
        StartupSource::HkcuRun => RegKey::predef(HKEY_CURRENT_USER),
        _ => RegKey::predef(HKEY_LOCAL_MACHINE),
    };
    let key = match hive.open_subkey(path) {
        Ok(key) => key,
        Err(error) if error.kind() == ErrorKind::NotFound => return Ok(Vec::new()),
        Err(error) => {
            return Err(AdapterError::new(format!(
                "cannot open {path} in the registry: {error}"
            )));
        }
    };
    let approved = approved_state(approval);
    let mut entries = Vec::new();
    for item in key.enum_values() {
        let Ok((name, _)) = item else { continue };
        if name.is_empty() {
            continue;
        }
        let Ok(command) = key.get_value::<String, _>(&name) else {
            continue;
        };
        let enabled = approved.get(&name).copied().unwrap_or(true);
        let location = command_path(&command);
        entries.push(StartupEntry {
            source,
            name,
            command,
            enabled,
            location,
        });
    }
    Ok(entries)
}

/// Reads `StartupApproved\<subkey>`: the first byte is 2/6 for enabled and
/// 3/7 for disabled records.
fn approved_state(subkey: &str) -> std::collections::HashMap<String, bool> {
    let mut states = std::collections::HashMap::new();
    let Ok(key) = RegKey::predef(HKEY_CURRENT_USER).open_subkey(approved_path(subkey)) else {
        return states;
    };
    for item in key.enum_values() {
        let Ok((name, raw)) = item else { continue };
        let first = raw.bytes.first().copied().unwrap_or(2);
        states.insert(name, approved_enabled(first));
    }
    states
}

/// `StartupApproved` marks a record disabled with byte 3 or 7.
fn approved_enabled(first: u8) -> bool {
    !matches!(first, 3 | 7)
}

fn approved_path(subkey: &str) -> String {
    format!("{}\\{}", APPROVED, subkey)
}

fn startup_folder_entries() -> Result<Vec<StartupEntry>, AdapterError> {
    let folders = startup_folders();
    if folders.is_empty() {
        return Err(AdapterError::new(
            "APPDATA and PROGRAMDATA are not set".to_owned(),
        ));
    }
    let mut entries = Vec::new();
    let mut readable = 0usize;
    for base in folders {
        let items = match fs::read_dir(&base) {
            Ok(items) => {
                readable += 1;
                items
            }
            Err(error) if error.kind() == ErrorKind::NotFound => {
                readable += 1;
                continue;
            }
            Err(error) => {
                logging::warn(&format!("autorun: cannot read {}: {error}", base.display()));
                continue;
            }
        };
        for item in items.flatten() {
            let path = item.path();
            if !path.is_file() {
                continue;
            }
            let name = item.file_name().to_string_lossy().to_string();
            let enabled = approved_state("StartupFolder")
                .get(&name)
                .copied()
                .unwrap_or(true);
            entries.push(StartupEntry {
                source: StartupSource::StartupFolder,
                command: path.display().to_string(),
                name,
                enabled,
                location: path,
            });
        }
    }
    if readable == 0 {
        return Err(AdapterError::new(
            "cannot read any Startup folder".to_owned(),
        ));
    }
    Ok(entries)
}

fn startup_folders() -> Vec<PathBuf> {
    let mut folders = Vec::new();
    if let Some(appdata) = std::env::var_os("APPDATA") {
        folders.push(PathBuf::from(appdata).join(r"Microsoft\Windows\Start Menu\Programs\Startup"));
    }
    if let Some(program_data) = std::env::var_os("PROGRAMDATA") {
        folders.push(
            PathBuf::from(program_data).join(r"Microsoft\Windows\Start Menu\Programs\Startup"),
        );
    }
    folders
}

fn task_entries() -> Result<Vec<StartupEntry>, AdapterError> {
    let scheduler = WindowsScheduler;
    let paths = scheduler.list("\\")?;
    let mut entries = Vec::new();
    for path in paths {
        if path == r"\Microsoft\Windows" || path.starts_with(r"\Microsoft\Windows\") {
            continue;
        }
        let enabled = scheduler.is_enabled(&path).ok().flatten().unwrap_or(true);
        entries.push(StartupEntry {
            source: StartupSource::ScheduledTask,
            command: path.clone(),
            name: path.clone(),
            enabled,
            location: PathBuf::new(),
        });
    }
    Ok(entries)
}

/// First token of a command line, when it points to an existing file.
fn command_path(command: &str) -> PathBuf {
    let trimmed = command.trim();
    let candidate = trimmed
        .strip_prefix('"')
        .and_then(|rest| rest.split('"').next())
        .unwrap_or_else(|| trimmed.split(' ').next().unwrap_or(""));
    let path = PathBuf::from(candidate);
    if path.is_file() { path } else { PathBuf::new() }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_at_least_one_source() {
        let startup = WindowsStartup;
        let entries = startup.entries().expect("entries");
        assert!(
            entries.iter().any(|entry| entry.enabled),
            "expected at least one enabled entry or task"
        );
        for entry in entries {
            assert!(!entry.name.is_empty(), "{entry:?}");
        }
    }

    #[test]
    fn command_path_extracts_quoted_program() {
        let path = command_path(r#""C:\Windows\System32\cmd.exe" /c echo"#);
        assert!(path.ends_with("cmd.exe"), "{path:?}");
        assert!(
            command_path("no-such-program-xyz.exe /x")
                .as_os_str()
                .is_empty()
        );
    }

    #[test]
    fn every_run_source_has_a_registry_location() {
        let hkcu = run_spec(StartupSource::HkcuRun).expect("hkcu run");
        assert_eq!(
            hkcu,
            (r"Software\Microsoft\Windows\CurrentVersion\Run", "Run")
        );
        let machine = run_spec(StartupSource::HklmRun).expect("hklm run");
        assert_eq!(
            machine,
            (r"SOFTWARE\Microsoft\Windows\CurrentVersion\Run", "Run")
        );
        let machine32 = run_spec(StartupSource::HklmRun32).expect("hklm run32");
        assert_eq!(
            machine32,
            (
                r"SOFTWARE\WOW6432Node\Microsoft\Windows\CurrentVersion\Run",
                "Run32"
            )
        );
        assert!(run_spec(StartupSource::StartupFolder).is_none());
        assert!(run_spec(StartupSource::ScheduledTask).is_none());
    }

    #[test]
    fn approved_bytes_decode_the_state() {
        for byte in [2, 6] {
            assert!(approved_enabled(byte), "byte {byte} should mean enabled");
        }
        for byte in [3, 7] {
            assert!(!approved_enabled(byte), "byte {byte} should mean disabled");
        }
        assert!(approved_enabled(0));
    }

    #[test]
    fn approved_key_path_is_built() {
        assert_eq!(
            approved_path("Run"),
            r"Software\Microsoft\Windows\CurrentVersion\Explorer\StartupApproved\Run"
        );
    }
}

//! Task Scheduler adapter built on `schtasks.exe`.
//!
//! The feature list itself prescribes `schtasks /Change /TN … /DISABLE`, so the
//! adapter uses the same supported command-line interface instead of COM. Task
//! names are read from the stable CSV column, the enabled flag from the XML form
//! (`<Enabled>false</Enabled>` is present only for disabled tasks).

use std::os::windows::process::CommandExt;
use std::path::PathBuf;
use std::process::Command;

use tweaker_core::adapters::{AdapterError, IScheduler};

/// Prevents a console window from flashing when the GUI calls `schtasks`.
const CREATE_NO_WINDOW: u32 = 0x0800_0000;

/// Task Scheduler access through `schtasks.exe`.
#[derive(Debug, Default, Clone, Copy)]
pub struct WindowsScheduler;

impl IScheduler for WindowsScheduler {
    fn list(&self, folder: &str) -> Result<Vec<String>, AdapterError> {
        let (ok, text) = run(&["/Query", "/FO", "CSV", "/NH"])?;
        if !ok {
            return Err(AdapterError::new(format!(
                "schtasks could not list tasks: {}",
                text.trim()
            )));
        }
        let prefix = folder_prefix(folder);
        let mut paths = Vec::new();
        for line in text.lines() {
            let Some(path) = parse_csv_task_path(line) else {
                continue;
            };
            if path.starts_with(&prefix) {
                paths.push(path);
            }
        }
        paths.sort();
        paths.dedup();
        Ok(paths)
    }

    fn is_enabled(&self, task_path: &str) -> Result<Option<bool>, AdapterError> {
        let (ok, text) = run(&["/Query", "/TN", task_path, "/XML"])?;
        if !ok {
            // A missing task and an unreadable task both mean "not usable here".
            return Ok(None);
        }
        let normalized = text.to_ascii_lowercase();
        Ok(Some(!normalized.contains("<enabled>false</enabled>")))
    }

    fn set_enabled(&self, task_path: &str, enabled: bool) -> Result<(), AdapterError> {
        let flag = if enabled { "/ENABLE" } else { "/DISABLE" };
        let (ok, text) = run(&["/Change", "/TN", task_path, flag])?;
        if ok {
            Ok(())
        } else {
            Err(AdapterError::new(format!(
                "schtasks could not change {task_path}: {}",
                text.trim()
            )))
        }
    }
}

fn folder_prefix(folder: &str) -> String {
    let trimmed = folder.trim_end_matches('\\');
    if trimmed.is_empty() {
        "\\".to_owned()
    } else {
        format!("{trimmed}\\")
    }
}

fn parse_csv_task_path(line: &str) -> Option<String> {
    let line = line.trim_start_matches('\u{feff}').trim();
    let rest = line.strip_prefix('"')?;
    let end = rest.find('"')?;
    let path = &rest[..end];
    if path.starts_with('\\') {
        Some(path.to_owned())
    } else {
        None
    }
}

fn run(args: &[&str]) -> Result<(bool, String), AdapterError> {
    let output = Command::new(schtasks_path())
        .args(args)
        .creation_flags(CREATE_NO_WINDOW)
        .output()
        .map_err(|error| AdapterError::new(format!("cannot run schtasks: {error}")))?;
    let text = if output.stdout.is_empty() {
        decode(&output.stderr)
    } else {
        decode(&output.stdout)
    };
    Ok((output.status.success(), text))
}

/// Absolute path to the system `schtasks.exe`.
///
/// Resolving it through `%SystemRoot%` keeps an elevated process from picking up
/// a same-named binary from `PATH` or the current directory.
fn schtasks_path() -> PathBuf {
    let root = std::env::var_os("SystemRoot")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from(r"C:\Windows"));
    root.join("System32").join("schtasks.exe")
}

/// `schtasks /XML` prints UTF-16LE with a BOM; CSV usually comes as ANSI/UTF-8.
fn decode(bytes: &[u8]) -> String {
    if bytes.starts_with(&[0xFF, 0xFE]) {
        let units: Vec<u16> = bytes[2..]
            .as_chunks::<2>()
            .0
            .iter()
            .map(|chunk| u16::from_le_bytes(*chunk))
            .collect();
        String::from_utf16_lossy(&units)
    } else if bytes.starts_with(&[0xFE, 0xFF]) {
        let units: Vec<u16> = bytes[2..]
            .as_chunks::<2>()
            .0
            .iter()
            .map(|chunk| u16::from_be_bytes(*chunk))
            .collect();
        String::from_utf16_lossy(&units)
    } else {
        String::from_utf8_lossy(bytes).into_owned()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lists_tasks_of_the_root_folder() {
        let scheduler = WindowsScheduler;
        let tasks = scheduler.list("\\").expect("list root");
        assert!(tasks.len() > 10, "expected many scheduled tasks");
        assert!(tasks.iter().all(|path| path.starts_with('\\')));
    }

    #[test]
    fn reads_the_enabled_state_of_a_known_task() {
        let scheduler = WindowsScheduler;
        let state = scheduler
            .is_enabled("\\Microsoft\\Windows\\Defrag\\ScheduledDefrag")
            .expect("query");
        assert!(state.is_some(), "ScheduledDefrag must exist on Windows 10");
    }

    #[test]
    fn missing_task_is_none() {
        let scheduler = WindowsScheduler;
        assert_eq!(
            scheduler
                .is_enabled("\\WinTweaker\\NoSuchTask")
                .expect("query"),
            None
        );
    }

    #[test]
    fn resolves_the_system_schtasks() {
        let path = schtasks_path();
        assert!(path.is_absolute(), "{path:?}");
        assert!(
            path.ends_with(PathBuf::from(r"System32\schtasks.exe")),
            "{path:?}"
        );
    }

    #[test]
    fn csv_paths_are_parsed() {
        assert_eq!(
            parse_csv_task_path(r#""\Microsoft\Office\OfficeTelemetryAgentLogOn","N/A","Ready""#),
            Some(r"\Microsoft\Office\OfficeTelemetryAgentLogOn".to_owned())
        );
        assert_eq!(parse_csv_task_path("Folder: \\Microsoft"), None);
    }
}

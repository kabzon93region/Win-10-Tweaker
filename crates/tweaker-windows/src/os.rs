//! Read-only information about the operating system.
//!
//! Used by `--selftest` now and by the "System information" section later. Only
//! registry reads happen here; nothing is written.

use std::os::windows::process::CommandExt;
use std::process::Command;

use tweaker_core::adapters::{AdapterError, ISystemInfo, OsVersion, ReportRow};
use winreg::RegKey;
use winreg::enums::HKEY_LOCAL_MACHINE;

const CURRENT_VERSION_KEY: &str = r"SOFTWARE\Microsoft\Windows NT\CurrentVersion";

/// Windows edition, version and build, as shown to the user.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OsInfo {
    /// e.g. "Windows 10 Pro".
    pub product: String,
    /// e.g. "22H2".
    pub display_version: String,
    /// e.g. "19045.6332".
    pub build: String,
}

impl OsInfo {
    /// One-line description: "Windows 10 Pro 22H2 (19045.6332)".
    pub fn display(&self) -> String {
        let mut text = self.product.trim().to_owned();
        if !self.display_version.trim().is_empty() {
            text.push(' ');
            text.push_str(self.display_version.trim());
        }
        if !self.build.trim().is_empty() {
            text.push_str(" (");
            text.push_str(self.build.trim());
            text.push(')');
        }
        if text.is_empty() {
            "Windows (version unknown)".to_owned()
        } else {
            text
        }
    }
}

/// Reads the OS description from the registry. Never fails: missing values
/// produce an "unknown" text instead of an error at startup.
pub fn os_info() -> OsInfo {
    let hklm = RegKey::predef(HKEY_LOCAL_MACHINE);
    let Ok(key) = hklm.open_subkey(CURRENT_VERSION_KEY) else {
        return OsInfo {
            product: "Windows".to_owned(),
            display_version: String::new(),
            build: String::new(),
        };
    };
    let product = key
        .get_value::<String, _>("ProductName")
        .unwrap_or_else(|_| "Windows".to_owned());
    let display_version = key
        .get_value::<String, _>("DisplayVersion")
        .or_else(|_| key.get_value::<String, _>("ReleaseId"))
        .unwrap_or_default();
    let build_number = key
        .get_value::<String, _>("CurrentBuildNumber")
        .unwrap_or_default();
    let revision = key
        .get_value::<u32, _>("UBR")
        .map(|value| value.to_string())
        .unwrap_or_default();
    let build = if build_number.is_empty() {
        String::new()
    } else if revision.is_empty() {
        build_number
    } else {
        format!("{build_number}.{revision}")
    };
    OsInfo {
        product,
        display_version,
        build,
    }
}

const CREATE_NO_WINDOW: u32 = 0x0800_0000;

/// System information through registry reads and one CIM query.
#[derive(Debug, Default, Clone, Copy)]
pub struct WindowsSystemInfo;

impl ISystemInfo for WindowsSystemInfo {
    fn os_version(&self) -> Result<OsVersion, AdapterError> {
        let info = os_info();
        let build = info
            .build
            .split('.')
            .next()
            .and_then(|value| value.parse::<u32>().ok())
            .unwrap_or(0);
        Ok(OsVersion {
            major: 10,
            minor: 0,
            build,
        })
    }

    fn report(&self) -> Result<Vec<ReportRow>, AdapterError> {
        let output = Command::new("powershell.exe")
            .args([
                "-NoProfile",
                "-NonInteractive",
                "-ExecutionPolicy",
                "Bypass",
                "-Command",
                REPORT_SCRIPT,
            ])
            .creation_flags(CREATE_NO_WINDOW)
            .output()
            .map_err(|error| AdapterError::new(format!("cannot run PowerShell: {error}")))?;
        if !output.status.success() {
            return Err(AdapterError::new(
                String::from_utf8_lossy(&output.stderr).trim().to_owned(),
            ));
        }
        rows_from_json(&String::from_utf8_lossy(&output.stdout))
    }
}

const REPORT_SCRIPT: &str = r#"
[Console]::OutputEncoding=[System.Text.Encoding]::UTF8;
$ErrorActionPreference='Stop';
$os=Get-CimInstance Win32_OperatingSystem;
$cs=Get-CimInstance Win32_ComputerSystem;
$cpu=Get-CimInstance Win32_Processor|Select-Object -First 1;
$gpu=@(Get-CimInstance Win32_VideoController|ForEach-Object{$_.Name});
$disks=@(Get-CimInstance Win32_LogicalDisk -Filter "DriveType=3"|ForEach-Object{"$($_.DeviceID) $([math]::Round($_.Size/1GB,1)) GB, free $([math]::Round($_.FreeSpace/1GB,1)) GB"});
$net=@(Get-CimInstance Win32_NetworkAdapterConfiguration -Filter "IPEnabled=1"|ForEach-Object{"$($_.Description): $($_.IPAddress -join ', ')"});
$mon=@(Get-CimInstance Win32_DesktopMonitor|ForEach-Object{$_.Name});
[pscustomobject]@{
  os=$os.Caption;
  version="$($os.Version) build $($os.BuildNumber)";
  installed=$os.InstallDate.ToString('yyyy-MM-dd');
  boot=$os.LastBootUpTime.ToString('yyyy-MM-dd HH:mm');
  cpu=$cpu.Name;
  cores=$cpu.NumberOfCores;
  ram=[math]::Round($cs.TotalPhysicalMemory/1GB,1);
  model="$($cs.Manufacturer) $($cs.Model)";
  gpu=$gpu; disks=$disks; net=$net; monitors=$mon
}|ConvertTo-Json -Compress
"#;

/// Turns the CIM JSON into report rows; also used by tests.
fn rows_from_json(text: &str) -> Result<Vec<ReportRow>, AdapterError> {
    let value: serde_json::Value = serde_json::from_str(text.trim())
        .map_err(|error| AdapterError::new(format!("cannot parse system report: {error}")))?;
    let text_of = |key: &str| {
        value
            .get(key)
            .map(|item| match item {
                serde_json::Value::String(text) => text.clone(),
                other => other.to_string(),
            })
            .unwrap_or_default()
    };
    let list_of = |key: &str| {
        value
            .get(key)
            .and_then(|item| item.as_array())
            .map(|items| {
                items
                    .iter()
                    .map(|item| match item {
                        serde_json::Value::String(text) => text.clone(),
                        other => other.to_string(),
                    })
                    .collect::<Vec<_>>()
                    .join("; ")
            })
            .unwrap_or_default()
    };
    let os_name = text_of("os");
    if os_name.trim().is_empty() || os_name == "null" {
        return Err(AdapterError::new("CIM returned no operating system name"));
    }
    Ok(vec![
        ReportRow {
            label: "OS".to_owned(),
            value: os_name,
        },
        ReportRow {
            label: "Version".to_owned(),
            value: text_of("version"),
        },
        ReportRow {
            label: "Installed".to_owned(),
            value: text_of("installed"),
        },
        ReportRow {
            label: "Last boot".to_owned(),
            value: text_of("boot"),
        },
        ReportRow {
            label: "CPU".to_owned(),
            value: format!("{} ({} cores)", text_of("cpu"), text_of("cores")),
        },
        ReportRow {
            label: "RAM".to_owned(),
            value: format!("{} GB", text_of("ram")),
        },
        ReportRow {
            label: "Computer".to_owned(),
            value: text_of("model"),
        },
        ReportRow {
            label: "GPU".to_owned(),
            value: list_of("gpu"),
        },
        ReportRow {
            label: "Monitors".to_owned(),
            value: list_of("monitors"),
        },
        ReportRow {
            label: "Disks".to_owned(),
            value: list_of("disks"),
        },
        ReportRow {
            label: "Network".to_owned(),
            value: list_of("net"),
        },
    ])
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_cim_report_into_rows() {
        let json = r#"{"os":"Windows 10 Pro","version":"10.0.19045 build 19045","installed":"2023-01-01","boot":"2026-01-01 10:00","cpu":"Test CPU","cores":8,"ram":16,"model":"Test PC","gpu":["GPU A","GPU B"],"disks":["C: 100 GB"],"net":["Ethernet: 10.0.0.2"],"monitors":["Monitor"]}"#;
        let rows = rows_from_json(json).expect("rows");
        assert!(
            rows.iter()
                .any(|row| row.label == "OS" && row.value == "Windows 10 Pro")
        );
        assert!(
            rows.iter()
                .any(|row| row.label == "GPU" && row.value.contains("GPU B"))
        );
        assert!(
            rows.iter()
                .any(|row| row.label == "RAM" && row.value == "16 GB")
        );
    }

    #[test]
    fn live_report_contains_os_and_cpu() {
        let rows = WindowsSystemInfo.report().expect("report");
        assert!(rows.iter().any(|row| row.label == "OS"));
        assert!(rows.iter().any(|row| row.label == "CPU"));
    }

    #[test]
    fn os_info_is_readable_and_non_empty() {
        let info = os_info();
        assert!(!info.display().trim().is_empty());
        assert!(info.display().starts_with("Windows"));
    }

    #[test]
    fn display_skips_empty_parts() {
        let info = OsInfo {
            product: "Windows 10 Pro".to_owned(),
            display_version: String::new(),
            build: String::new(),
        };
        assert_eq!(info.display(), "Windows 10 Pro");
    }

    #[test]
    fn unknown_system_has_fallback_text() {
        let info = OsInfo {
            product: String::new(),
            display_version: String::new(),
            build: String::new(),
        };
        assert_eq!(info.display(), "Windows (version unknown)");
    }
}

//! Store package adapter.
//!
//! Packages installed for the current user are listed through the per-user
//! package repository (the same data the WinRT `PackageManager` reads), not by
//! parsing console output. Removal and registration use the documented
//! `Remove-AppxPackage` / `Add-AppxPackage -Register` commands; the engine keeps
//! the manifest path in the journal so a removed package can be registered again
//! while its files are still on disk.

use std::os::windows::process::CommandExt;
use std::path::{Path, PathBuf};
use std::process::Command;

use tweaker_core::adapters::{AdapterError, AppxPackage, IAppx};
use winreg::RegKey;
use winreg::enums::HKEY_CURRENT_USER;

const REPOSITORY: &str = r"Software\Classes\Local Settings\Software\Microsoft\Windows\CurrentVersion\AppModel\Repository\Packages";
const CREATE_NO_WINDOW: u32 = 0x0800_0000;

/// Store packages of the current user.
#[derive(Debug, Default, Clone, Copy)]
pub struct WindowsAppx;

impl IAppx for WindowsAppx {
    fn installed(&self, package_family: &str) -> Result<bool, AdapterError> {
        Ok(self.package(package_family)?.is_some())
    }

    fn package(&self, package_family: &str) -> Result<Option<AppxPackage>, AdapterError> {
        let key = RegKey::predef(HKEY_CURRENT_USER)
            .open_subkey(REPOSITORY)
            .map_err(|error| {
                AdapterError::new(format!("cannot open the package repository: {error}"))
            })?;
        let token = package_family.to_lowercase();
        for item in key.enum_keys() {
            let Ok(full_name) = item else { continue };
            if !full_name.to_lowercase().contains(&token) {
                continue;
            }
            let Ok(package_key) = key.open_subkey(&full_name) else {
                continue;
            };
            let Ok(root) = package_key.get_value::<String, _>("PackageRootFolder") else {
                continue;
            };
            let location = PathBuf::from(root);
            let manifest = location.join("AppxManifest.xml");
            return Ok(Some(AppxPackage {
                family: family_name(&full_name),
                full_name,
                manifest,
            }));
        }
        Ok(None)
    }

    fn remove(&self, package_family: &str) -> Result<(), AdapterError> {
        let Some(package) = self.package(package_family)? else {
            return Err(AdapterError::new(format!(
                "package {package_family} is not installed"
            )));
        };
        let script = format!("Remove-AppxPackage -Package '{}'", package.full_name);
        run_powershell(&script)
    }

    fn register(&self, manifest: &Path) -> Result<(), AdapterError> {
        if !manifest.is_file() {
            return Err(AdapterError::new(format!(
                "{}: package files are gone; install the app from the Store",
                manifest.display()
            )));
        }
        let script = format!(
            "Add-AppxPackage -Register '{}' -DisableDevelopmentMode",
            manifest.display()
        );
        run_powershell(&script)
    }
}

/// `Name_Version_Architecture_ResourceId_PublisherId` → `Name_PublisherId`.
fn family_name(full_name: &str) -> String {
    let name = full_name.split('_').next().unwrap_or(full_name);
    let publisher = full_name.rsplit('_').next().unwrap_or_default();
    if publisher.is_empty() || publisher == name {
        name.to_owned()
    } else {
        format!("{name}_{publisher}")
    }
}

fn run_powershell(script: &str) -> Result<(), AdapterError> {
    let output = Command::new("powershell.exe")
        .args([
            "-NoProfile",
            "-NonInteractive",
            "-ExecutionPolicy",
            "Bypass",
            "-Command",
            script,
        ])
        .creation_flags(CREATE_NO_WINDOW)
        .output()
        .map_err(|error| AdapterError::new(format!("cannot run PowerShell: {error}")))?;
    if output.status.success() {
        return Ok(());
    }
    let stderr = String::from_utf8_lossy(&output.stderr);
    let detail = stderr.trim();
    if detail.is_empty() {
        Err(AdapterError::new(format!(
            "PowerShell exited with code {:?}",
            output.status.code()
        )))
    } else {
        Err(AdapterError::new(detail.to_owned()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn family_name_drops_version_and_architecture() {
        assert_eq!(
            family_name("Microsoft.WindowsStore_12107.1001.12.0_x64__8wekyb3d8bbwe"),
            "Microsoft.WindowsStore_8wekyb3d8bbwe"
        );
        assert_eq!(family_name("single"), "single");
    }

    #[test]
    fn store_package_is_found_when_installed() {
        let appx = WindowsAppx;
        let package = appx.package("Microsoft.WindowsStore").expect("query");
        let Some(package) = package else {
            return;
        };
        assert!(package.family.starts_with("Microsoft.WindowsStore_"));
        assert!(package.manifest.ends_with("AppxManifest.xml"));
        assert_eq!(appx.installed("Microsoft.WindowsStore"), Ok(true));
    }

    #[test]
    fn unknown_package_is_not_installed() {
        let appx = WindowsAppx;
        assert_eq!(appx.installed("WinTweaker.NoSuchPackage.Tests"), Ok(false));
    }
}

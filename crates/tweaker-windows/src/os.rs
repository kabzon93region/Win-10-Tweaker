//! Read-only information about the operating system.
//!
//! Used by `--selftest` now and by the "System information" section later. Only
//! registry reads happen here; nothing is written.

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

#[cfg(test)]
mod tests {
    use super::*;

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

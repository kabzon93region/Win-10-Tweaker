//! Product identity constants.
//!
//! The final product name is still an open decision (D-06 in `docs/DECISIONS.md`);
//! until then the working name "WinTweaker" is used. All code must take the name
//! from here instead of hard-coding it, so a rename is a one-line change.

/// Working product name, shown in the window title and the "About" section.
pub const PRODUCT_NAME: &str = "WinTweaker";

/// Folder name under `%LocalAppData%` for logs, journal and future config.
pub const PRODUCT_DIR_NAME: &str = "WinTweaker";

/// Version of the application crate (`Cargo.toml`), compiled into the binary.
pub const PRODUCT_VERSION: &str = env!("CARGO_PKG_VERSION");

/// Version string for display: "WinTweaker 0.1.0".
pub fn version_string() -> String {
    format!("{PRODUCT_NAME} {PRODUCT_VERSION}")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn version_is_not_empty() {
        assert!(!PRODUCT_VERSION.is_empty());
        assert_eq!(version_string(), format!("WinTweaker {PRODUCT_VERSION}"));
    }

    #[test]
    fn directory_name_is_a_plain_folder_name() {
        assert!(!PRODUCT_DIR_NAME.contains(['\\', '/', ':']));
        assert!(!PRODUCT_DIR_NAME.trim().is_empty());
    }
}

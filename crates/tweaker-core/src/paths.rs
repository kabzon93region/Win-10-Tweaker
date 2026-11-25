//! Filesystem layout of the product.
//!
//! Everything the program writes about itself lives under
//! `%LocalAppData%\WinTweaker\`. Nothing outside that folder is touched by the
//! skeleton; the tweak engine will add explicit, journaled actions later.

use std::fmt;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use crate::product::PRODUCT_DIR_NAME;

/// Resolved directories of one product installation for the current user.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProductPaths {
    root: PathBuf,
}

impl ProductPaths {
    /// Builds the layout for the current user from `%LOCALAPPDATA%`.
    pub fn for_current_user() -> Result<Self, PathsError> {
        let base = std::env::var_os("LOCALAPPDATA")
            .filter(|value| !value.is_empty())
            .map(PathBuf::from)
            .ok_or(PathsError::NoLocalAppData)?;
        Self::from_base(&base)
    }

    /// Builds the layout under an explicit base directory (used by tests).
    pub fn from_base(base: &Path) -> Result<Self, PathsError> {
        if !base.is_absolute() {
            return Err(PathsError::NotAbsolute(base.to_path_buf()));
        }
        Ok(Self {
            root: base.join(PRODUCT_DIR_NAME),
        })
    }

    /// Builds the layout for an already resolved product root directory.
    pub fn with_root(root: impl Into<PathBuf>) -> Self {
        Self { root: root.into() }
    }

    /// `%LocalAppData%\WinTweaker`.
    pub fn root(&self) -> &Path {
        &self.root
    }

    /// Technical log files (see AGENTS.md rule 17).
    pub fn logs(&self) -> PathBuf {
        self.root.join("logs")
    }

    /// Journal of applied actions (see AGENTS.md rule 18).
    pub fn journal(&self) -> PathBuf {
        self.root.join("journal")
    }

    /// User configuration (profiles, API keys in DPAPI-protected form).
    pub fn config(&self) -> PathBuf {
        self.root.join("config")
    }

    /// Exports and backups created on user request.
    pub fn backups(&self) -> PathBuf {
        self.root.join("backups")
    }

    /// Creates the directories the program needs while it runs.
    ///
    /// Only `logs` and `journal` are created at startup: the rest appear when the
    /// user explicitly exports a profile or a backup.
    pub fn ensure_runtime_dirs(&self) -> io::Result<()> {
        fs::create_dir_all(self.logs())?;
        fs::create_dir_all(self.journal())?;
        Ok(())
    }
}

/// Why the product layout could not be resolved.
#[derive(Debug)]
pub enum PathsError {
    /// `%LOCALAPPDATA%` is missing or empty.
    NoLocalAppData,
    /// The base directory is not absolute.
    NotAbsolute(PathBuf),
}

impl fmt::Display for PathsError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            PathsError::NoLocalAppData => {
                write!(f, "%LOCALAPPDATA% is not set, cannot locate product data")
            }
            PathsError::NotAbsolute(path) => {
                write!(f, "base directory is not absolute: {}", path.display())
            }
        }
    }
}

impl std::error::Error for PathsError {}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_dir(tag: &str) -> PathBuf {
        crate::test_support::temp_dir(&format!("paths-{tag}"))
    }

    #[test]
    fn root_is_product_folder_under_base() {
        let base = std::env::temp_dir();
        let paths = ProductPaths::from_base(&base).expect("absolute temp dir");
        assert_eq!(paths.root(), base.join(PRODUCT_DIR_NAME).as_path());
        assert_eq!(paths.logs(), paths.root().join("logs"));
        assert_eq!(paths.journal(), paths.root().join("journal"));
        assert_eq!(paths.config(), paths.root().join("config"));
        assert_eq!(paths.backups(), paths.root().join("backups"));
    }

    #[test]
    fn relative_base_is_rejected() {
        let error = ProductPaths::from_base(Path::new("relative\\dir")).expect_err("must fail");
        assert!(matches!(error, PathsError::NotAbsolute(_)));
    }

    #[test]
    fn runtime_dirs_are_created_only_where_needed() {
        let root = temp_dir("dirs");
        let paths = ProductPaths::with_root(&root);
        paths.ensure_runtime_dirs().expect("create dirs");
        assert!(paths.logs().is_dir());
        assert!(paths.journal().is_dir());
        assert!(!paths.config().exists());
        assert!(!paths.backups().exists());
        fs::remove_dir_all(&root).expect("cleanup");
    }
}

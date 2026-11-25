//! Helpers for unit tests only.
//!
//! Temp folders live under `target/test-tmp` instead of `%TEMP%`: the build must
//! not depend on the temp folder being writable in restricted environments, and
//! `target/` is already ignored by git.

use std::path::PathBuf;
use std::time::{SystemTime, UNIX_EPOCH};

/// Unique scratch directory for one test.
pub fn temp_dir(tag: &str) -> PathBuf {
    let unique = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("clock before 1970")
        .as_nanos();
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../target/test-tmp")
        .join(format!("{tag}-{unique}"))
}

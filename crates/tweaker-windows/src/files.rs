//! Text file adapter: UTF-8 files, creating parent folders on write.

use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use tweaker_core::adapters::{AdapterError, CleanStats, IFileSystem};

/// File access through `std::fs`.
#[derive(Debug, Default, Clone, Copy)]
pub struct WindowsFileSystem;

/// Expands `%VAR%` segments of a catalog path.
fn expand(path: &Path) -> PathBuf {
    let text = path.to_string_lossy();
    let mut result = String::with_capacity(text.len());
    let mut rest = text.as_ref();
    while let Some(start) = rest.find('%') {
        result.push_str(&rest[..start]);
        let after = &rest[start + 1..];
        let Some(end) = after.find('%') else {
            result.push('%');
            result.push_str(after);
            return PathBuf::from(result);
        };
        let name = &after[..end];
        match std::env::var(name) {
            Ok(value) => result.push_str(&value),
            Err(_) => {
                result.push('%');
                result.push_str(name);
                result.push('%');
            }
        }
        rest = &after[end + 1..];
    }
    result.push_str(rest);
    PathBuf::from(result)
}

/// Total size of a file or of every file under a directory.
fn size_of(path: &Path) -> u64 {
    let Ok(metadata) = fs::metadata(path) else {
        return 0;
    };
    if metadata.is_file() {
        return metadata.len();
    }
    let Ok(items) = fs::read_dir(path) else {
        return 0;
    };
    items.flatten().map(|item| size_of(&item.path())).sum()
}

/// Deletes the contents of one path and keeps the path itself.
fn clean_contents(path: &Path, stats: &mut CleanStats) {
    let Ok(metadata) = fs::metadata(path) else {
        return;
    };
    if metadata.is_file() {
        match fs::remove_file(path) {
            Ok(()) => stats.freed_bytes += metadata.len(),
            Err(_) => stats.skipped_files += 1,
        }
        return;
    }
    let Ok(items) = fs::read_dir(path) else {
        return;
    };
    for item in items.flatten() {
        clean_path(&item.path(), stats);
    }
}

/// Deletes the contents of one path; locked files are counted, not fatal.
fn clean_path(path: &Path, stats: &mut CleanStats) {
    let Ok(metadata) = fs::metadata(path) else {
        return;
    };
    if metadata.is_file() {
        match fs::remove_file(path) {
            Ok(()) => stats.freed_bytes += metadata.len(),
            Err(_) => stats.skipped_files += 1,
        }
        return;
    }
    let Ok(items) = fs::read_dir(path) else {
        return;
    };
    for item in items.flatten() {
        clean_path(&item.path(), stats);
    }
    let _ = fs::remove_dir(path);
}

impl IFileSystem for WindowsFileSystem {
    fn exists(&self, path: &Path) -> bool {
        path.is_file()
    }

    fn read_text(&self, path: &Path) -> Result<Option<String>, AdapterError> {
        match fs::read(path) {
            Ok(bytes) => String::from_utf8(bytes)
                .map(Some)
                .map_err(|_| AdapterError::new(format!("{} is not valid UTF-8", path.display()))),
            Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(None),
            Err(error) => Err(AdapterError::new(format!(
                "cannot read {}: {error}",
                path.display()
            ))),
        }
    }

    fn write_text(&self, path: &Path, content: &str) -> Result<(), AdapterError> {
        self.write_bytes(path, content.as_bytes())
    }

    fn read_bytes(&self, path: &Path) -> Result<Option<Vec<u8>>, AdapterError> {
        match fs::read(path) {
            Ok(bytes) => Ok(Some(bytes)),
            Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(None),
            Err(error) => Err(AdapterError::new(format!(
                "cannot read {}: {error}",
                path.display()
            ))),
        }
    }

    fn write_bytes(&self, path: &Path, bytes: &[u8]) -> Result<(), AdapterError> {
        if let Some(parent) = path.parent()
            && !parent.as_os_str().is_empty()
        {
            fs::create_dir_all(parent).map_err(|error| {
                AdapterError::new(format!("cannot create {}: {error}", parent.display()))
            })?;
        }
        fs::write(path, bytes)
            .map_err(|error| AdapterError::new(format!("cannot write {}: {error}", path.display())))
    }

    fn measure(&self, paths: &[PathBuf]) -> Result<u64, AdapterError> {
        Ok(paths.iter().map(|path| size_of(&expand(path))).sum())
    }

    fn clean(&self, paths: &[PathBuf]) -> Result<CleanStats, AdapterError> {
        let mut stats = CleanStats::default();
        for path in paths {
            clean_contents(&expand(path), &mut stats);
        }
        Ok(stats)
    }

    fn delete(&self, path: &Path) -> Result<(), AdapterError> {
        match fs::remove_file(path) {
            Ok(()) => Ok(()),
            Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(()),
            Err(error) => Err(AdapterError::new(format!(
                "cannot delete {}: {error}",
                path.display()
            ))),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    fn test_dir(tag: &str) -> PathBuf {
        let unique = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .expect("clock before 1970")
            .as_nanos();
        PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../../target/test-tmp")
            .join(format!("files-{tag}-{unique}"))
    }

    #[test]
    fn expand_replaces_known_variables_and_keeps_unknown() {
        // SAFETY: the variable name is unique to this test.
        unsafe { std::env::set_var("WINTWEAKER_TEST_DIR", r"C:\Temp\wt") };
        assert_eq!(
            expand(Path::new(r"%WINTWEAKER_TEST_DIR%\cache")),
            PathBuf::from(r"C:\Temp\wt\cache")
        );
        assert_eq!(
            expand(Path::new(r"%WINTWEAKER_NO_SUCH_VAR%\cache")),
            PathBuf::from(r"%WINTWEAKER_NO_SUCH_VAR%\cache")
        );
    }

    #[test]
    fn cleanup_removes_contents_and_keeps_the_root() {
        let dir = test_dir("clean");
        let nested = dir.join("nested");
        fs::create_dir_all(&nested).expect("create dirs");
        fs::write(dir.join("a.tmp"), b"12345").expect("write a");
        fs::write(nested.join("b.tmp"), b"123").expect("write b");
        let files = WindowsFileSystem;
        let roots = [dir.clone()];
        assert_eq!(files.measure(&roots).expect("measure"), 8);
        let stats = files.clean(&roots).expect("clean");
        assert_eq!(stats.freed_bytes, 8);
        assert_eq!(stats.skipped_files, 0);
        assert!(dir.exists(), "the root itself must stay");
        assert!(!dir.join("a.tmp").exists());
        assert!(!nested.exists());
        fs::remove_dir_all(&dir).expect("cleanup");
    }

    #[test]
    fn write_read_delete_round_trip() {
        let dir = test_dir("round-trip");
        let path = dir.join("nested").join("file.txt");
        let files = WindowsFileSystem;
        assert!(!files.exists(&path));
        assert_eq!(files.read_text(&path).expect("read"), None);
        files.write_text(&path, "привет").expect("write");
        assert!(files.exists(&path));
        assert_eq!(
            files.read_text(&path).expect("read").as_deref(),
            Some("привет")
        );
        files.delete(&path).expect("delete");
        assert!(!files.exists(&path));
        files.delete(&path).expect("second delete");
        fs::remove_dir_all(&dir).expect("cleanup");
    }

    #[test]
    fn binary_round_trip() {
        let dir = test_dir("binary");
        let path = dir.join("icon.ico");
        let files = WindowsFileSystem;
        let bytes = vec![0u8, 1, 2, 255];
        files.write_bytes(&path, &bytes).expect("write bytes");
        assert_eq!(files.read_bytes(&path).expect("read"), Some(bytes));
        files.delete(&path).expect("delete");
        assert_eq!(files.read_bytes(&path).expect("read"), None);
        fs::remove_dir_all(&dir).expect("cleanup");
    }
}

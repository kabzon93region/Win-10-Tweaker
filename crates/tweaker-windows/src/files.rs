//! Text file adapter: UTF-8 files, creating parent folders on write.

use std::fs;
use std::io;
use std::path::Path;

use tweaker_core::adapters::{AdapterError, IFileSystem};

/// File access through `std::fs`.
#[derive(Debug, Default, Clone, Copy)]
pub struct WindowsFileSystem;

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

//! Single-instance guard.
//!
//! A second copy would share the journal file with the first one and could revert
//! changes the user is still reviewing, so the window refuses to start while
//! another instance is running. The guard is a session-local named mutex: it is
//! released automatically when the process ends, even after a crash.

use std::io;

use windows::Win32::Foundation::{CloseHandle, ERROR_ALREADY_EXISTS, GetLastError, HANDLE};
use windows::Win32::System::Threading::CreateMutexW;
use windows::core::HSTRING;

/// Holds the named mutex for the lifetime of the process.
#[derive(Debug)]
pub struct SingleInstance {
    handle: HANDLE,
}

impl SingleInstance {
    /// Takes the mutex; `Ok(None)` means another process already holds it.
    pub fn acquire(name: &str) -> io::Result<Option<Self>> {
        let wide = HSTRING::from(name);
        let handle = unsafe { CreateMutexW(None, false, &wide) }
            .map_err(|error| io::Error::other(error.to_string()))?;
        if unsafe { GetLastError() } == ERROR_ALREADY_EXISTS {
            unsafe {
                let _ = CloseHandle(handle);
            }
            return Ok(None);
        }
        Ok(Some(Self { handle }))
    }
}

impl Drop for SingleInstance {
    fn drop(&mut self) {
        unsafe {
            let _ = CloseHandle(self.handle);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::{SystemTime, UNIX_EPOCH};

    fn unique_name() -> String {
        let stamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("clock before 1970")
            .as_nanos();
        format!("Local\\WinTweakerTest-{stamp}")
    }

    #[test]
    fn second_acquire_of_the_same_name_is_refused() {
        let name = unique_name();
        let first = SingleInstance::acquire(&name)
            .expect("first acquire")
            .expect("the name must be free");
        assert!(
            SingleInstance::acquire(&name)
                .expect("second acquire")
                .is_none(),
            "the second instance must be refused"
        );
        drop(first);
        assert!(
            SingleInstance::acquire(&name)
                .expect("third acquire")
                .is_some(),
            "the name is free again after the first guard is dropped"
        );
    }

    #[test]
    fn different_names_do_not_conflict() {
        let first = SingleInstance::acquire(&unique_name())
            .expect("first")
            .expect("free");
        let second = SingleInstance::acquire(&unique_name())
            .expect("second")
            .expect("free");
        drop((first, second));
    }
}

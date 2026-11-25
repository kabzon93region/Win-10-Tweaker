//! Elevation: is the process running as administrator, and a polite restart.
//!
//! * The release binary embeds a `requireAdministrator` manifest, so Windows asks
//!   for elevation before the process starts.
//! * Debug builds use `asInvoker`, so `--selftest` and the UI smoke test can run
//!   from a normal console without a UAC prompt.
//! * If a build without the manifest is started normally, the app asks the user
//!   whether to restart elevated instead of failing silently.

use std::ffi::c_void;
use std::mem::size_of;

use windows::Win32::Foundation::{CloseHandle, HANDLE, HWND};
use windows::Win32::Security::{GetTokenInformation, TOKEN_ELEVATION, TOKEN_QUERY, TokenElevation};
use windows::Win32::System::Threading::{GetCurrentProcess, OpenProcessToken};
use windows::Win32::UI::Shell::{SEE_MASK_NOCLOSEPROCESS, SHELLEXECUTEINFOW, ShellExecuteExW};
use windows::Win32::UI::WindowsAndMessaging::{
    IDYES, MB_ICONERROR, MB_ICONWARNING, MB_OK, MB_YESNO, MESSAGEBOX_RESULT, MESSAGEBOX_STYLE,
    MessageBoxW, SW_SHOWNORMAL,
};
use windows::core::{PCWSTR, w};

/// What happened when the program asked for administrator rights.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ElevationResult {
    /// Already running as administrator.
    AlreadyElevated,
    /// A new elevated instance was started; the current one should exit.
    Relaunched,
    /// The user declined to restart; the program should exit without changes.
    Declined,
    /// Restart failed; the message is already shown to the user.
    Failed(String),
}

/// Returns `true` when the current process has administrator rights.
pub fn is_elevated() -> bool {
    // SAFETY: all handles are checked before use; the token is closed exactly once.
    unsafe {
        let mut token = HANDLE(std::ptr::null_mut());
        if OpenProcessToken(GetCurrentProcess(), TOKEN_QUERY, &mut token).is_err() {
            return false;
        }
        let mut elevation = TOKEN_ELEVATION { TokenIsElevated: 0 };
        let mut returned = 0u32;
        let result = GetTokenInformation(
            token,
            TokenElevation,
            Some(&mut elevation as *mut TOKEN_ELEVATION as *mut c_void),
            size_of::<TOKEN_ELEVATION>() as u32,
            &mut returned,
        );
        let _ = CloseHandle(token);
        result.is_ok() && elevation.TokenIsElevated != 0
    }
}

/// Makes sure the process is elevated.
///
/// If it is not, shows a localized question; when the user agrees, starts a new
/// elevated copy of the same executable with the same command line arguments. The
/// caller must exit on any result other than [`ElevationResult::AlreadyElevated`].
pub fn ensure_elevated(
    title: &str,
    question: &str,
    failure: &str,
    args: &[String],
) -> ElevationResult {
    if is_elevated() {
        return ElevationResult::AlreadyElevated;
    }
    let answer = message_box(title, question, MB_YESNO | MB_ICONWARNING);
    if answer != IDYES {
        return ElevationResult::Declined;
    }
    match relaunch_elevated(args) {
        Ok(()) => ElevationResult::Relaunched,
        Err(error) => {
            let _ = message_box(title, failure, MB_OK | MB_ICONERROR);
            ElevationResult::Failed(error)
        }
    }
}

/// Shows a modal message box with the given style.
fn message_box(title: &str, text: &str, style: MESSAGEBOX_STYLE) -> MESSAGEBOX_RESULT {
    let text = wide(text);
    let title = wide(title);
    // SAFETY: both buffers are NUL-terminated and live until the call returns.
    unsafe { MessageBoxW(None, PCWSTR(text.as_ptr()), PCWSTR(title.as_ptr()), style) }
}

/// Starts a new copy of this executable through the `runas` verb.
fn relaunch_elevated(args: &[String]) -> Result<(), String> {
    let executable = std::env::current_exe().map_err(|error| error.to_string())?;
    let executable = wide(&executable.to_string_lossy());
    let parameters = args
        .iter()
        .map(|arg| quote_arg(arg))
        .collect::<Vec<_>>()
        .join(" ");
    let parameters = wide(&parameters);

    let mut info = SHELLEXECUTEINFOW {
        cbSize: size_of::<SHELLEXECUTEINFOW>() as u32,
        fMask: SEE_MASK_NOCLOSEPROCESS,
        hwnd: HWND::default(),
        lpVerb: w!("runas"),
        lpFile: PCWSTR(executable.as_ptr()),
        lpParameters: PCWSTR(parameters.as_ptr()),
        nShow: SW_SHOWNORMAL.0,
        ..Default::default()
    };
    // SAFETY: `info` is fully initialized and both buffers live until the call returns.
    unsafe {
        ShellExecuteExW(&mut info).map_err(|error| error.to_string())?;
        if !info.hProcess.is_invalid() {
            let _ = CloseHandle(info.hProcess);
        }
    }
    Ok(())
}

/// Quotes one command-line argument the way `CommandLineToArgvW` expects.
fn quote_arg(arg: &str) -> String {
    if !arg.is_empty() && !arg.contains([' ', '\t', '"']) {
        return arg.to_owned();
    }
    let mut quoted = String::with_capacity(arg.len() + 2);
    quoted.push('"');
    let mut backslashes = 0usize;
    for character in arg.chars() {
        match character {
            '\\' => {
                backslashes += 1;
                quoted.push('\\');
            }
            '"' => {
                quoted.push_str(&"\\".repeat(backslashes + 1));
                quoted.push('"');
                backslashes = 0;
            }
            other => {
                backslashes = 0;
                quoted.push(other);
            }
        }
    }
    // Backslashes right before the closing quote must be doubled, otherwise the
    // argument would end with an escaped quote.
    quoted.push_str(&"\\".repeat(backslashes));
    quoted.push('"');
    quoted
}

/// NUL-terminated UTF-16, the form every Win32 text API expects.
fn wide(text: &str) -> Vec<u16> {
    text.encode_utf16().chain(std::iter::once(0)).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn elevation_state_is_stable() {
        assert_eq!(is_elevated(), is_elevated());
    }

    #[test]
    fn wide_strings_are_nul_terminated() {
        assert_eq!(wide("abc"), vec![0x0061, 0x0062, 0x0063, 0x0000]);
    }

    #[test]
    fn arguments_are_quoted_for_the_elevated_copy() {
        assert_eq!(quote_arg("--section"), "--section");
        assert_eq!(quote_arg("C:\\a b\\shot.png"), "\"C:\\a b\\shot.png\"");
        assert_eq!(quote_arg("say \"hi\""), "\"say \\\"hi\\\"\"");
        assert_eq!(quote_arg(""), "\"\"");
        assert_eq!(quote_arg("C:\\a b\\"), "\"C:\\a b\\\\\"");
        assert_eq!(quote_arg("a b\\\\"), "\"a b\\\\\\\\\"");
        assert_eq!(quote_arg("a\\\\"), "a\\\\");
    }
}

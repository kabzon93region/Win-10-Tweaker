//! Small native dialogs that must work before the GUI and the logger exist.
//!
//! Text comes from the localization catalog; this module only draws the dialog.

use windows::Win32::UI::WindowsAndMessaging::{
    MB_ICONINFORMATION, MB_OK, MESSAGEBOX_STYLE, MessageBoxW,
};
use windows::core::HSTRING;

/// Shows a modal information dialog and waits until the user closes it.
pub fn info(title: &str, text: &str) {
    let title = HSTRING::from(title);
    let text = HSTRING::from(text);
    let style: MESSAGEBOX_STYLE = MB_OK | MB_ICONINFORMATION;
    unsafe {
        MessageBoxW(None, &text, &title, style);
    }
}

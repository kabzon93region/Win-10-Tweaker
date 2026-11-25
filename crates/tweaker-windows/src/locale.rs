//! Which language the interface should use.
//!
//! The interface follows the Windows UI language (SPEC.md section 1). Russian
//! systems get Russian, everything else gets English. `WIN_TWEAKER_LANG=ru|en`
//! overrides the choice for testing and screenshots.

use tweaker_core::Lang;
use windows::Win32::Globalization::GetUserDefaultUILanguage;

/// Primary language id of Russian (`LANGID & 0x03FF`).
const LANG_RUSSIAN: u16 = 0x19;

/// Returns the language of the Windows user interface.
pub fn system_ui_language() -> Lang {
    // SAFETY: the function has no parameters and cannot fail.
    let langid = unsafe { GetUserDefaultUILanguage() };
    if langid & 0x03FF == LANG_RUSSIAN {
        Lang::Ru
    } else {
        Lang::En
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn detected_language_is_supported() {
        assert!(Lang::ALL.contains(&system_ui_language()));
    }
}

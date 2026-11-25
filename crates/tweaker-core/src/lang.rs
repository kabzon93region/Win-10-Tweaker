//! Interface languages supported by the product.
//!
//! The language is picked from the Windows UI language (see
//! `tweaker_windows::locale`). Both languages must be complete: a missing string
//! is a test failure, not an empty label on screen (SPEC.md section 11).

/// Language of the user interface.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Lang {
    /// Russian.
    Ru,
    /// English.
    En,
}

impl Lang {
    /// Every supported language, in menu order.
    pub const ALL: [Lang; 2] = [Lang::Ru, Lang::En];

    /// Short code used in file names and in the `WIN_TWEAKER_LANG` override.
    pub fn code(self) -> &'static str {
        match self {
            Lang::Ru => "ru",
            Lang::En => "en",
        }
    }

    /// Parses a short code; `None` for anything we do not ship.
    pub fn from_code(code: &str) -> Option<Self> {
        match code.trim().to_ascii_lowercase().as_str() {
            "ru" => Some(Lang::Ru),
            "en" => Some(Lang::En),
            _ => None,
        }
    }

    /// Name of the language written in that language.
    pub fn native_name(self) -> &'static str {
        match self {
            Lang::Ru => "Русский",
            Lang::En => "English",
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn codes_round_trip() {
        for lang in Lang::ALL {
            assert_eq!(Lang::from_code(lang.code()), Some(lang));
        }
    }

    #[test]
    fn codes_are_case_insensitive_and_trimmed() {
        assert_eq!(Lang::from_code(" RU "), Some(Lang::Ru));
        assert_eq!(Lang::from_code("En"), Some(Lang::En));
        assert_eq!(Lang::from_code("de"), None);
        assert_eq!(Lang::from_code(""), None);
    }

    #[test]
    fn native_names_are_not_empty() {
        for lang in Lang::ALL {
            assert!(!lang.native_name().trim().is_empty());
        }
    }
}

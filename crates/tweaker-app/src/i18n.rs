//! Interface strings.
//!
//! The catalogs are embedded JSON resources, one file per language. The rule from
//! SPEC.md section 11 is strict: both languages must be complete, an empty string
//! or a missing key is a test failure. `Localizer::t` never returns an empty
//! string; an unknown key is shown as `!key` so it is visible immediately.

use std::collections::BTreeMap;
use std::fmt;

use tweaker_core::lang::Lang;

const RU_JSON: &str = include_str!("../i18n/ru.json");
const EN_JSON: &str = include_str!("../i18n/en.json");

/// Flat map of localization keys to texts.
pub type Catalog = BTreeMap<String, String>;

/// Problems found while loading the embedded catalogs.
#[derive(Debug)]
pub enum I18nError {
    /// The JSON resource does not parse or is empty.
    Parse(Lang, String),
    /// A key is empty.
    EmptyKey(Lang),
    /// A value is empty or whitespace-only.
    EmptyValue(Lang, String),
    /// A key exists in one language but not in the other.
    MissingIn(Lang, String),
}

impl fmt::Display for I18nError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            I18nError::Parse(lang, reason) => {
                write!(f, "cannot parse {} catalog: {reason}", lang.code())
            }
            I18nError::EmptyKey(lang) => write!(f, "{} catalog has an empty key", lang.code()),
            I18nError::EmptyValue(lang, key) => {
                write!(f, "{} catalog has an empty value for {key}", lang.code())
            }
            I18nError::MissingIn(lang, key) => {
                write!(f, "{} catalog is missing key {key}", lang.code())
            }
        }
    }
}

impl std::error::Error for I18nError {}

/// Two complete catalogs plus the active language.
pub struct Localizer {
    lang: Lang,
    ru: Catalog,
    en: Catalog,
}

impl Localizer {
    /// Loads the catalogs for the given language, falling back to a minimal
    /// built-in set if the embedded resources are broken (never panics at start).
    pub fn load(lang: Lang) -> Self {
        match Self::try_load(lang) {
            Ok(localizer) => localizer,
            Err(error) => {
                eprintln!("i18n: {error}");
                tweaker_core::logging::error(&format!("i18n: {error}"));
                Self::fallback(lang)
            }
        }
    }

    /// Loads and validates both catalogs.
    pub fn try_load(lang: Lang) -> Result<Self, I18nError> {
        let ru = parse(RU_JSON, Lang::Ru)?;
        let en = parse(EN_JSON, Lang::En)?;
        validate_same_keys(&ru, &en)?;
        Ok(Self { lang, ru, en })
    }

    /// Language currently shown.
    pub fn lang(&self) -> Lang {
        self.lang
    }

    /// Catalog of the given language (used by tests and the self-test).
    pub fn catalog(&self, lang: Lang) -> &Catalog {
        match lang {
            Lang::Ru => &self.ru,
            Lang::En => &self.en,
        }
    }

    /// Text for the key in the active language.
    ///
    /// Falls back to English, then to Russian, then to `!key`; never empty.
    pub fn t(&self, key: &str) -> String {
        self.catalog(self.lang)
            .get(key)
            .or_else(|| self.en.get(key))
            .or_else(|| self.ru.get(key))
            .cloned()
            .unwrap_or_else(|| format!("!{key}"))
    }

    /// Text with simple `{name}` placeholders replaced.
    pub fn fmt(&self, key: &str, args: &[(&str, &str)]) -> String {
        let mut text = self.t(key);
        for (name, value) in args {
            text = text.replace(&format!("{{{name}}}"), value);
        }
        text
    }

    fn fallback(lang: Lang) -> Self {
        let mut ru = Catalog::new();
        let mut en = Catalog::new();
        let pairs = [
            ("app.title", "Твикер Windows", "Windows Tweaker"),
            ("app.window", "Твикер Windows", "Windows Tweaker"),
            (
                "app.badge.no_admin",
                "Без прав администратора",
                "Not elevated",
            ),
            (
                "placeholder.title",
                "Раздел в разработке",
                "Section under construction",
            ),
        ];
        for (key, ru_text, en_text) in pairs {
            ru.insert(key.to_owned(), ru_text.to_owned());
            en.insert(key.to_owned(), en_text.to_owned());
        }
        Self { lang, ru, en }
    }
}

fn parse(json: &str, lang: Lang) -> Result<Catalog, I18nError> {
    let catalog: Catalog =
        serde_json::from_str(json).map_err(|error| I18nError::Parse(lang, error.to_string()))?;
    if catalog.is_empty() {
        return Err(I18nError::Parse(lang, "catalog is empty".to_owned()));
    }
    for (key, value) in &catalog {
        if key.trim().is_empty() {
            return Err(I18nError::EmptyKey(lang));
        }
        if value.trim().is_empty() {
            return Err(I18nError::EmptyValue(lang, key.clone()));
        }
    }
    Ok(catalog)
}

fn validate_same_keys(ru: &Catalog, en: &Catalog) -> Result<(), I18nError> {
    for key in ru.keys() {
        if !en.contains_key(key) {
            return Err(I18nError::MissingIn(Lang::En, key.clone()));
        }
    }
    for key in en.keys() {
        if !ru.contains_key(key) {
            return Err(I18nError::MissingIn(Lang::Ru, key.clone()));
        }
    }
    Ok(())
}

/// Language of the interface: `WIN_TWEAKER_LANG` override, then Windows UI language.
pub fn detect_lang() -> Lang {
    if let Ok(value) = std::env::var("WIN_TWEAKER_LANG")
        && let Some(lang) = Lang::from_code(&value)
    {
        return lang;
    }
    tweaker_windows::locale::system_ui_language()
}

#[cfg(test)]
mod tests {
    use super::*;
    use tweaker_core::sections::load_sections;

    /// Keys the UI code asks for directly.
    const REQUIRED_KEYS: [&str; 39] = [
        "app.title",
        "app.window",
        "app.version_label",
        "app.badge.admin",
        "app.badge.no_admin",
        "app.free",
        "menu.title",
        "kind.catalog",
        "kind.tool",
        "placeholder.title",
        "placeholder.body",
        "status.log",
        "status.journal",
        "status.language",
        "status.version",
        "about.title",
        "about.description",
        "about.license",
        "about.stage",
        "about.data_path",
        "about.log_path",
        "about.journal_path",
        "about.sections",
        "elevation.title",
        "elevation.body",
        "elevation.failed",
        "elevation.declined",
        "selftest.title",
        "selftest.paths",
        "selftest.log",
        "selftest.language",
        "selftest.sections",
        "selftest.admin",
        "selftest.admin_yes",
        "selftest.admin_no",
        "selftest.os",
        "selftest.keys",
        "selftest.ok",
        "selftest.fail",
    ];

    /// Exact number of interface strings shipped in each language.
    const KEY_COUNT: usize = 67;

    #[test]
    fn both_catalogs_load_and_have_equal_keys() {
        for lang in Lang::ALL {
            let localizer = Localizer::try_load(lang).expect("catalogs must load");
            assert_eq!(
                localizer.catalog(Lang::Ru).len(),
                localizer.catalog(Lang::En).len()
            );
            assert_eq!(localizer.catalog(Lang::Ru).len(), KEY_COUNT);
            assert_eq!(localizer.catalog(Lang::En).len(), KEY_COUNT);
        }
    }

    #[test]
    fn no_empty_values_in_either_language() {
        let localizer = Localizer::try_load(Lang::Ru).expect("catalogs must load");
        for lang in Lang::ALL {
            for (key, value) in localizer.catalog(lang) {
                assert!(!value.trim().is_empty(), "empty text for {key} in {lang:?}");
            }
        }
    }

    #[test]
    fn every_section_has_title_and_hint_in_both_languages() {
        let localizer = Localizer::try_load(Lang::Ru).expect("catalogs must load");
        for section in load_sections().expect("catalog") {
            for key in [&section.title_key, &section.hint_key] {
                for lang in Lang::ALL {
                    let text = localizer
                        .catalog(lang)
                        .get(key)
                        .unwrap_or_else(|| panic!("missing {key} in {}", lang.code()));
                    assert!(!text.trim().is_empty());
                }
            }
        }
    }

    #[test]
    fn required_ui_keys_exist_in_both_languages() {
        let localizer = Localizer::try_load(Lang::Ru).expect("catalogs must load");
        for key in REQUIRED_KEYS {
            for lang in Lang::ALL {
                assert!(
                    localizer.catalog(lang).contains_key(key),
                    "missing {key} in {}",
                    lang.code()
                );
            }
        }
    }

    #[test]
    fn active_language_is_used_first() {
        let ru = Localizer::try_load(Lang::Ru).expect("catalogs");
        assert_eq!(ru.t("app.title"), "Твикер Windows");
        let en = Localizer::try_load(Lang::En).expect("catalogs");
        assert_eq!(en.t("app.title"), "Windows Tweaker");
    }

    #[test]
    fn unknown_key_is_visible_but_not_empty() {
        let localizer = Localizer::try_load(Lang::En).expect("catalogs");
        assert_eq!(localizer.t("no.such.key"), "!no.such.key");
    }

    #[test]
    fn formatter_replaces_all_placeholders() {
        let localizer = Localizer::try_load(Lang::En).expect("catalogs");
        let text = localizer.fmt("about.log_path", &[("path", "C:\\Temp\\log.txt")]);
        assert!(text.contains("C:\\Temp\\log.txt"));
        assert!(!text.contains("{path}"));
    }
}

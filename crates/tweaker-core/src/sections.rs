//! Catalog of UI sections, loaded from embedded data (not from code).
//!
//! The list matches `FEATURES.md` and the interface table in `SPEC.md` section 5.
//! Adding or reordering sections is a data change in `data/sections.json`.

use std::fmt;

use serde::Deserialize;

/// What a section contains.
#[derive(Debug, Clone, Copy, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum SectionKind {
    /// A list of catalog items with apply/revert (privacy, ui, services, ...).
    Catalog,
    /// A tool screen without a catalog (system information, VirusTotal, about).
    Tool,
}

/// One entry of the left menu.
#[derive(Debug, Clone, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Section {
    /// Stable identifier used in profiles and logs, e.g. `privacy`.
    pub id: String,
    /// Localization key of the title.
    pub title_key: String,
    /// Localization key of the one-line description.
    pub hint_key: String,
    /// Number of catalog items (0 for tools).
    pub items: u32,
    /// Catalog or tool.
    pub kind: SectionKind,
}

const SECTIONS_JSON: &str = include_str!("../data/sections.json");

/// Errors while loading or validating the section catalog.
#[derive(Debug)]
pub enum CatalogError {
    /// Embedded JSON does not parse.
    Parse(serde_json::Error),
    /// The catalog is empty.
    Empty,
    /// An identifier is empty.
    EmptyId,
    /// Two sections share an identifier.
    DuplicateId(String),
    /// A localization key is empty.
    EmptyKey(String),
}

impl fmt::Display for CatalogError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            CatalogError::Parse(error) => write!(f, "cannot parse section catalog: {error}"),
            CatalogError::Empty => write!(f, "section catalog is empty"),
            CatalogError::EmptyId => write!(f, "section catalog contains an empty id"),
            CatalogError::DuplicateId(id) => write!(f, "duplicate section id: {id}"),
            CatalogError::EmptyKey(key) => write!(f, "empty localization key in section {key}"),
        }
    }
}

impl std::error::Error for CatalogError {}

/// Loads and validates the embedded section catalog.
pub fn load_sections() -> Result<Vec<Section>, CatalogError> {
    let sections: Vec<Section> =
        serde_json::from_str(SECTIONS_JSON).map_err(CatalogError::Parse)?;
    validate(&sections)?;
    Ok(sections)
}

fn validate(sections: &[Section]) -> Result<(), CatalogError> {
    if sections.is_empty() {
        return Err(CatalogError::Empty);
    }
    let mut seen = std::collections::HashSet::new();
    for section in sections {
        if section.id.trim().is_empty() {
            return Err(CatalogError::EmptyId);
        }
        if !seen.insert(section.id.as_str()) {
            return Err(CatalogError::DuplicateId(section.id.clone()));
        }
        if section.title_key.trim().is_empty() || section.hint_key.trim().is_empty() {
            return Err(CatalogError::EmptyKey(section.id.clone()));
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Section ids required by `SPEC.md` section 5 and `FEATURES.md`.
    const EXPECTED_IDS: [&str; 14] = [
        "privacy",
        "context",
        "ui",
        "system",
        "services",
        "cleaner",
        "apps",
        "autorun",
        "optimization",
        "virustotal",
        "sysinfo",
        "backup",
        "maintenance",
        "about",
    ];

    #[test]
    fn catalog_loads_and_has_all_required_sections() {
        let sections = load_sections().expect("catalog must parse");
        let ids: Vec<&str> = sections.iter().map(|s| s.id.as_str()).collect();
        assert_eq!(ids, EXPECTED_IDS);
    }

    #[test]
    fn item_counts_match_features_document() {
        let sections = load_sections().expect("catalog must parse");
        let count = |id: &str| {
            sections
                .iter()
                .find(|s| s.id == id)
                .map(|s| s.items)
                .unwrap_or(0)
        };
        assert_eq!(count("privacy"), 17);
        assert_eq!(count("context"), 17);
        assert_eq!(count("ui"), 17);
        assert_eq!(count("system"), 18); // 17 items + proxy auto-config (SYS-AZ)
        assert_eq!(count("services"), 24);
        assert_eq!(count("cleaner"), 18); // 17 items + auto-clean at logon
        assert_eq!(count("apps"), 24);
        assert_eq!(count("autorun"), 5); // five sources
        assert_eq!(count("optimization"), 6);
        assert_eq!(count("backup"), 4);
        assert_eq!(count("maintenance"), 6);
        // "about" is a tool screen: ABT-01..05 are informational, not catalog items.
        assert_eq!(count("about"), 0);
    }

    #[test]
    fn catalog_sections_have_items() {
        let sections = load_sections().expect("catalog must parse");
        for section in &sections {
            if section.kind == SectionKind::Catalog {
                assert!(section.items > 0, "catalog {} must have items", section.id);
            }
        }
    }

    #[test]
    fn duplicate_ids_are_rejected() {
        let sections = vec![
            Section {
                id: "ui".into(),
                title_key: "section.ui".into(),
                hint_key: "hint.ui".into(),
                items: 1,
                kind: SectionKind::Catalog,
            },
            Section {
                id: "ui".into(),
                title_key: "section.ui".into(),
                hint_key: "hint.ui".into(),
                items: 1,
                kind: SectionKind::Catalog,
            },
        ];
        assert!(matches!(
            validate(&sections),
            Err(CatalogError::DuplicateId(_))
        ));
    }

    #[test]
    fn empty_key_is_rejected() {
        let sections = vec![Section {
            id: "ui".into(),
            title_key: " ".into(),
            hint_key: "hint.ui".into(),
            items: 1,
            kind: SectionKind::Catalog,
        }];
        assert!(matches!(
            validate(&sections),
            Err(CatalogError::EmptyKey(_))
        ));
    }
}

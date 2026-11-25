//! Embedded tweak catalog.
//!
//! Each section is a JSON resource validated at startup: ids, keys, sections and
//! actions are checked before anything can be applied. Adding a tweak is a data
//! change here, not a code change in the UI.

use std::collections::HashSet;
use std::fmt;

use crate::model::{DefinitionError, TweakDefinition};

const PRIVACY_JSON: &str = include_str!("../data/tweaks/privacy.json");

/// Problems found while loading the embedded catalog.
#[derive(Debug)]
pub enum CatalogError {
    /// A section file does not parse.
    Parse(String),
    /// A section has no items.
    Empty,
    /// An item failed its own validation.
    Validation(DefinitionError),
    /// Two items share an id.
    DuplicateId(String),
    /// An item is stored in the wrong section.
    WrongSection {
        /// Item id.
        id: String,
        /// Section of the file.
        expected: String,
        /// Section declared by the item.
        actual: String,
    },
}

impl fmt::Display for CatalogError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            CatalogError::Parse(reason) => write!(f, "cannot parse the catalog: {reason}"),
            CatalogError::Empty => f.write_str("the catalog section is empty"),
            CatalogError::Validation(error) => write!(f, "invalid catalog item: {error}"),
            CatalogError::DuplicateId(id) => write!(f, "duplicate tweak id: {id}"),
            CatalogError::WrongSection {
                id,
                expected,
                actual,
            } => write!(f, "tweak {id} belongs to {actual}, expected {expected}"),
        }
    }
}

impl std::error::Error for CatalogError {}

/// Loads the privacy section of the catalog.
pub fn load_privacy() -> Result<Vec<TweakDefinition>, CatalogError> {
    let tweaks: Vec<TweakDefinition> = serde_json::from_str(PRIVACY_JSON)
        .map_err(|error| CatalogError::Parse(error.to_string()))?;
    validate(&tweaks, "privacy")?;
    Ok(tweaks)
}

fn validate(tweaks: &[TweakDefinition], section: &str) -> Result<(), CatalogError> {
    if tweaks.is_empty() {
        return Err(CatalogError::Empty);
    }
    let mut seen = HashSet::new();
    for tweak in tweaks {
        tweak.validate().map_err(CatalogError::Validation)?;
        if !seen.insert(tweak.id.as_str()) {
            return Err(CatalogError::DuplicateId(tweak.id.clone()));
        }
        if tweak.section != section {
            return Err(CatalogError::WrongSection {
                id: tweak.id.clone(),
                expected: section.to_owned(),
                actual: tweak.section.clone(),
            });
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::action::Action;
    use crate::model::RiskLevel;

    const EXPECTED_IDS: [&str; 11] = [
        "PRIV-01", "PRIV-02", "PRIV-06", "PRIV-07", "PRIV-08", "PRIV-09", "PRIV-10", "PRIV-11",
        "PRIV-12", "PRIV-13", "PRIV-15",
    ];

    #[test]
    fn privacy_catalog_loads_with_known_ids() {
        let tweaks = load_privacy().expect("catalog must load");
        let ids: Vec<&str> = tweaks.iter().map(|tweak| tweak.id.as_str()).collect();
        assert_eq!(ids, EXPECTED_IDS);
    }

    #[test]
    fn every_tweak_has_actions_and_keys() {
        for tweak in load_privacy().expect("catalog") {
            assert!(!tweak.actions.is_empty(), "{} has no actions", tweak.id);
            assert!(tweak.title_key.starts_with("tweak."));
            assert!(tweak.description_key.starts_with("tweak."));
        }
    }

    #[test]
    fn hosts_tweak_contains_the_managed_block() {
        let tweaks = load_privacy().expect("catalog");
        let hosts = tweaks
            .iter()
            .find(|tweak| tweak.id == "PRIV-09")
            .expect("PRIV-09");
        assert_eq!(hosts.risk, RiskLevel::High);
        let block = hosts
            .actions
            .iter()
            .find_map(|action| match action {
                Action::HostsBlock { domains, .. } => Some(domains),
                _ => None,
            })
            .expect("hosts block action");
        assert!(block.len() >= 55, "expected a real domain list");
        assert!(block.iter().all(|domain| !domain.trim().is_empty()));
    }

    #[test]
    fn duplicate_ids_are_rejected() {
        let mut tweaks = load_privacy().expect("catalog");
        tweaks.push(tweaks[0].clone());
        assert!(matches!(
            validate(&tweaks, "privacy"),
            Err(CatalogError::DuplicateId(_))
        ));
    }

    #[test]
    fn wrong_section_is_rejected() {
        let mut tweaks = load_privacy().expect("catalog");
        tweaks[0].section = "ui".to_owned();
        assert!(matches!(
            validate(&tweaks, "privacy"),
            Err(CatalogError::WrongSection { .. })
        ));
    }
}

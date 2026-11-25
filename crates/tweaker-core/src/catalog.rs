//! Embedded tweak catalog.
//!
//! Each section is a JSON resource validated at startup: ids, keys, sections and
//! actions are checked before anything can be applied. Adding a tweak is a data
//! change here, not a code change in the UI.

use std::collections::HashSet;
use std::fmt;

use crate::model::{DefinitionError, TweakDefinition};

const PRIVACY_JSON: &str = include_str!("../data/tweaks/privacy.json");
const SERVICES_JSON: &str = include_str!("../data/tweaks/services.json");

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
    load(PRIVACY_JSON, "privacy")
}

/// Loads the services section of the catalog.
pub fn load_services() -> Result<Vec<TweakDefinition>, CatalogError> {
    load(SERVICES_JSON, "services")
}

/// Loads a catalog section by its id.
///
/// `Ok(None)` means the section is not a catalog (a tool screen) or its data has
/// not been added yet; the UI shows the placeholder in that case.
pub fn load_section(section: &str) -> Result<Option<Vec<TweakDefinition>>, CatalogError> {
    match section {
        "privacy" => load_privacy().map(Some),
        "services" => load_services().map(Some),
        _ => Ok(None),
    }
}

fn load(json: &str, section: &str) -> Result<Vec<TweakDefinition>, CatalogError> {
    let tweaks: Vec<TweakDefinition> =
        serde_json::from_str(json).map_err(|error| CatalogError::Parse(error.to_string()))?;
    validate(&tweaks, section)?;
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

    const EXPECTED_IDS: [&str; 17] = [
        "PRIV-01", "PRIV-02", "PRIV-03", "PRIV-04", "PRIV-05", "PRIV-06", "PRIV-07", "PRIV-08",
        "PRIV-09", "PRIV-10", "PRIV-11", "PRIV-12", "PRIV-13", "PRIV-14", "PRIV-15", "PRIV-16",
        "PRIV-17",
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
    fn services_catalog_loads_with_known_ids() {
        let tweaks = load_services().expect("catalog must load");
        let ids: Vec<&str> = tweaks.iter().map(|tweak| tweak.id.as_str()).collect();
        assert_eq!(ids.len(), 24);
        assert_eq!(ids.first(), Some(&"SVC-01"));
        assert_eq!(ids.last(), Some(&"SVC-24"));
        for tweak in &tweaks {
            assert!(!tweak.actions.is_empty(), "{} has no actions", tweak.id);
            assert_eq!(tweak.section, "services");
            assert!(
                tweak.id.starts_with("SVC-"),
                "{} is not a service id",
                tweak.id
            );
        }
    }

    #[test]
    fn service_risk_matches_the_feature_list() {
        let expected: [(&str, &[&str], RiskLevel, bool); 24] = [
            ("SVC-01", &["PolicyAgent"], RiskLevel::High, false),
            ("SVC-02", &["IKEEXT"], RiskLevel::High, false),
            ("SVC-03", &["WdiServiceHost"], RiskLevel::Medium, false),
            ("SVC-04", &["WdiSystemHost"], RiskLevel::Medium, false),
            ("SVC-05", &["wscsvc"], RiskLevel::Critical, true),
            (
                "SVC-06",
                &["SecurityHealthService"],
                RiskLevel::Critical,
                true,
            ),
            ("SVC-07", &["Themes"], RiskLevel::Medium, false),
            ("SVC-08", &["FontCache"], RiskLevel::Medium, false),
            ("SVC-09", &["WebClient"], RiskLevel::High, false),
            ("SVC-10", &["Dnscache"], RiskLevel::High, false),
            ("SVC-11", &["LanmanServer"], RiskLevel::High, false),
            ("SVC-12", &["lmhosts"], RiskLevel::High, false),
            ("SVC-13", &["wuauserv"], RiskLevel::Critical, true),
            ("SVC-14", &["UsoSvc"], RiskLevel::Critical, true),
            ("SVC-15", &["TabletInputService"], RiskLevel::Medium, false),
            ("SVC-16", &["wmiApSrv"], RiskLevel::Medium, false),
            ("SVC-17", &["WerSvc"], RiskLevel::Medium, false),
            ("SVC-18", &["SENS"], RiskLevel::High, false),
            ("SVC-19", &["WSearch"], RiskLevel::Medium, false),
            ("SVC-20", &["SysMain"], RiskLevel::Medium, false),
            ("SVC-21", &["Spooler"], RiskLevel::High, false),
            ("SVC-22", &["stisvc"], RiskLevel::Medium, false),
            (
                "SVC-23",
                &["SensorService", "SensorDataService", "SensrSvc"],
                RiskLevel::Medium,
                false,
            ),
            ("SVC-24", &["lfsvc"], RiskLevel::Medium, false),
        ];
        let tweaks = load_services().expect("catalog");
        assert_eq!(tweaks.len(), expected.len());
        for (tweak, (id, services, risk, reboot)) in tweaks.iter().zip(expected) {
            assert_eq!(tweak.id, id);
            assert_eq!(tweak.risk, risk, "{id}: risk");
            assert_eq!(tweak.requires_reboot, reboot, "{id}: reboot flag");
            let mut names: Vec<&str> = tweak
                .actions
                .iter()
                .filter_map(|action| match action {
                    Action::ServiceStop { service } | Action::ServiceStartMode { service, .. } => {
                        Some(service.as_str())
                    }
                    _ => None,
                })
                .collect();
            names.sort_unstable();
            names.dedup();
            let mut wanted = services.to_vec();
            wanted.sort_unstable();
            assert_eq!(names, wanted, "{id}: services");
        }
    }

    #[test]
    fn catalog_sections_are_available_by_id() {
        assert!(load_section("privacy").expect("privacy").is_some());
        assert!(load_section("services").expect("services").is_some());
        assert!(load_section("ui").expect("ui").is_none());
        assert!(load_section("sysinfo").expect("sysinfo").is_none());
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

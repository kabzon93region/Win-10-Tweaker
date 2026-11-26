//! Settings profile: a small JSON file with the ids of enabled tweaks.
//!
//! Only ids are stored (decision D-48): import re-plans every item on the current
//! machine instead of writing stale snapshots back.

use serde::{Deserialize, Serialize};

/// Current profile format version.
pub const PROFILE_VERSION: u32 = 1;

/// Parsed profile file.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Profile {
    /// Format version.
    #[serde(default = "default_version")]
    pub version: u32,
    /// Tweak ids in the order they were exported.
    #[serde(default)]
    pub tweaks: Vec<String>,
}

fn default_version() -> u32 {
    PROFILE_VERSION
}

/// Why a profile cannot be read.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ProfileError {
    /// JSON is broken.
    Parse(String),
    /// The file has no known ids.
    Empty,
}

impl std::fmt::Display for ProfileError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ProfileError::Parse(message) => write!(f, "cannot parse the profile: {message}"),
            ProfileError::Empty => write!(f, "the profile contains no tweak ids"),
        }
    }
}

/// Renders a profile with the given ids, sorted and deduplicated.
pub fn export(ids: &[String]) -> String {
    let mut tweaks: Vec<String> = ids.to_vec();
    tweaks.sort();
    tweaks.dedup();
    let profile = Profile {
        version: PROFILE_VERSION,
        tweaks,
    };
    serde_json::to_string_pretty(&profile).unwrap_or_else(|_| "{}".to_owned())
}

/// Reads ids from a profile file.
pub fn parse(text: &str) -> Result<Vec<String>, ProfileError> {
    let profile: Profile =
        serde_json::from_str(text).map_err(|error| ProfileError::Parse(error.to_string()))?;
    if profile.tweaks.is_empty() {
        return Err(ProfileError::Empty);
    }
    if profile.version > PROFILE_VERSION {
        return Err(ProfileError::Parse(format!(
            "unsupported profile version {}",
            profile.version
        )));
    }
    let mut ids = profile.tweaks;
    ids.retain(|id| !id.trim().is_empty());
    ids.sort();
    ids.dedup();
    if ids.is_empty() {
        return Err(ProfileError::Empty);
    }
    Ok(ids)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn export_and_parse_round_trip() {
        let text = export(&[
            "PRIV-01".to_owned(),
            "SVC-02".to_owned(),
            "PRIV-01".to_owned(),
        ]);
        assert!(text.contains("PRIV-01"));
        assert_eq!(
            parse(&text).expect("parse"),
            vec!["PRIV-01".to_owned(), "SVC-02".to_owned()]
        );
    }

    #[test]
    fn empty_profile_is_rejected() {
        assert_eq!(
            parse(r#"{"version":1,"tweaks":[]}"#),
            Err(ProfileError::Empty)
        );
    }

    #[test]
    fn broken_json_is_rejected() {
        assert!(matches!(parse("{oops"), Err(ProfileError::Parse(_))));
    }

    #[test]
    fn newer_version_is_rejected() {
        assert!(matches!(
            parse(r#"{"version":99,"tweaks":["PRIV-01"]}"#),
            Err(ProfileError::Parse(_))
        ));
    }
}
